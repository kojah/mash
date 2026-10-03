use super::*;
use crate::shared::snapshot::{Head, PendingSnapshot, payload::Receiver};
use serde_json::{Value, json};

fn fixture() -> Data {
    let mut data = Data::default();
    data.messages.insert(
        "message".into(),
        MessageRecord {
            id: "message".into(),
            conversation_id: "thread".into(),
            ordinal: 0,
            context: MessageContext::Root,
            role: Role::User,
            content: "canonical 🦀".into(),
            projection: "visible".into(),
            created_at: Some(42),
            generation: Some("model".into()),
            vector: Some(vec![-1.0, 0.0, 1.0]),
        },
    );
    data.conversations.insert(
        "thread".into(),
        ConversationRecord {
            id: "thread".into(),
            provider: crate::provider::Provider::Codex,
            title: Some("Thread".into()),
            project: Some("project".into()),
            repository: None,
            created_at: Some(40),
            updated_at: Some(42),
        },
    );
    data.sources.insert(
        "source".into(),
        Checkpoint {
            provider: crate::provider::Provider::Codex,
            path_hash: "source".into(),
            size_bytes: 4,
            modified_ns: 5,
            fingerprint: "digest".into(),
            conversation_id: Some("thread".into()),
            ready: true,
        },
    );
    data.tombstones.insert(
        "forgotten".into(),
        Tombstone {
            provider: crate::provider::Provider::Codex,
            conversation_id: "forgotten".into(),
        },
    );
    data
}

#[test]
fn persisted_sections_separate_history_and_embeddings_without_losing_records() {
    let data = fixture();
    let layered = serde_json::to_value(data.layers()).unwrap();
    let message = &layered["silver"]["messages"]["message"];
    assert_eq!(message["content"], "canonical 🦀");
    assert!(message.get("vector").is_none());
    assert!(message.get("generation").is_none());
    let embedding = &layered["gold"]["embeddings"]["message"];
    assert!(embedding.get("content").is_none());
    assert_eq!(embedding["vector"], json!([-1.0, 0.0, 1.0]));
    let decoded: Snapshot = serde_json::from_value(layered).unwrap();
    assert_eq!(
        serde_json::to_value(decoded.into_data().unwrap()).unwrap(),
        serde_json::to_value(data).unwrap()
    );
}

#[test]
fn malformed_layer_membership_and_mixed_fields_fail_closed() {
    let original = serde_json::to_value(fixture().layers()).unwrap();
    let mut mixed = original.clone();
    mixed["silver"]["messages"]["message"]["vector"] = json!([1]);
    assert!(serde_json::from_value::<Snapshot>(mixed).is_err());
    let mut orphan = original.clone();
    orphan["silver"]["messages"] = json!({});
    assert!(
        serde_json::from_value::<Snapshot>(orphan)
            .unwrap()
            .into_data()
            .is_err()
    );
    let mut context_only = original.clone();
    context_only["silver"]["messages"]["message"]["projection"] = json!("");
    assert!(
        serde_json::from_value::<Snapshot>(context_only)
            .unwrap()
            .into_data()
            .is_err()
    );
    let mut wrong_id = original;
    wrong_id["silver"]["messages"]["message"]["id"] = json!("other");
    assert!(
        serde_json::from_value::<Snapshot>(wrong_id)
            .unwrap()
            .into_data()
            .is_err()
    );
}

#[tokio::test]
async fn typed_legacy_migration_and_layered_reads_preserve_identical_planning_data() {
    let data = fixture();
    for (schema, value) in [
        (None, serde_json::to_value(&data).unwrap()),
        (
            Some("layered"),
            serde_json::to_value(data.layers()).unwrap(),
        ),
    ] {
        let bytes = serde_json::to_vec(&value).unwrap();
        let mut receiver = Receiver::new(bytes.len()).await.unwrap();
        receiver.append(&bytes).await.unwrap();
        let payload = receiver
            .finish(blake3::hash(&bytes).to_hex().as_str())
            .await
            .unwrap();
        let mut head: Value = json!({"instance":"instance","generation":"model","revision":1,"version":"version","counts":{"conversations":1,"messages":1,"searchable_messages":1,"embeddings":1},"last_synced_at":null,"last_updated_at":null});
        if let Some(schema) = schema {
            head["snapshot_schema"] = json!(schema);
        }
        let head: Head = serde_json::from_value(head).unwrap();
        let loaded = PendingSnapshot {
            payload: Some(payload),
            head: Some(head),
            expected: "version".into(),
        }
        .decode()
        .unwrap();
        assert_eq!(
            serde_json::to_value(&loaded.data).unwrap(),
            serde_json::to_value(&data).unwrap()
        );
        assert_eq!(
            serde_json::to_value(loaded.data.layers()).unwrap()["silver"]["messages"]["message"]["content"],
            "canonical 🦀"
        );
    }
}
