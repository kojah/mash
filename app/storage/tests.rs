use super::*;
use std::path::PathBuf;

use crate::ingestion::Conversation;

fn conversation(content: &str) -> Conversation {
    Conversation {
        id: "session-1".to_owned(),
        provider: crate::provider::Provider::Codex,
        source: crate::ingestion::SourceIdentity::file(PathBuf::from("/tmp/session-1.jsonl")),
        project: None,
        repository: None,
        title: Some("session".to_owned()),
        created_at: Some(1),
        updated_at: Some(2),
        messages: vec![crate::ingestion::NormalizedMessage {
            id: "message-1".to_owned(),
            ordinal: 0,
            context: crate::message_context::MessageContext::Linear,
            role: crate::role::Role::User,
            content: content.to_owned(),
            search_projection: None,
            created_at: Some(i64::MAX / 2),
        }],
    }
}

fn conversation_with_messages(messages: &[(&str, &str)]) -> Conversation {
    let mut value = conversation("");
    value.messages = messages
        .iter()
        .enumerate()
        .map(
            |(ordinal, (id, content))| crate::ingestion::NormalizedMessage {
                id: (*id).to_owned(),
                ordinal: i64::try_from(ordinal).expect("test ordinal"),
                context: crate::message_context::MessageContext::Linear,
                role: crate::role::Role::User,
                content: (*content).to_owned(),
                search_projection: None,
                created_at: Some(i64::MAX / 2),
            },
        )
        .collect();
    value.created_at = Some(i64::MAX / 2);
    value.updated_at = Some(i64::MAX / 2);
    value
}

mod indexing;
mod schema;
mod semantic;
