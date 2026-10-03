use super::*;
use crate::{provider::Provider, role::Role};

const WRAPPED: &str = "<mode_notice>injectednoise</mode_notice><user_input mode=\"act\">Repair Cline migration</user_input>";

fn fixture(path: &Path) {
    let mut storage = Storage::open_writer(path).unwrap();
    let mut cline = conversation_with_messages(&[
        ("wrapped", WRAPPED),
        ("synthetic", "[TASK RESUMPTION] operational prompt"),
        ("plain", "Already clean Cline subject"),
        ("excluded", "<user_input>Do not expose</user_input>"),
    ]);
    cline.id = "cline-session".into();
    cline.provider = Provider::Cline;
    let mut messages = cline.messages.inline().to_vec();
    messages[3].search_projection = Some(String::new());
    cline.messages = messages.into();
    storage.replace_conversation(&cline).unwrap();
    storage
        .replace_conversation(&conversation("unrelated Codex subject"))
        .unwrap();
    storage.rebuild_derived_search_state().unwrap();
    for id in ["wrapped", "synthetic", "plain", "message-1"] {
        storage
            .replace_embeddings(
                "fixture",
                &[EmbeddingWrite {
                    message_id: id,
                    vector: &[127, 0],
                    norm: 127.0,
                }],
            )
            .unwrap();
    }
    storage.mark_semantic_index_ready("fixture").unwrap();
    let source = crate::silver::SourceIdentity::file("/fixture/source.jsonl");
    let stamp = crate::silver::SourceStamp::file(10, 20);
    for provider in [Provider::Cline, Provider::Codex] {
        storage
            .record_source_checkpoint(provider, &source, &stamp)
            .unwrap();
    }
    storage.commit_writer().unwrap();
}

#[test]
fn projection_upgrade_preserves_canonical_rows_and_only_invalidates_changed_vectors() {
    for version in [32, 33, 34] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("index.sqlite3");
        fixture(&path);
        Connection::open(&path)
            .unwrap()
            .pragma_update(None, "user_version", version)
            .unwrap();
        assert!(
            Storage::open_search(&path, "fixture").is_err(),
            "search never migrates"
        );
        let mut storage = Storage::open_writer(&path).unwrap();
        assert!(!storage.semantic_index_is_ready("fixture").unwrap());
        assert!(!storage.derived_search_is_dirty().unwrap());
        assert_eq!(storage.embedding_count("fixture").unwrap(), 2);
        assert!(storage.has_pending_embeddings().unwrap());
        assert_eq!(storage.view("wrapped", 0).unwrap()[0].content, WRAPPED);
        assert_eq!(
            storage
                .view_content("wrapped", 0, MessageContent::SearchProjection)
                .unwrap()[0]
                .content,
            "Repair Cline migration"
        );
        assert!(
            storage
                .search("injectednoise", 5, None, None)
                .unwrap()
                .is_empty()
        );
        assert!(
            storage
                .search("operational", 5, None, None)
                .unwrap()
                .is_empty()
        );
        assert!(storage.search("expose", 5, None, None).unwrap().is_empty());
        for id in ["plain", "message-1"] {
            crate::storage::test_support::assert_quantized_embedding(
                &storage,
                id,
                &[127, 0],
                127.0,
            );
        }
        let source = crate::silver::SourceIdentity::file("/fixture/source.jsonl");
        let stamp = crate::silver::SourceStamp::file(10, 20);
        assert!(
            !storage
                .source_checkpoint_matches(Provider::Cline, &source, &stamp)
                .unwrap()
        );
        assert!(
            storage
                .source_checkpoint_matches(Provider::Codex, &source, &stamp)
                .unwrap()
        );
        storage
            .replace_embeddings(
                "fixture",
                &[EmbeddingWrite {
                    message_id: "wrapped",
                    vector: &[0, 127],
                    norm: 127.0,
                }],
            )
            .unwrap();
        storage.mark_semantic_index_ready("fixture").unwrap();
        storage.commit_writer().unwrap();
        drop(storage);
        let storage = Storage::open_search(&path, "fixture").unwrap();
        assert_eq!(storage.embedding_count("fixture").unwrap(), 3);
        assert!(!storage.has_pending_embeddings().unwrap());
        assert_eq!(storage.view("wrapped", 0).unwrap()[0].role, Role::User);
        assert_eq!(storage.view("wrapped", 0).unwrap()[0].content, WRAPPED);
    }
}

#[test]
fn older_schema_also_repairs_cline_projection_before_explicit_reindex() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    fixture(&path);
    Connection::open(&path)
        .unwrap()
        .pragma_update(None, "user_version", 31)
        .unwrap();
    let storage = Storage::open_writer(&path).unwrap();
    assert!(storage.derived_search_is_dirty().unwrap());
    assert!(!storage.semantic_index_is_ready("fixture").unwrap());
    assert_eq!(storage.view("wrapped", 0).unwrap()[0].content, WRAPPED);
    assert_eq!(
        storage
            .view_content("wrapped", 0, MessageContent::SearchProjection)
            .unwrap()[0]
            .content,
        "Repair Cline migration"
    );
    assert_eq!(storage.embedding_count("fixture").unwrap(), 2);
}

#[test]
fn failed_projection_upgrade_rolls_back_for_retry() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    fixture(&path);
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch("PRAGMA user_version=34; CREATE TRIGGER reject_cline_projection BEFORE UPDATE ON messages BEGIN SELECT RAISE(ABORT,'test refusal'); END;").unwrap();
    drop(connection);
    assert!(Storage::open_writer(&path).is_err());
    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        34
    );
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM message_embeddings", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        4
    );
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM source_checkpoints", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COALESCE(search_projection,content) FROM messages WHERE id='wrapped'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        WRAPPED
    );
    connection
        .execute_batch("DROP TRIGGER reject_cline_projection")
        .unwrap();
    drop(connection);
    assert!(Storage::open_writer(&path).is_ok());
}
