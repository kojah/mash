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
            enrichment: None,
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
            interpretation_version: crate::silver::certificate::INTERPRETATION_VERSION,
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
fn canonical_identity_excludes_gold_and_checkpoints_and_tracks_silver_edits() {
    let mut data = fixture();
    let fingerprint = data.canonical_fingerprint().unwrap();
    let message = data.messages.get_mut("message").unwrap();
    message.vector = Some(vec![127.0; 1024]);
    message.generation = Some("different model".into());
    data.sources.clear();
    assert_eq!(data.canonical_fingerprint().unwrap(), fingerprint);
    let restored = serde_json::from_value::<Snapshot>(serde_json::to_value(data.layers()).unwrap())
        .unwrap()
        .into_data()
        .unwrap();
    assert_eq!(restored.canonical_fingerprint().unwrap(), fingerprint);

    for field in [
        "content",
        "projection",
        "role",
        "context",
        "created_at",
        "ordinal",
    ] {
        let mut data = fixture();
        let message = data.messages.get_mut("message").unwrap();
        match field {
            "content" => message.content.push_str(" edited"),
            "projection" => message.projection.push_str(" interpreted"),
            "role" => message.role = Role::Assistant,
            "context" => message.context = MessageContext::Linear,
            "created_at" => message.created_at = Some(99),
            "ordinal" => message.ordinal = 1,
            _ => unreachable!(),
        }
        assert_ne!(
            data.canonical_fingerprint().unwrap(),
            fingerprint,
            "{field}"
        );
    }
    let mut data = fixture();
    data.conversations.get_mut("thread").unwrap().title = Some("edited title".into());
    assert_ne!(data.canonical_fingerprint().unwrap(), fingerprint);
    data = fixture();
    data.tombstones.clear();
    assert_ne!(data.canonical_fingerprint().unwrap(), fingerprint);
}

#[test]
#[ignore = "manual synthetic serialization-cost diagnostic"]
fn benchmark_canonical_fingerprint_cost() {
    let mut data = fixture();
    let mut message = data.messages.remove("message").unwrap();
    message.content = "representative dialogue text with Unicode 🦀 ".repeat(24);
    message.projection.clone_from(&message.content);
    message.vector = Some(vec![1.0; 384]);
    for ordinal in 0..10_000 {
        let mut row = message.clone();
        row.ordinal = ordinal;
        row.id = format!("message-{ordinal}");
        data.messages.insert(row.id.clone(), row);
    }
    let started = std::time::Instant::now();
    let encoded = crate::shared::snapshot::Encoded::new(&data.layers(), 128 * 1024 * 1024).unwrap();
    let encoding = started.elapsed();
    let started = std::time::Instant::now();
    let fingerprint = data.canonical_fingerprint().unwrap();
    let hashing = started.elapsed();
    assert_eq!(fingerprint.len(), 64);
    eprintln!(
        "synthetic messages=10000 encoded_bytes={} encoding_ms={} canonical_hash_ms={}",
        encoded.bytes,
        encoding.as_millis(),
        hashing.as_millis()
    );
}

#[test]
fn missing_checkpoint_versions_remain_unverified_after_snapshot_decode() {
    let mut value = serde_json::to_value(fixture().layers()).unwrap();
    value["silver"]["sources"]["source"]
        .as_object_mut()
        .unwrap()
        .remove("interpretation_version");
    let data = serde_json::from_value::<Snapshot>(value)
        .unwrap()
        .into_data()
        .unwrap();
    assert_eq!(data.sources["source"].interpretation_version, 0);
    let identity = crate::shared::server::Identity {
        group_id: "group".into(),
        node_id: "node".into(),
        generation: "model".into(),
        dimensions: 3,
    };
    assert!(
        !data
            .state(&identity, &crate::gold::config::Build::default())
            .sources[0]
            .ready
    );
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
    assert!(
        layered["silver"]["sources"]["source"]
            .get("ready")
            .is_none()
    );
    let decoded: Snapshot = serde_json::from_value(layered).unwrap();
    assert_eq!(
        serde_json::to_value(decoded.into_data().unwrap().layers()).unwrap(),
        serde_json::to_value(data.layers()).unwrap()
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
            serde_json::to_value(loaded.data.layers()).unwrap(),
            serde_json::to_value(data.layers()).unwrap()
        );
        assert_eq!(
            serde_json::to_value(loaded.data.layers()).unwrap()["silver"]["messages"]["message"]["content"],
            "canonical 🦀"
        );
    }
}

#[test]
fn source_facts_are_independent_of_gold_and_input_readiness_claims() {
    let mut data = fixture();
    let identity = crate::shared::server::Identity {
        group_id: "group".into(),
        node_id: "node".into(),
        generation: "model".into(),
        dimensions: 3,
    };
    let silver = serde_json::to_value(data.silver()).unwrap();
    assert!(silver["sources"]["source"].get("ready").is_none());
    assert!(
        data.state(&identity, &crate::gold::config::Build::default())
            .sources[0]
            .ready
    );
    data.sources.get_mut("source").unwrap().ready = false;
    assert_eq!(serde_json::to_value(data.silver()).unwrap(), silver);
    assert!(
        data.state(&identity, &crate::gold::config::Build::default())
            .sources[0]
            .ready
    );
    data.messages.get_mut("message").unwrap().vector = None;
    assert_eq!(serde_json::to_value(data.silver()).unwrap(), silver);
    assert!(
        !data
            .state(&identity, &crate::gold::config::Build::default())
            .sources[0]
            .ready
    );

    // Existing snapshots may carry this known transport field. It never
    // supplies readiness when joining source facts with current gold.
    for claimed in [true, false] {
        let mut previous = serde_json::to_value(data.layers()).unwrap();
        previous["silver"]["sources"]["source"]["ready"] = json!(claimed);
        let decoded = serde_json::from_value::<Snapshot>(previous)
            .unwrap()
            .into_data()
            .unwrap();
        assert_eq!(serde_json::to_value(decoded.silver()).unwrap(), silver);
        assert!(!decoded.sources["source"].ready);
        assert!(
            !decoded
                .state(&identity, &crate::gold::config::Build::default())
                .sources[0]
                .ready
        );
    }
    let mut invalid = serde_json::to_value(data.layers()).unwrap();
    invalid["silver"]["sources"]["source"]["generation"] = json!("model");
    assert!(serde_json::from_value::<Snapshot>(invalid).is_err());
}
