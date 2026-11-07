use super::*;
use crate::shared::activity::Progress;
use crate::shared::silver_store::{SilverStore, read};
use std::io::Write;

fn identity() -> Identity {
    Identity {
        group_id: "retry-test".into(),
        node_id: "node".into(),
        generation: "fixture:test:4".to_owned(),
        dimensions: 4,
    }
}

fn file(content: Option<&str>, revision: i64) -> tempfile::NamedTempFile {
    let messages = content
        .into_iter()
        .map(|text| MessageRecord {
            attachments: vec![],
            id: "message".into(),
            chat_id: "chat".into(),
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
        Event::Batch(Batch::Chats {
            records: vec![ChatRecord {
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
    storage_ids: Vec<(String, i64, String)>,
}

fn retained(store: &SilverStore) -> Retained {
    // A fresh connection verifies persisted state after each interrupted writer.
    let connection = store.connect(&identity()).unwrap();
    let mut history = vec![];
    read::export(&connection, &mut history).unwrap();
    let storage_ids = connection
        .prepare("SELECT 'session',rowid,id FROM chats UNION ALL SELECT 'message',rowid,id FROM messages ORDER BY 1,2")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    Retained {
        history,
        head: encode(&head(&connection).unwrap()).unwrap(),
        storage_ids,
    }
}

fn with_recall(statement: &str, revision: i64) -> tempfile::NamedTempFile {
    use crate::records::{
        Body, Details, Evidence, FactState, Review, Scope, transfer::Kind, transfer::Row,
    };
    let input = file(Some("original"), revision);
    let lines = std::fs::read_to_string(input.path()).unwrap();
    let mut output = tempfile::NamedTempFile::new().unwrap();
    for line in lines.lines() {
        let event: Event = serde_json::from_str(line).unwrap();
        if matches!(event, Event::Finish) {
            let rows = vec![
                Row::Owner { id: "owner".into() },
                Row::Header {
                    id: "fact".into(),
                    owner: "owner".into(),
                    kind: Kind::Fact,
                    scope: Some(Scope::Personal),
                    revision: 1,
                    forgotten: false,
                },
                Row::Revision {
                    id: "fact".into(),
                    revision: 1,
                    recorded_at: 1,
                    body: Body {
                        statement: statement.into(),
                        rationale: String::new(),
                        recorded_by: "test".into(),
                        review: Review::Candidate,
                        effective_at: None,
                        details: Details::Fact {
                            state: FactState::Current,
                            reported_by: "user".into(),
                            valid_until: None,
                        },
                    },
                },
                Row::Evidence {
                    id: "fact".into(),
                    revision: 1,
                    position: 0,
                    source: Evidence {
                        chat_id: "chat".into(),
                        message_id: "message".into(),
                        quote: "original".into(),
                        fingerprint: blake3::hash(b"original").to_hex().to_string(),
                        purpose: None,
                        source_role: None,
                    },
                },
            ];
            serde_json::to_writer(&mut output, &Event::Batch(Batch::Records { records: rows }))
                .unwrap();
            writeln!(output).unwrap();
        }
        serde_json::to_writer(&mut output, &event).unwrap();
        writeln!(output).unwrap();
    }
    output
}

#[test]
fn shared_record_snapshot_retry_rejects_changed_record_text_and_rolls_back() {
    let directory = tempfile::tempdir().unwrap();
    let store = SilverStore::open(directory.path(), [&identity()]).unwrap();
    let build = Build::default();
    let progress = Progress::default();
    let input = with_recall("original claim", 1);
    write(&store, &identity(), &input, &build, &progress, None, false).unwrap();
    let before =
        crate::storage::records::transfer::fingerprint(&store.connect(&identity()).unwrap())
            .unwrap();
    let error = write(
        &store,
        &identity(),
        &with_recall("changed claim", 1),
        &build,
        &progress,
        None,
        false,
    )
    .unwrap_err();
    assert!(error.error.message.contains("different records"));
    assert_eq!(
        crate::storage::records::transfer::fingerprint(&store.connect(&identity()).unwrap())
            .unwrap(),
        before
    );
    assert!(
        write(&store, &identity(), &input, &build, &progress, None, false)
            .unwrap()
            .duplicate
    );
}

#[test]
fn retry_outer_rollback_preserves_committed_history_across_interruption() {
    let directory = tempfile::tempdir().unwrap();
    let store = SilverStore::open(directory.path(), [&identity()]).unwrap();
    let input = file(Some("original"), 1);
    let build = Build::default();
    let progress = Progress::default();
    write(&store, &identity(), &input, &build, &progress, None, false).unwrap();
    let before = retained(&store);
    let error = write_inner(
        &store,
        &identity(),
        &input,
        &build,
        &progress,
        None,
        false,
        || Err(AppError::internal("interrupted after rollback")),
    )
    .unwrap_err();
    assert_eq!(error.error.message, "interrupted after rollback");
    assert_eq!(retained(&store), before);
    for content in [Some("changed"), None] {
        assert!(
            write(
                &store,
                &identity(),
                &file(content, 1),
                &build,
                &progress,
                None,
                false
            )
            .is_err()
        );
        assert_eq!(retained(&store), before);
    }
    let applied = write(&store, &identity(), &input, &build, &progress, None, false).unwrap();
    assert!(applied.duplicate);
    assert!(!applied.changed);
    let after = retained(&store);
    assert_eq!(after.history, before.history);
    assert_eq!(after.storage_ids, before.storage_ids);
    // A subsequent revision can still write after the interrupted retry.
    let next = write(
        &store,
        &identity(),
        &file(Some("new revision"), 2),
        &build,
        &progress,
        None,
        false,
    )
    .unwrap();
    assert!(!next.duplicate);
    assert_eq!(next.revision, 2);
    assert_ne!(retained(&store).history, before.history);
}
