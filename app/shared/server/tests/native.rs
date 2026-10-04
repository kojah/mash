//! Publish parsed native conversations through the same typed shared protocol.
use super::*;
use crate::shared::protocol::{Batch, ConversationRecord, Event, MessageRecord, Start};

pub(super) fn publication(conversation: crate::silver::Conversation) -> (Vec<Value>, Vec<String>) {
    let ids = conversation
        .messages
        .inline()
        .iter()
        .map(|m| m.id.clone())
        .collect();
    let record = ConversationRecord {
        enrichment: None,
        id: conversation.id.clone(),
        provider: conversation.provider,
        title: conversation.title,
        project: conversation.project,
        repository: conversation.repository,
        created_at: conversation.created_at,
        updated_at: conversation.updated_at,
    };
    let mut vector = vec![0.0; 384];
    vector[0] = 127.0;
    let messages = conversation
        .messages
        .inline()
        .to_vec()
        .into_iter()
        .map(|m| {
            let projection = m.dialogue_text.unwrap();
            let visible = !projection.is_empty();
            MessageRecord {
                attachments: Vec::new(),
                id: m.id,
                conversation_id: conversation.id.clone(),
                ordinal: m.ordinal,
                context: m.context,
                role: m.role,
                content: m.content,
                projection,
                created_at: m.created_at,
                generation: visible.then(|| crate::semantic::SHARED_EMBEDDING_GENERATION.into()),
                vector: visible.then(|| vector.clone()),
            }
        })
        .collect();
    let events = [
        Event::Start(Start {
            reuse_gold: false,
            expected_version: None,
            gold_build: None,
            source_silver: None,
            instance: format!("native-{}", conversation.provider),
            revision: Some(1),
            base_revision: None,
            snapshot: true,
            forgets: Vec::new(),
        }),
        Event::Batch(Batch::Conversations {
            records: vec![record],
        }),
        Event::Batch(Batch::Messages { records: messages }),
        Event::Finish,
    ]
    .into_iter()
    .map(|event| serde_json::to_value(event).unwrap())
    .collect();
    (events, ids)
}
