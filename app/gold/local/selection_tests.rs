use super::*;
use crate::gold::config::{Build as Recipe, DenseContent};
use crate::provider::Provider;
use crate::silver::{IndexOptions, local::ingest};
use std::fmt::Write;
use std::path::{Path, PathBuf};

fn history(root: &Path, roles: [&str; 2]) {
    std::fs::create_dir_all(root).unwrap();
    let mut text = "{\"type\":\"session_meta\",\"payload\":{\"id\":\"thread\"}}\n".to_owned();
    for (ordinal, role) in roles.into_iter().enumerate() {
        writeln!(text, "{{\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"id\":\"native-{ordinal}\",\"role\":\"{role}\",\"content\":[{{\"type\":\"input_text\",\"text\":\"reply evidence {ordinal}\"}}]}}}}").unwrap();
    }
    std::fs::write(root.join("session.jsonl"), text).unwrap();
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

fn rows(database: &Path) -> Vec<u8> {
    let connection = rusqlite::Connection::open(database).unwrap();
    let id: String = connection
        .query_row(
            "SELECT id FROM messages ORDER BY ordinal LIMIT 1",
            [],
            |row| row.get("id"),
        )
        .unwrap();
    serde_json::to_vec(
        &Storage::open_catalog(database)
            .unwrap()
            .view(&id, 10)
            .unwrap(),
    )
    .unwrap()
}

#[test]
#[ignore = "requires installed MASH_TEST_MODELS_DIR"]
fn parent_units_replace_only_gold_and_recover_after_missing_model_build() {
    use crate::gold::config::RetrievalUnit;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("provider");
    let database = directory.path().join("silver.sqlite3");
    history(&root, ["user", "assistant"]);
    let mut storage = Storage::open_writer(&database).unwrap();
    let source = ingest(&mut storage, &options(&root)).unwrap();
    let silver = storage.silver_certificate().unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let before = rows(&database);
    std::fs::remove_dir_all(&root).unwrap();
    let models = Models::new(PathBuf::from(
        std::env::var_os("MASH_TEST_MODELS_DIR").unwrap(),
    ));
    let default = Recipe::default();
    let parent = Recipe {
        retrieval_unit: RetrievalUnit::ParentAndMessage,
        ..Default::default()
    };
    let mut vectors = Vec::new();
    for recipe in [&default, &parent, &default] {
        let mut storage = Storage::open_writer(&database).unwrap();
        if recipe == &parent {
            let missing = Models::new(directory.path().join("absent"));
            assert!(build(&mut storage, &missing, &source.summary, recipe).is_err());
            assert!(
                storage
                    .require_gold_build(recipe, &models.generation())
                    .is_err()
            );
            assert_eq!(storage.silver_certificate().unwrap(), silver);
        }
        let result = build(&mut storage, &models, &source.summary, recipe).unwrap();
        assert!(result.ready);
        assert_eq!(result.counts.embeddings, 2);
        assert_eq!(storage.silver_certificate().unwrap(), silver);
        assert_eq!(storage.gold_manifest().unwrap().unwrap().build, *recipe);
        storage.commit_writer().unwrap();
        drop(storage);
        assert_eq!(rows(&database), before);
        let connection = rusqlite::Connection::open(&database).unwrap();
        vectors.push(connection.query_row(
            "SELECT vector FROM message_embeddings JOIN messages ON messages.id=message_embeddings.message_id WHERE ordinal=1",
            [], |row| row.get::<_, Vec<u8>>("vector"),
        ).unwrap());
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM gold_manifest WHERE manifest IS NOT NULL",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        let stored =
            Storage::open_search(&database, &recipe.generation(&models.generation())).unwrap();
        assert!(
            stored
                .semantic_index_is_ready(&recipe.generation(&models.generation()))
                .unwrap()
        );
        assert!(!root.exists());
    }
    assert_ne!(vectors[0], vectors[1]);
    assert_eq!(vectors[0], vectors[2]);
}

#[test]
#[ignore = "requires installed MASH_TEST_MODELS_DIR"]
fn parent_edits_reembed_unchanged_child_anchors() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("provider");
    let database = directory.path().join("silver.sqlite3");
    let models = Models::new(PathBuf::from(
        std::env::var_os("MASH_TEST_MODELS_DIR").unwrap(),
    ));
    let recipe = Recipe {
        retrieval_unit: crate::gold::config::RetrievalUnit::ParentAndMessage,
        ..Default::default()
    };
    let mut vectors = Vec::new();
    let mut revisions = Vec::new();
    for replacement in [
        "relational database query optimization",
        "clinical genomic cancer studies",
    ] {
        history(&root, ["user", "assistant"]);
        let path = root.join("session.jsonl");
        let original = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, original.replace("reply evidence 0", replacement)).unwrap();
        let mut storage = Storage::open_writer(&database).unwrap();
        let source = ingest(&mut storage, &options(&root)).unwrap();
        storage.commit_writer().unwrap();
        drop(storage);
        let before = rows(&database);
        std::fs::remove_dir_all(&root).unwrap();
        let mut storage = Storage::open_writer(&database).unwrap();
        revisions.push(storage.silver_certificate().unwrap().revision);
        let result = build(&mut storage, &models, &source.summary, &recipe).unwrap();
        assert!(result.ready);
        assert_eq!(result.embeddings.model_inferences, 2);
        storage.commit_writer().unwrap();
        drop(storage);
        assert_eq!(rows(&database), before);
        let connection = rusqlite::Connection::open(&database).unwrap();
        vectors.push(connection.query_row(
            "SELECT vector FROM message_embeddings JOIN messages ON messages.id=message_embeddings.message_id WHERE ordinal=1",
            [], |row| row.get::<_, Vec<u8>>("vector"),
        ).unwrap());
    }
    assert!(revisions[1] > revisions[0]);
    assert_ne!(vectors[0], vectors[1]);
}

#[test]
fn users_only_empty_eligibility_builds_offline_without_inference() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("provider");
    let database = directory.path().join("silver.sqlite3");
    history(&root, ["assistant", "assistant"]);
    let mut storage = Storage::open_writer(&database).unwrap();
    let source = ingest(&mut storage, &options(&root)).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let before = rows(&database);
    let mut storage = Storage::open_writer(&database).unwrap();
    let silver = storage.silver_certificate().unwrap();
    let models = Models::new(directory.path().join("absent-models"));
    let recipe = Recipe {
        dense_content: DenseContent::UserMessages,
        retrieval_unit: crate::gold::config::RetrievalUnit::ParentAndMessage,
        ..Default::default()
    };
    let result = build(&mut storage, &models, &source.summary, &recipe).unwrap();
    assert!(result.ready);
    assert_eq!(result.embeddings.model_inferences, 0);
    assert_eq!(result.counts.embeddings, 0);
    assert!(!storage.has_pending_embeddings().unwrap());
    assert_eq!(storage.silver_certificate().unwrap(), silver);
    storage.commit_writer().unwrap();
    drop(storage);
    std::fs::remove_dir_all(&root).unwrap();
    let generation = recipe.generation(&models.generation());
    let stored = Storage::open_search(&database, &generation).unwrap();
    assert_eq!(stored.search("reply", 5, None, None).unwrap().len(), 2);
    assert!(stored.semantic_index_is_ready(&generation).unwrap());
    assert_eq!(rows(&database), before);
    assert!(!root.exists());
    assert!(!directory.path().join("absent-models").exists());
}

#[test]
#[ignore = "requires installed MASH_TEST_MODELS_DIR"]
fn dense_content_switches_edits_and_forget_preserve_canonical_silver_and_lexical_search() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("provider");
    let database = directory.path().join("silver.sqlite3");
    history(&root, ["user", "assistant"]);
    let mut storage = Storage::open_writer(&database).unwrap();
    let source = ingest(&mut storage, &options(&root)).unwrap();
    let silver = storage.silver_certificate().unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let before = rows(&database);
    std::fs::remove_dir_all(&root).unwrap();
    let models = Models::new(PathBuf::from(
        std::env::var_os("MASH_TEST_MODELS_DIR").unwrap(),
    ));
    for selection in [
        DenseContent::AllVisible,
        DenseContent::UserMessages,
        DenseContent::AllVisible,
        DenseContent::UserMessages,
    ] {
        let recipe = Recipe {
            dense_content: selection,
            ..Default::default()
        };
        let mut storage = Storage::open_writer(&database).unwrap();
        let result = build(&mut storage, &models, &source.summary, &recipe).unwrap();
        assert!(result.ready);
        assert_eq!(
            result.counts.embeddings,
            if recipe.users_only() { 1 } else { 2 }
        );
        assert_eq!(storage.silver_certificate().unwrap(), silver);
        assert_eq!(storage.search("reply", 5, None, None).unwrap().len(), 2);
        assert!(!storage.has_pending_embeddings().unwrap());
        storage.commit_writer().unwrap();
        drop(storage);
        assert_eq!(rows(&database), before);
        let stored =
            Storage::open_search(&database, &recipe.generation(&models.generation())).unwrap();
        assert!(
            stored
                .semantic_index_is_ready(&recipe.generation(&models.generation()))
                .unwrap()
        );
        assert_eq!(stored.gold_manifest().unwrap().unwrap().build, recipe);
        assert!(!root.exists());
    }
    history(&root, ["assistant", "assistant"]);
    let mut storage = Storage::open_writer(&database).unwrap();
    let edited = ingest(&mut storage, &options(&root)).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let edited_rows = rows(&database);
    let mut storage = Storage::open_writer(&database).unwrap();
    let recipe = Recipe {
        dense_content: DenseContent::UserMessages,
        ..Default::default()
    };
    let result = build(
        &mut storage,
        &Models::new(directory.path().join("absent-models")),
        &edited.summary,
        &recipe,
    )
    .unwrap();
    assert!(result.ready);
    assert_eq!(result.embeddings.model_inferences, 0);
    assert_eq!(result.counts.embeddings, 0);
    assert!(storage.silver_certificate().unwrap().revision > silver.revision);
    let id = storage
        .chat_candidates(&crate::chat_catalog::Request::default(), "local")
        .unwrap()[0]
        .conversation_id
        .clone();
    storage.commit_writer().unwrap();
    drop(storage);
    assert_eq!(rows(&database), edited_rows);
    let mut storage = Storage::open_existing(&database).unwrap();
    assert!(storage.forget(&id).unwrap());
    assert_eq!(storage.counts().unwrap().messages, 0);
    assert_eq!(storage.counts().unwrap().embeddings, 0);
}
