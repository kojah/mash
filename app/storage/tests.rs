use super::*;
use std::path::PathBuf;

use crate::silver::Conversation;

fn conversation(content: &str) -> Conversation {
    Conversation {
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

pub(super) fn conversation_with_messages(messages: &[(&str, &str)]) -> Conversation {
    let mut value = conversation("");
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
mod connectors;
mod indexing;
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
