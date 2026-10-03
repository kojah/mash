use super::*;
use crate::silver::{IndexOptions, SourceIdentity, SourceStamp, enrichment::Origin};

#[test]
fn previous_schema_migration_preserves_history_and_explicit_replay_restores_evidence() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("silver.sqlite3");
    let thread = crate::storage::tests::conversation_with_messages(&[("message", "session")]);
    let stamp = SourceStamp::file(10, 20);
    let mut storage = Storage::open_writer(&path).unwrap();
    storage.replace_conversation(&thread).unwrap();
    storage
        .record_source_checkpoint(thread.provider, &thread.source, &stamp)
        .unwrap();
    let before = storage.silver_certificate().unwrap();
    let history = storage.view("message", 5).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch("DROP TABLE conversation_enrichment; PRAGMA user_version=37;")
        .unwrap();
    drop(connection);
    let mut storage = Storage::open_writer(&path).unwrap();
    assert_eq!(storage.silver_certificate().unwrap(), before);
    assert_eq!(
        serde_json::to_vec(&storage.view("message", 5).unwrap()).unwrap(),
        serde_json::to_vec(&history).unwrap()
    );
    assert!(
        storage
            .conversation_enrichment(&thread.id)
            .unwrap()
            .is_none()
    );
    assert!(
        !storage
            .source_checkpoint_matches(thread.provider, &thread.source, &stamp)
            .unwrap()
    );
    // Identical history can still require evidence repair after migration.
    assert!(storage.replace_conversation(&thread).unwrap().unchanged);
    assert!(
        storage
            .source_checkpoint_matches(thread.provider, &thread.source, &stamp)
            .unwrap()
    );
    assert!(
        storage
            .conversation_enrichment(&thread.id)
            .unwrap()
            .unwrap()
            .is_current()
    );
    assert!(storage.silver_certificate().unwrap().revision > before.revision);
}

#[test]
fn native_history_retains_traceable_title_and_project_evidence_across_restart() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("sessions");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("session.jsonl"), concat!(
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"thread\",\"cwd\":\"/work/project\"}}\n",
        "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"id\":\"hidden\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"<environment_context>hidden</environment_context>\"}]}}\n",
        "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"id\":\"visible\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"qdrant filtering\"}]}}\n",
    )).unwrap();
    let path = directory.path().join("silver.sqlite3");
    let options = IndexOptions {
        providers: [(Provider::Codex, vec![source])].into_iter().collect(),
        roots_are_authoritative: true,
        since_days: None,
    };
    let mut storage = Storage::open_writer(&path).unwrap();
    crate::silver::index(&mut storage, &options).unwrap();
    let (id, message): (String, String) = storage.connection.query_row("SELECT c.id,m.id FROM conversations c JOIN messages m ON m.conversation_id=c.id WHERE m.ordinal=1", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    let evidence = storage.conversation_enrichment(&id).unwrap().unwrap();
    assert_eq!(
        evidence.title,
        Some(Origin::FirstVisibleUserMessage {
            message_id: message,
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
    assert_eq!(storage.counts().unwrap().messages, 2);
    assert_eq!(storage.counts().unwrap().embeddings, 0);
    storage.commit_writer().unwrap();
    drop(storage);
    let stored = Storage::open_catalog(&path).unwrap();
    assert_eq!(
        stored.conversation_enrichment(&id).unwrap(),
        Some(evidence.clone())
    );
    let threads = stored
        .thread_candidates(&crate::thread_catalog::Request::default(), "local")
        .unwrap();
    assert_eq!(threads[0].metadata.enrichment, Some(evidence.clone()));
    stored
        .with_publication_snapshot(|snapshot| {
            assert_eq!(snapshot.source_silver, stored.silver_certificate().unwrap());
            snapshot.visit(crate::shared::protocol::BatchKind::Conversations, |batch| {
                let crate::shared::protocol::Batch::Conversations { records } = batch else {
                    panic!("conversation batch")
                };
                assert_eq!(records[0].enrichment, Some(evidence.clone()));
                Ok(())
            })
        })
        .unwrap();
}

#[test]
fn missing_outdated_and_malformed_evidence_cannot_skip_replay_and_deletes_cascade() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("silver.sqlite3");
    let mut storage = Storage::open_writer(&path).unwrap();
    let thread = crate::storage::tests::conversation_with_messages(&[("message", "session")]);
    storage.replace_conversation(&thread).unwrap();
    let stamp = SourceStamp::file(10, 20);
    storage
        .record_source_checkpoint(thread.provider, &thread.source, &stamp)
        .unwrap();
    assert!(
        storage
            .source_checkpoint_matches(thread.provider, &thread.source, &stamp)
            .unwrap()
    );
    let evidence = storage
        .conversation_enrichment(&thread.id)
        .unwrap()
        .unwrap();
    assert!(evidence.is_current());
    let before = storage.silver_certificate().unwrap().revision;
    storage
        .connection
        .execute("DELETE FROM conversation_enrichment", [])
        .unwrap();
    assert!(storage.silver_certificate().unwrap().revision > before);
    assert!(
        !storage
            .source_checkpoint_matches(thread.provider, &thread.source, &stamp)
            .unwrap()
    );
    assert!(storage.replace_conversation(&thread).unwrap().unchanged);
    assert_eq!(
        storage.conversation_enrichment(&thread.id).unwrap(),
        Some(evidence.clone())
    );
    let mut outdated = evidence;
    outdated.version += 1;
    storage.store_enrichment(&thread.id, &outdated).unwrap();
    assert!(
        !storage
            .source_checkpoint_matches(thread.provider, &thread.source, &stamp)
            .unwrap()
    );
    storage
        .connection
        .execute("UPDATE conversation_enrichment SET evidence='{}'", [])
        .unwrap();
    assert_eq!(
        storage
            .source_checkpoint_matches(thread.provider, &thread.source, &stamp)
            .unwrap_err()
            .error
            .kind,
        crate::ErrorKind::Database
    );
    storage.replace_conversation(&thread).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let mut storage = Storage::open_existing(&path).unwrap();
    assert!(storage.forget(&thread.id).unwrap());
    assert!(
        storage
            .conversation_enrichment(&thread.id)
            .unwrap()
            .is_none()
    );
    assert!(
        storage
            .source_enrichment_is_current(thread.provider, &SourceIdentity::file("empty").key())
            .unwrap()
    );
}
