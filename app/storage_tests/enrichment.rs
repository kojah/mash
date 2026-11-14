use crate::provider::Provider;
use crate::silver::{
    IndexOptions,
    enrichment::{Boundary, Origin, TimestampOrigin, VERSION},
};
use crate::storage::Storage;

#[test]
fn native_history_retains_traceable_title_and_project_evidence_across_restart() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("chats");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("session.jsonl"), concat!(
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"thread\",\"cwd\":\"/work/project\"}}\n",
        "{\"type\":\"response_item\",\"timestamp\":\"2026-10-01T00:00:00Z\",\"payload\":{\"type\":\"message\",\"id\":\"hidden\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"<environment_context>hidden</environment_context>\"}]}}\n",
        "{\"type\":\"response_item\",\"timestamp\":\"2026-10-02T00:00:00Z\",\"payload\":{\"type\":\"message\",\"id\":\"visible\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"qdrant filtering\"}]}}\n",
    )).unwrap();
    let path = directory.path().join("silver.sqlite3");
    let options = IndexOptions {
        providers: [(Provider::Codex, vec![source])].into_iter().collect(),
        roots_are_authoritative: true,
        since_days: None,
    };
    let mut storage = Storage::open_writer(&path).unwrap();
    crate::silver::index(&mut storage, &options).unwrap();
    let (id, message): (String, String) = storage
        .connection
        .query_row(
            "SELECT m.chat_id,m.id FROM messages m WHERE m.ordinal=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let evidence = storage.chat_enrichment(&id).unwrap().unwrap();
    assert_eq!(
        evidence.title,
        Some(Origin::FirstVisibleUserMessage {
            message_id: message.clone(),
            rule_version: crate::silver::enrichment::VERSION
        })
    );
    assert_eq!(
        evidence.project,
        Some(Origin::SourceMetadata {
            field: crate::silver::enrichment::SourceField::Cwd
        })
    );
    assert!(evidence.repository.is_none());
    let hidden: String = storage
        .connection
        .query_row(
            "SELECT id FROM messages WHERE chat_id=?1 AND ordinal=0",
            [&id],
            |row| row.get(0),
        )
        .unwrap();
    let times = evidence.timestamps.as_ref().unwrap();
    assert_eq!(
        times.created_at,
        Some(TimestampOrigin::MessageBoundary {
            message_id: hidden,
            selection: Boundary::Minimum,
            rule_version: VERSION,
        })
    );
    assert_eq!(
        times.updated_at,
        Some(TimestampOrigin::MessageBoundary {
            message_id: message,
            selection: Boundary::Maximum,
            rule_version: VERSION,
        })
    );
    assert_eq!(storage.counts().unwrap().messages, 2);
    assert_eq!(storage.counts().unwrap().embeddings, 0);
    storage.commit_writer().unwrap();
    drop(storage);
    let stored = Storage::open_silver(&path).unwrap();
    assert_eq!(stored.chat_enrichment(&id).unwrap(), Some(evidence.clone()));
    let threads = stored
        .chat_candidates(&crate::chat_catalog::Request::default(), "local")
        .unwrap();
    assert_eq!(threads[0].metadata.enrichment, Some(evidence.clone()));
    stored
        .with_publication_snapshot(|snapshot| {
            assert_eq!(snapshot.source_silver, stored.silver_certificate().unwrap());
            snapshot.visit(crate::shared::protocol::BatchKind::Chats, |batch| {
                let crate::shared::protocol::Batch::Chats { records } = batch else {
                    panic!("conversation batch")
                };
                assert_eq!(records[0].enrichment, Some(evidence.clone()));
                Ok(())
            })
        })
        .unwrap();
}
