//! Canonical SQLite access for fixtures and diagnostics.
use super::{Connection, Message, MessageContent, Path, Storage};
use crate::silver::Chat;
use std::path::PathBuf;

/// Inspect retained silver projections for ingestion assertions. This is a
/// case-insensitive substring check, with no retrieval, ranking or gold access.
pub fn projections_with_text(
    storage: &Storage,
    text: &str,
    provider: Option<crate::provider::Provider>,
) -> Vec<Message> {
    let mut statement = storage
        .connection
        .prepare(
            "SELECT m.id FROM messages m JOIN chats c ON c.id=m.chat_id
         WHERE (?2 IS NULL OR c.provider=?2)
         AND instr(lower(COALESCE(m.search_projection,m.content)),lower(?1))>0
         ORDER BY m.id",
        )
        .unwrap();
    let ids = statement
        .query_map(rusqlite::params![text, provider], |row| {
            row.get::<_, String>(0)
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    ids.into_iter()
        .map(|id| {
            storage
                .view_content(&id, 0, MessageContent::SearchProjection)
                .unwrap()
                .remove(0)
        })
        .collect()
}

pub fn connect(path: &Path) -> Connection {
    let connection = Connection::open(path).expect("open local silver");
    crate::storage::packed::register(&connection).expect("register silver codec");
    connection
}

/// Apply raw canonical SQL with the node publication journal installed.
pub fn write(path: &Path, sql: &str) {
    Storage::open_existing(path)
        .expect("local index")
        .connection
        .execute_batch(sql)
        .expect("raw write");
}

/// One Codex chat holding a single user message.
pub fn chat(content: &str) -> Chat {
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

/// The fixture chat with these `(id, content)` user messages instead.
pub fn chat_with_messages(messages: &[(&str, &str)]) -> Chat {
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
