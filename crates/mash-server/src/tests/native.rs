//! Publish parsed native chats through the same typed shared protocol.
use super::*;
use crate::shared::protocol::{Batch, ChatRecord, Event, MessageRecord, Start};

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_large_native_tool_context_survives_publication_and_view() {
    let service = Service::qdrant(384).await;
    let content = "largepatchcanary\n".repeat(600_000);
    let mut events = snapshot(1);
    events[2]["records"][2]["content"] = json!(content);
    let response = service.upload(FIRST, &events).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{}",
        response.text().await.unwrap()
    );
    let messages: Vec<crate::storage::Message> = service
        .request("view", FIRST)
        .json(&json!({"node_id":"first","id":"context","context":0}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(messages.len(), 1);
    // Public reads keep the existing fail-closed masking limit. Canonical
    // shared silver must still preserve an explicitly unmasked upload.
    assert_eq!(messages[0].content, "[REDACTED_SECRET]");
    let canonical = service
        .store
        .view(
            &service.first,
            protocol::View {
                node_id: "first".into(),
                id: "context".into(),
                context: 0,
                content: crate::storage::MessageContent::Canonical,
            },
        )
        .await
        .unwrap();
    assert_eq!(canonical.len(), 1);
    assert_eq!(
        blake3::hash(canonical[0].content.as_bytes()),
        blake3::hash(content.as_bytes())
    );
    assert_eq!(service.state().await.revision, 1);
    assert_eq!(service.counts(FIRST).await.messages, 3);
    let freshness = service.get("freshness", FIRST).await;
    let node = freshness["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["node_id"] == "first")
        .unwrap();
    assert_eq!(node["views_current"], true);
    service.cleanup().await;
}

pub(super) fn publication(chat: crate::silver::Chat) -> (Vec<Value>, Vec<String>) {
    let ids = chat
        .messages
        .inline()
        .iter()
        .map(|m| m.id.clone())
        .collect();
    let record = ChatRecord {
        enrichment: None,
        id: chat.id.clone(),
        provider: chat.provider,
        title: chat.title,
        project: chat.project,
        repository: chat.repository,
        created_at: chat.created_at,
        updated_at: chat.updated_at,
    };
    let mut vector = vec![0.0; 384];
    vector[0] = 127.0;
    let messages = chat
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
                chat_id: chat.id.clone(),
                ordinal: m.ordinal,
                context: m.context,
                role: m.role,
                content: m.content,
                projection,
                created_at: m.created_at,
                generation: visible.then(|| crate::semantic::SHARED_EMBEDDING_GENERATION.into()),
                vector: visible.then(|| vector.clone()),
                tools: crate::tools::Activity::default(),
            }
        })
        .collect();
    let events = [
        Event::Start(Start {
            views_build: None,
            source_silver: None,
            instance: format!("native-{}", chat.provider),
            revision: Some(1),
            base_revision: None,
            snapshot: true,
            forgets: Vec::new(),
        }),
        Event::Batch(Batch::Chats {
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
