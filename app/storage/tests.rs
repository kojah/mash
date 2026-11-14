use super::*;
use std::path::PathBuf;

use crate::silver::Chat;

fn chat(content: &str) -> Chat {
    Chat {
        id: "session-1".to_owned(),
        provider: crate::provider::Provider::Codex,
        title_origin: None,
        timestamp_origins: None,
        claude_lineage: None,
        source: crate::silver::SourceIdentity::file(PathBuf::from("/tmp/session-1.jsonl")),
        project: None,
        repository: None,
        title: Some("session".to_owned()),
        created_at: Some(1),
        updated_at: Some(2),
        messages: vec![crate::silver::NormalizedMessage {
            attachments: Vec::new(),
            id: "message-1".to_owned(),
            ordinal: 0,
            context: crate::message_context::MessageContext::Linear,
            role: crate::role::Role::User,
            content: content.to_owned(),
            dialogue_text: None,
            created_at: Some(i64::MAX / 2),
            tools: crate::tools::Activity::default(),
        }]
        .into(),
    }
}

pub(super) fn chat_with_messages(messages: &[(&str, &str)]) -> Chat {
    let mut value = chat("");
    value.messages = messages
        .iter()
        .enumerate()
        .map(
            |(ordinal, (id, content))| crate::silver::NormalizedMessage {
                attachments: Vec::new(),
                id: (*id).to_owned(),
                ordinal: i64::try_from(ordinal).expect("test ordinal"),
                context: crate::message_context::MessageContext::Linear,
                role: crate::role::Role::User,
                content: (*content).to_owned(),
                dialogue_text: None,
                created_at: Some(i64::MAX / 2),
                tools: crate::tools::Activity::default(),
            },
        )
        .collect();
    value.created_at = Some(i64::MAX / 2);
    value.updated_at = Some(i64::MAX / 2);
    value
}

mod chats;
mod indexing;
mod properties;
mod publication;
mod schema;
mod staging;
mod views_database;

#[test]
fn writers_commit_without_an_fsync_per_transaction() {
    let directory = tempfile::tempdir().unwrap();
    let storage = Storage::open(&directory.path().join("index.sqlite3")).unwrap();
    let mode: String = storage
        .connection
        .query_row("PRAGMA main.journal_mode", [], |row| row.get(0))
        .unwrap();
    let sync: i64 = storage
        .connection
        .query_row("PRAGMA main.synchronous", [], |row| row.get(0))
        .unwrap();
    assert_eq!((mode.as_str(), sync), ("wal", 1));
}

#[test]
fn policy_upgrade_is_pending_and_retries_keep_the_same_revision() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("index.sqlite3");
    let mut storage = crate::storage::Storage::open_writer(&path).unwrap();
    storage.commit_writer().unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE shared_publication SET revision=1 WHERE singleton=1",
            [],
        )
        .unwrap();
    storage.prepare_publication_target("first").unwrap();
    let revision = storage.with_publication_snapshot(|_| Ok(())).unwrap();
    storage
        .acknowledge_publication(
            "first",
            revision,
            crate::storage::publication::JournalCoverage::Incremental { through: revision },
        )
        .unwrap();
    assert!(!crate::storage::Storage::publication_is_pending(&path, "first").unwrap());
    storage.prepare_publication_target("redaction-v1").unwrap();
    let next = storage.with_publication_snapshot(|_| Ok(())).unwrap();
    assert_eq!(next, revision + 1);
    assert!(crate::storage::Storage::publication_is_pending(&path, "redaction-v1").unwrap());
    storage.prepare_publication_target("redaction-v1").unwrap();
    assert_eq!(storage.with_publication_snapshot(|_| Ok(())).unwrap(), next);
}
