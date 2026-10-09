use super::*;
use crate::silver::{Conversation, NormalizedMessage, SourceIdentity};

#[test]
fn incremental_capture_includes_only_changed_complete_threads_and_explicit_removals() {
    let directory = tempfile::tempdir().unwrap();
    let database = seed(directory.path());
    let storage = Storage::open_existing(&database).unwrap();
    storage.prepare_publication_target("journal").unwrap();
    let mut instance = String::new();
    let base = storage
        .with_publication_snapshot(|snapshot| {
            instance.clone_from(&snapshot.instance);
            Ok(())
        })
        .unwrap();
    storage.acknowledge_publication("journal", base).unwrap();
    crate::storage::test_support::write(
        &database,
        "UPDATE objects SET title='changed' WHERE origin='provider' AND id='a'; DELETE FROM objects WHERE origin='provider' AND id='b';",
    );
    let target = Target {
        build: crate::gold::config::Build::default(),
        generation: "unused".into(),
        dimensions: 384,
        instance: Some(instance),
        publication_revision: base,
    };
    let source = capture(&database, &target, true).unwrap();
    assert_eq!(source.conversations.keys().collect::<Vec<_>>(), vec!["a"]);
    let events: Vec<Event> = std::fs::read_to_string(source.file.path())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let Event::Start(start) = &events[0] else {
        panic!("expected start")
    };
    assert!(!start.snapshot);
    assert_eq!(start.base_revision, Some(base));
    assert!(
        events
            .iter()
            .any(|event| matches!(event,Event::Remove{id} if id=="b"))
    );
    let mut messages = 0;
    for event in events {
        if let Event::Batch(Batch::Messages { records }) = event {
            assert!(records.iter().all(|record| record.conversation_id == "a"));
            messages += records.len();
        }
    }
    assert_eq!(messages, 37);
}

#[test]
fn snapshots_upload_redacted_records_and_keep_canonical_silver() {
    let root = tempfile::tempdir().unwrap();
    let database = seed(root.path());
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection.execute("UPDATE objects SET content='password=\"hunter2\"',search_projection='password=\"hunter2\"' WHERE origin='provider' AND id='a-100'",[]).unwrap();
    connection
        .execute(
            "UPDATE objects SET title='password=\"rosebud\"' WHERE origin='provider' AND id='a'",
            [],
        )
        .unwrap();
    let target = Target {
        build: crate::gold::config::Build::default(),
        generation: "unused".into(),
        dimensions: 384,
        instance: None,
        publication_revision: 0,
    };
    let source = capture(&database, &target, false).unwrap();
    let stream = std::fs::read_to_string(source.file.path()).unwrap();
    assert!(!stream.contains("hunter2") && !stream.contains("rosebud"));
    let canonical: String = connection
        .query_row("SELECT content FROM objects WHERE origin='provider' AND object_role='entry' AND id='a-100'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(canonical.contains("hunter2"));
}

fn seed(directory: &Path) -> std::path::PathBuf {
    let database = directory.join("silver.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    for (thread, length) in [("a", 37), ("b", 2)] {
        storage
            .replace_conversation(&Conversation {
                id: thread.into(),
                provider: crate::provider::Provider::Codex,
                title_origin: None,
                timestamp_origins: None,
                claude_lineage: None,
                source: SourceIdentity::file(format!("/nonexistent/mash-{thread}.jsonl")),
                project: None,
                repository: None,
                title: Some(format!("Topic {thread}")),
                created_at: Some(1),
                updated_at: Some(2),
                messages: (0..length)
                    .map(|ordinal| NormalizedMessage {
                        attachments: Vec::new(),
                        id: format!("{thread}-{}", 100 - ordinal),
                        ordinal,
                        context: crate::message_context::MessageContext::Linear,
                        role: crate::role::Role::User,
                        content: format!("canonical {thread} message {ordinal}"),
                        dialogue_text: (thread == "a" && ordinal % 4 == 0).then(String::new),
                        created_at: Some(ordinal),
                    })
                    .collect::<Vec<_>>()
                    .into(),
            })
            .unwrap();
    }
    storage.commit_writer().unwrap();
    database
}

#[tokio::test]
async fn parent_units_resolve_from_consistent_sqlite_snapshot_across_batches_and_threads() {
    let directory = tempfile::tempdir().unwrap();
    let database = seed(directory.path());
    let silver = Storage::open_catalog(&database)
        .unwrap()
        .silver_certificate()
        .unwrap();
    let target = Target {
        build: crate::gold::config::Build {
            retrieval_unit: crate::gold::config::RetrievalUnit::ParentAndMessage,
            ..Default::default()
        },
        generation: "unused-model".into(),
        dimensions: 4,
        instance: None,
        publication_revision: 0,
    };
    let source = capture(&database, &target, false).unwrap();
    let mut reader = Reader::new(source.file.reopen().unwrap()).unwrap();
    let mut inputs = BTreeMap::new();
    let mut sizes = Vec::new();
    loop {
        let (next, batch) = reader.next().unwrap();
        reader = next;
        if batch.is_empty() {
            break;
        }
        sizes.push(batch.len());
        let (next, batch, prepared) = prepare_units(reader, batch, &target.build, None)
            .await
            .unwrap();
        reader = next;
        for (row, text) in batch
            .iter()
            .filter(|m| !m.projection.is_empty())
            .zip(prepared)
        {
            inputs.insert(row.id.clone(), text);
        }
    }
    assert_eq!(sizes, [32, 5, 2]);
    assert_eq!(inputs["a-67"], "canonical a message 33");
    assert_eq!(
        inputs["a-66"],
        "canonical a message 34\n\nParent message:\ncanonical a message 33"
    );
    assert_eq!(inputs["b-100"], "canonical b message 0");
    assert_eq!(
        inputs["b-99"],
        "canonical b message 1\n\nParent message:\ncanonical b message 0"
    );
    assert_eq!(inputs.len(), 29);
    assert_eq!(
        Storage::open_catalog(&database)
            .unwrap()
            .silver_certificate()
            .unwrap(),
        silver
    );
}
