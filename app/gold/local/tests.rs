use super::*;
use crate::provider::Provider;
use crate::silver::IndexOptions;
use crate::silver::local as silver;
use std::path::{Path, PathBuf};

struct Fixture {
    _directory: tempfile::TempDir,
    database: PathBuf,
    source_root: PathBuf,
    source: silver::Ingestion,
}

fn write_history(source_root: &Path, appended: bool) {
    std::fs::write(source_root.join("session.jsonl"), concat!(
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"silver-thread\"}}\n",
        "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"id\":\"request\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"qdrant filtering\"}]}}\n",
        "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"id\":\"context\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"<environment_context>canonical-only-secret</environment_context>\"}]}}\n",
    )).unwrap();
    if appended {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(source_root.join("session.jsonl"))
            .unwrap();
        file.write_all(concat!(
            r#"{"type":"response_item","payload":{"type":"message","id":"new","role":"user","content":[{"type":"input_text","text":"postgres pooling"}]}}"#,
            "\n",
        ).as_bytes()).unwrap();
    }
}

fn options(root: &Path) -> IndexOptions {
    IndexOptions {
        providers: [(Provider::Codex, vec![root.to_path_buf()])]
            .into_iter()
            .collect(),
        roots_are_authoritative: true,
        since_days: None,
    }
}

fn fixture(rejected: bool) -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("history.sqlite3");
    let source_root = directory.path().join("provider");
    std::fs::create_dir(&source_root).unwrap();
    write_history(&source_root, false);
    if rejected {
        std::fs::write(source_root.join("malformed.jsonl"), "{\n").unwrap();
    }
    let mut storage = Storage::open_writer(&database).unwrap();
    let source = silver::ingest(&mut storage, &options(&source_root)).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 2);
    std::fs::remove_dir_all(&source_root).unwrap();
    Fixture {
        _directory: directory,
        database,
        source_root,
        source,
    }
}

fn canonical(path: &Path) -> Vec<u8> {
    let storage = Storage::open_catalog(path).unwrap();
    // Read complete canonical history, including context-only records. Gold
    // must neither mutate it nor silently drop records excluded from search.
    let connection =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let id: String = connection
        .query_row("SELECT id FROM messages WHERE ordinal = 0", [], |row| {
            row.get("id")
        })
        .unwrap();
    let messages = storage.view(&id, 10).unwrap();
    assert_eq!(messages.len(), 2);
    assert!(messages[1].content.contains("canonical-only-secret"));
    serde_json::to_vec(&messages).unwrap()
}

#[test]
fn failed_dense_build_preserves_silver_and_publishes_completed_lexical_artifact() {
    let fixture = fixture(false);
    let before = canonical(&fixture.database);
    let missing = fixture.database.with_file_name("missing-models");
    let models = Models::new(missing.clone());
    let stored = Storage::open_catalog(&fixture.database).unwrap();
    assert!(stored.derived_search_is_dirty().unwrap());
    assert!(stored.search("qdrant", 5, None, None).unwrap().is_empty());
    let mut storage = Storage::open_writer(&fixture.database).unwrap();
    let error = build(
        &mut storage,
        &models,
        &fixture.source.summary,
        &crate::gold::config::Build::default(),
    )
    .err()
    .unwrap();
    assert_eq!(error.error.kind, crate::ErrorKind::Model);
    drop(storage);
    assert_eq!(canonical(&fixture.database), before);
    assert_eq!(
        Storage::open_catalog(&fixture.database)
            .unwrap()
            .counts()
            .unwrap()
            .embeddings,
        0
    );
    assert!(Storage::open_search(&fixture.database, &models.generation()).is_err());
    let lexical = Storage::open_lexical(&fixture.database).unwrap();
    assert!(lexical.gold_manifest().unwrap().is_none());
    assert_eq!(
        lexical
            .lexical_manifest()
            .unwrap()
            .unwrap()
            .searchable_messages,
        1
    );
    assert!(!missing.exists());
    assert!(!fixture.source_root.exists());
}

#[derive(Clone, Copy)]
enum Resume {
    SameWriter,
    Reopen,
}

#[test]
fn lexical_only_build_is_model_free_and_replaces_dense_storage() {
    let fixture = fixture(false);
    let before = canonical(&fixture.database);
    let models = Models::new(fixture.database.with_file_name("absent-models"));
    let recipe = crate::gold::config::Build {
        representations: crate::gold::config::Representations::LexicalOnly,
        ..Default::default()
    };
    let mut storage = Storage::open_writer(&fixture.database).unwrap();
    let silver = storage.silver_certificate().unwrap();
    let result = build(&mut storage, &models, &fixture.source.summary, &recipe).unwrap();
    assert!(result.ready);
    assert_eq!(result.model_load_milliseconds, 0);
    assert_eq!(result.embeddings.model_inferences, 0);
    assert_eq!(result.counts.embeddings, 0);
    storage.commit_writer().unwrap();
    assert_eq!(storage.gold_manifest().unwrap().unwrap().build, recipe);
    drop(storage);
    let reader = Storage::open_lexical(&fixture.database).unwrap();
    assert_eq!(reader.silver_certificate().unwrap(), silver);
    assert_eq!(canonical(&fixture.database), before);
    assert!(!fixture.source_root.exists());
}

#[test]
#[ignore = "requires installed MASH_TEST_MODELS_DIR"]
fn artifact_switch_replaces_dense_gold_over_unchanged_silver() {
    let fixture = fixture(false);
    let before = canonical(&fixture.database);
    let models = Models::new(PathBuf::from(
        std::env::var("MASH_TEST_MODELS_DIR").unwrap(),
    ));
    for representations in [
        crate::gold::config::Representations::LexicalAndDense,
        crate::gold::config::Representations::LexicalOnly,
        crate::gold::config::Representations::LexicalAndDense,
    ] {
        let recipe = crate::gold::config::Build {
            representations,
            ..Default::default()
        };
        let mut storage = Storage::open_writer(&fixture.database).unwrap();
        let silver = storage.silver_certificate().unwrap();
        let result = build(&mut storage, &models, &fixture.source.summary, &recipe).unwrap();
        assert!(result.ready);
        assert_eq!(result.counts.embeddings, recipe.expected_embeddings(1));
        assert_eq!(
            result.embeddings.model_inferences,
            recipe.expected_embeddings(1)
        );
        storage.commit_writer().unwrap();
        assert_eq!(storage.gold_manifest().unwrap().unwrap().build, recipe);
        assert_eq!(storage.silver_certificate().unwrap(), silver);
        assert_eq!(canonical(&fixture.database), before);
        assert!(Storage::open_lexical(&fixture.database).is_ok());
        assert!(!fixture.source_root.exists());
    }
}

#[test]
#[ignore = "requires installed MASH_TEST_MODELS_DIR"]
fn recipe_switch_replaces_vectors_without_changing_silver_or_reading_sources() {
    use crate::gold::config::{Build as Recipe, EmbeddingText};
    let fixture = fixture(false);
    let models = Models::new(PathBuf::from(
        std::env::var("MASH_TEST_MODELS_DIR").unwrap(),
    ));
    let before = canonical(&fixture.database);
    let certificate = Storage::open_catalog(&fixture.database)
        .unwrap()
        .silver_certificate()
        .unwrap();
    let dialogue = Recipe::default();
    let titled = Recipe {
        max_embedding_chars: None,
        representations: crate::gold::config::Representations::LexicalAndDense,
        embedding_text: EmbeddingText::TitleAndDialogue,
    };
    let budgeted = Recipe {
        max_embedding_chars: Some(256_u32.try_into().unwrap()),
        ..titled.clone()
    };
    let mut previous_vector = None;
    for recipe in [&dialogue, &titled, &budgeted, &dialogue] {
        let mut storage = Storage::open_writer(&fixture.database).unwrap();
        if previous_vector.is_some() {
            assert!(
                storage
                    .require_gold_build(recipe, &models.generation())
                    .is_err()
            );
        }
        let result = build(&mut storage, &models, &fixture.source.summary, recipe).unwrap();
        assert!(result.ready);
        assert_eq!(result.embeddings.model_inferences, 1);
        storage.commit_writer().unwrap();
        storage
            .require_gold_build(recipe, &models.generation())
            .unwrap();
        assert_eq!(storage.silver_certificate().unwrap(), certificate);
        assert_eq!(storage.gold_manifest().unwrap().unwrap().build, *recipe);
        let connection = rusqlite::Connection::open(&fixture.database).unwrap();
        let generations: i64 = connection
            .query_row(
                "SELECT COUNT(DISTINCT generation) FROM message_embeddings",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(generations, 1);
        let vector: Vec<u8> = connection
            .query_row("SELECT vector FROM message_embeddings LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        if let Some(previous) = previous_vector.replace(vector.clone()) {
            if recipe.max_embedding_chars.is_some() {
                // This short fixture fits both recipes; identity still changes.
                assert_eq!(previous, vector);
            } else {
                assert_ne!(previous, vector, "title context changes representation");
            }
        }
        assert_eq!(canonical(&fixture.database), before);
        assert!(!fixture.source_root.exists());
        let generation = recipe.generation(&models.generation());
        assert_eq!(
            Storage::open_search(&fixture.database, &generation)
                .unwrap()
                .search("qdrant", 5, None, None)
                .unwrap()
                .len(),
            1
        );
    }
    let mut storage = Storage::open_existing(&fixture.database).unwrap();
    let connection = rusqlite::Connection::open(&fixture.database).unwrap();
    let thread: String = connection
        .query_row("SELECT id FROM conversations LIMIT 1", [], |r| r.get(0))
        .unwrap();
    assert!(storage.forget(&thread).unwrap());
    storage
        .require_gold_build(&dialogue, &models.generation())
        .unwrap();
    assert_eq!(storage.counts().unwrap().messages, 0);
    assert!(storage.silver_certificate().unwrap().revision > certificate.revision);
}

#[test]
#[ignore = "requires installed MASH_TEST_MODELS_DIR"]
fn incremental_gold_preserves_vectors_and_reopened_gold_recovers_without_sources() {
    let models = Models::new(PathBuf::from(
        std::env::var("MASH_TEST_MODELS_DIR").unwrap(),
    ));
    for resume in [Resume::SameWriter, Resume::Reopen] {
        let fixture = fixture(false);
        let mut storage = Storage::open_writer(&fixture.database).unwrap();
        assert!(
            build(
                &mut storage,
                &models,
                &fixture.source.summary,
                &crate::gold::config::Build::default()
            )
            .unwrap()
            .ready
        );
        storage.commit_writer().unwrap();
        drop(storage);
        std::fs::create_dir(&fixture.source_root).unwrap();
        write_history(&fixture.source_root, true);
        let mut storage = Storage::open_writer(&fixture.database).unwrap();
        let source = silver::ingest(&mut storage, &options(&fixture.source_root)).unwrap();
        assert_eq!(
            storage.counts().unwrap().embeddings,
            1,
            "unchanged vector retained by silver"
        );
        assert!(storage.derived_search_is_dirty().unwrap());
        assert!(Storage::open_search(&fixture.database, &models.generation()).is_err());
        assert!(
            storage
                .search("postgres", 5, None, None)
                .unwrap()
                .is_empty(),
            "silver has not built new FTS entries"
        );
        std::fs::remove_dir_all(&fixture.source_root).unwrap();
        if matches!(resume, Resume::Reopen) {
            // Silver checkpoints are durable. Lost temporary delta queues
            // require recovery from the canonical rows, without source files.
            drop(storage);
            storage = Storage::open_writer(&fixture.database).unwrap();
        }
        let result = build(
            &mut storage,
            &models,
            &source.summary,
            &crate::gold::config::Build::default(),
        )
        .unwrap();
        assert!(result.ready);
        assert_eq!(result.counts.messages, 3);
        assert_eq!(result.counts.embeddings, 2);
        assert_eq!(
            result.embeddings.model_inferences,
            match resume {
                Resume::SameWriter => 1,
                Resume::Reopen => 2,
            }
        );
        storage.commit_writer().unwrap();
        let search = Storage::open_search(&fixture.database, &models.generation()).unwrap();
        for query in ["qdrant", "postgres"] {
            assert_eq!(search.search(query, 5, None, None).unwrap().len(), 1);
        }
        assert!(
            search
                .search("canonical-only-secret", 5, None, None)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
#[ignore = "requires installed MASH_TEST_MODELS_DIR"]
fn gold_rebuild_from_stored_history_survives_source_removal_and_is_idempotent() {
    let fixture = fixture(false);
    let before = canonical(&fixture.database);
    let models = Models::new(PathBuf::from(
        std::env::var("MASH_TEST_MODELS_DIR").unwrap(),
    ));
    for (rebuild, expected_inferences) in [(false, 1), (false, 0), (true, 1)] {
        let mut storage = Storage::open_writer(&fixture.database).unwrap();
        if rebuild {
            storage.defer_search_updates().unwrap();
            storage.mark_derived_search_dirty().unwrap();
        }
        let result = build(
            &mut storage,
            &models,
            &fixture.source.summary,
            &crate::gold::config::Build::default(),
        )
        .unwrap();
        assert!(result.ready);
        assert_eq!(result.counts.messages, 2);
        assert_eq!(result.counts.searchable_messages, 1);
        assert_eq!(result.counts.embeddings, 1);
        assert_eq!(result.embeddings.model_inferences, expected_inferences);
        if expected_inferences == 0 {
            assert_eq!(result.model_load_milliseconds, 0);
        }
        storage.commit_writer().unwrap();
        drop(storage);
        assert_eq!(canonical(&fixture.database), before);
        let search = Storage::open_search(&fixture.database, &models.generation()).unwrap();
        assert_eq!(search.search("qdrant", 5, None, None).unwrap().len(), 1);
        assert!(
            search
                .search("canonical-only-secret", 5, None, None)
                .unwrap()
                .is_empty()
        );
        assert!(!fixture.source_root.exists());
    }
}

#[test]
#[ignore = "requires installed MASH_TEST_MODELS_DIR"]
fn rejected_source_keeps_gold_unready_even_with_complete_stored_vectors() {
    let mut fixture = fixture(true);
    assert_eq!(fixture.source.summary.malformed_records, 1);
    // A stale caller's success summary/certificate cannot override durable evidence.
    fixture.source.summary.malformed_records = 0;
    fixture.source.certificate.completeness = crate::silver::certificate::Completeness::Complete;
    let models = Models::new(PathBuf::from(
        std::env::var("MASH_TEST_MODELS_DIR").unwrap(),
    ));
    let mut storage = Storage::open_writer(&fixture.database).unwrap();
    let result = build(
        &mut storage,
        &models,
        &fixture.source.summary,
        &crate::gold::config::Build::default(),
    )
    .unwrap();
    assert_eq!(result.counts.embeddings, result.counts.searchable_messages);
    assert!(!result.ready);
    storage.commit_writer().unwrap();
    assert!(Storage::open_search(&fixture.database, &models.generation()).is_err());
}
