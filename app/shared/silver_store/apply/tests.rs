use super::*;
use crate::shared::activity::Progress;
use crate::shared::silver_store::{SilverStore, read};
use std::io::Write;

fn identity() -> Identity {
    Identity {
        group_id: "retry-test".into(),
        node_id: "node".into(),
        generation: crate::semantic::model_space("test", 4),
        dimensions: 4,
    }
}

fn file(content: Option<&str>, revision: i64) -> tempfile::NamedTempFile {
    let messages = content
        .into_iter()
        .map(|text| MessageRecord {
            attachments: vec![],
            id: "message".into(),
            conversation_id: "chat".into(),
            ordinal: 0,
            context: crate::message_context::MessageContext::Linear,
            role: crate::role::Role::User,
            content: text.into(),
            projection: text.into(),
            created_at: None,
            generation: Some(identity().generation),
            vector: Some(vec![127.0, 0.0, 0.0, 0.0]),
            tools: crate::tools::Activity::default(),
        })
        .collect();
    let events = [
        Event::Start(Start {
            views_build: None,
            source_silver: None,
            instance: "instance".into(),
            revision: Some(revision),
            base_revision: None,
            snapshot: true,
            forgets: vec![],
        }),
        Event::Batch(Batch::Conversations {
            records: vec![ConversationRecord {
                enrichment: None,
                id: "chat".into(),
                provider: Provider::Codex,
                title: None,
                project: None,
                repository: None,
                created_at: None,
                updated_at: None,
            }],
        }),
        Event::Batch(Batch::Messages { records: messages }),
        Event::Finish,
    ];
    let mut file = tempfile::NamedTempFile::new().unwrap();
    for event in events {
        serde_json::to_writer(&mut file, &event).unwrap();
        writeln!(file).unwrap();
    }
    file
}

#[derive(Debug, PartialEq, Eq)]
struct Retained {
    history: Vec<u8>,
    head: String,
    storage_ids: Vec<i64>,
    sequence: i64,
}

fn retained(store: &SilverStore) -> Retained {
    // A fresh connection verifies persisted state after each interrupted writer.
    let connection = store.connect(&identity()).unwrap();
    let mut history = vec![];
    read::export(&connection, &mut history).unwrap();
    let storage_ids = connection
        .prepare("SELECT storage_id FROM objects ORDER BY storage_id")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    Retained {
        history,
        head: encode(&head(&connection).unwrap()).unwrap(),
        storage_ids,
        sequence: connection
            .query_row(
                "SELECT seq FROM sqlite_sequence WHERE name='objects'",
                [],
                |row| row.get(0),
            )
            .unwrap(),
    }
}

#[test]
fn retry_outer_rollback_preserves_committed_history_across_interruption() {
    let directory = tempfile::tempdir().unwrap();
    let store = SilverStore::open(directory.path(), [&identity()]).unwrap();
    let input = file(Some("original"), 1);
    let build = Build::default();
    let progress = Progress::default();
    write(&store, &identity(), &input, &build, &progress).unwrap();
    let before = retained(&store);
    let error = write_inner(&store, &identity(), &input, &build, &progress, || {
        Err(AppError::internal("interrupted after rollback"))
    })
    .unwrap_err();
    assert_eq!(error.error.message, "interrupted after rollback");
    assert_eq!(retained(&store), before);
    for content in [Some("changed"), None] {
        assert!(write(&store, &identity(), &file(content, 1), &build, &progress).is_err());
        assert_eq!(retained(&store), before);
    }
    let applied = write(&store, &identity(), &input, &build, &progress).unwrap();
    assert!(applied.duplicate);
    assert!(!applied.changed);
    let after = retained(&store);
    assert_eq!(after.history, before.history);
    assert_eq!(after.storage_ids, before.storage_ids);
    assert_eq!(after.sequence, before.sequence);
    // A subsequent revision can still write after the interrupted retry.
    let next = write(
        &store,
        &identity(),
        &file(Some("new revision"), 2),
        &build,
        &progress,
    )
    .unwrap();
    assert!(!next.duplicate);
    assert_eq!(next.revision, 2);
    assert_ne!(retained(&store).history, before.history);
}
