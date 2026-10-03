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
        .thread_candidates(&crate::thread_catalog::Request::default(), "local")
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
