//! The same branch contract must hold through every shared HTTP backend.
use super::*;

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn qdrant_context_preserves_branches_and_rejects_broken_publications() {
    contract(Service::qdrant(384).await).await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn qdrant_context_projects_multiple_topology_pages_and_fetches_selected_batches() {
    let service = Service::qdrant(384).await;
    let mut events = snapshot(1);
    let base: MessageRecord = serde_json::from_value(events[2]["records"][0].clone()).unwrap();
    let records: Vec<_> = (0..256)
        .map(|ordinal| MessageRecord {
            id: format!("message-{ordinal}"),
            ordinal,
            context: MessageContext::Linear,
            content: format!("canonical {ordinal}: {}", "payload ".repeat(128)),
            projection: format!("projection {ordinal}"),
            ..base.clone()
        })
        .collect();
    events.splice(
        2..3,
        records.chunks(128).map(|batch| {
            serde_json::to_value(Event::Batch(Batch::Messages {
                records: batch.to_vec(),
            }))
            .unwrap()
        }),
    );
    let response = service.upload(FIRST, &events).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{}",
        response.text().await.unwrap()
    );
    for representation in [MessageContent::Canonical, MessageContent::SearchProjection] {
        let messages = view(&service, 125, 100, representation).await;
        assert_eq!(messages.len(), 201);
        for (message, ordinal) in messages.iter().zip(25..=225) {
            assert_eq!(message.ordinal, ordinal);
            assert_eq!(message.id, format!("message-{ordinal}"));
            let expected = match representation {
                MessageContent::Canonical => {
                    format!("canonical {ordinal}: {}", "payload ".repeat(128))
                }
                MessageContent::SearchProjection => format!("projection {ordinal}"),
            };
            assert_eq!(message.content, expected);
        }
    }
    service.cleanup().await;
}
use crate::{
    message_context::MessageContext,
    shared::protocol::{Batch, ConversationRecord, Event, MessageRecord, Start, View},
    storage::{Message, MessageContent},
};

fn snapshot(revision: i64) -> Vec<Value> {
    let mut vector = vec![0.0; 384];
    vector[0] = 127.0;
    let messages = [
        (0, MessageContext::Root, "root request", "root request"),
        (
            1,
            MessageContext::Parent { ordinal: 0 },
            "left reply",
            "left reply",
        ),
        (
            2,
            MessageContext::Parent { ordinal: 1 },
            "left detail",
            "left detail",
        ),
        (
            3,
            MessageContext::Root,
            "independent root",
            "independent root",
        ),
        (
            5,
            MessageContext::Parent { ordinal: 0 },
            "right request\nprivate tool marker",
            "right request",
        ),
        (
            6,
            MessageContext::Parent { ordinal: 5 },
            "right reply",
            "right reply",
        ),
        (
            8,
            MessageContext::Parent { ordinal: 6 },
            "private output",
            "",
        ),
    ]
    .into_iter()
    .map(|(ordinal, context, content, projection)| MessageRecord {
        id: format!("message-{ordinal}"),
        conversation_id: "branch".into(),
        ordinal,
        context,
        role: if projection.is_empty() {
            crate::role::Role::Tool
        } else {
            crate::role::Role::User
        },
        content: content.into(),
        projection: projection.into(),
        created_at: None,
        generation: (!projection.is_empty())
            .then(|| crate::semantic::SHARED_EMBEDDING_GENERATION.into()),
        vector: (!projection.is_empty()).then(|| vector.clone()),
    })
    .collect();
    [
        Event::Start(Start {
            expected_version: None,
            gold_build: None,
            source_silver: None,
            instance: "branch-test".into(),
            revision: Some(revision),
            base_revision: None,
            snapshot: true,
            forgets: Vec::new(),
        }),
        Event::Batch(Batch::Conversations {
            records: vec![ConversationRecord {
                enrichment: None,
                id: "branch".into(),
                provider: crate::provider::Provider::Pi,
                title: None,
                project: None,
                repository: None,
                created_at: None,
                updated_at: None,
            }],
        }),
        Event::Batch(Batch::Messages { records: messages }),
        Event::Finish,
    ]
    .into_iter()
    .map(|event| serde_json::to_value(event).unwrap())
    .collect()
}

async fn view(
    service: &Service,
    ordinal: i64,
    context: u32,
    representation: MessageContent,
) -> Vec<Message> {
    service
        .request("view", FIRST)
        .json(&View {
            node_id: "first".into(),
            id: format!("message-{ordinal}"),
            context,
            content: representation,
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn contract(service: Service) {
    let response = service.upload(FIRST, &snapshot(1)).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{}",
        response.text().await.unwrap()
    );
    for content in [MessageContent::Canonical, MessageContent::SearchProjection] {
        assert_eq!(
            view(&service, 5, 1, content)
                .await
                .iter()
                .map(|m| m.ordinal)
                .collect::<Vec<_>>(),
            [0, 5, 6]
        );
        assert_eq!(
            view(&service, 0, 2, content)
                .await
                .iter()
                .map(|m| m.ordinal)
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert_eq!(
            view(&service, 3, 3, content)
                .await
                .iter()
                .map(|m| m.ordinal)
                .collect::<Vec<_>>(),
            [3]
        );
    }
    assert!(
        view(&service, 5, 0, MessageContent::Canonical).await[0]
            .content
            .contains("private tool marker")
    );
    assert_eq!(
        view(&service, 5, 0, MessageContent::SearchProjection).await[0].content,
        "right request"
    );
    assert_eq!(
        view(&service, 5, 2, MessageContent::Canonical)
            .await
            .iter()
            .map(|m| m.ordinal)
            .collect::<Vec<_>>(),
        [0, 5, 6, 8]
    );
    let before = service.get("freshness", FIRST).await;
    for parent in [-1, 4, 5, 7] {
        let mut invalid = snapshot(2);
        invalid[2]["records"][4]["context"] = json!(MessageContext::Parent { ordinal: parent });
        let response = service.upload(FIRST, &invalid).await;
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "{}",
            response.text().await.unwrap()
        );
        assert_eq!(service.get("freshness", FIRST).await, before);
        assert_eq!(
            view(&service, 5, 1, MessageContent::Canonical)
                .await
                .iter()
                .map(|m| m.ordinal)
                .collect::<Vec<_>>(),
            [0, 5, 6]
        );
    }
    service.cleanup().await;
}
