//! Persisted ownership rejects corruption while retaining stale candidates for inspection.
use super::*;
use crate::gold::config::EmbeddingText;
use crate::shared::qdrant_index::{PointKind, VectorIndex};

fn active() -> Configuration {
    Configuration {
        candidate: None,
        kind: PointKind::Configuration,
        schema_version: 2,
        version: "a".repeat(64),
        build: Build::default(),
        generation: "test-model".into(),
        dimensions: 4,
        vector_search: VectorIndex::default(),
        publication: Some("b".repeat(64)),
        phase: Some(Phase::Ready),
        nodes: Some(BTreeMap::new()),
    }
}

fn candidate() -> Candidate {
    Candidate {
        schema_version: 1,
        id: "c".repeat(64),
        created_at: chrono::Utc::now(),
        base_epoch: "a".repeat(64),
        build: Build {
            embedding_text: EmbeddingText::TitleAndDialogue,
            ..Default::default()
        },
        generation: "test-model".into(),
        dimensions: 4,
        nodes: BTreeMap::new(),
    }
}

#[test]
fn absent_candidates_remain_wire_compatible_and_stale_epochs_remain_inspectable() {
    let mut active = active();
    let wire = serde_json::to_value(&active).unwrap();
    assert!(wire.get("candidate").is_none());
    let decoded: Configuration = serde_json::from_value(wire).unwrap();
    decoded.validate().unwrap();
    let mut candidate = candidate();
    assert!(candidate.current(&active).unwrap());
    candidate.base_epoch = "d".repeat(64);
    assert!(!candidate.current(&active).unwrap());
    active.candidate = Some(candidate.clone());
    active.validate().unwrap();
    let decoded: Configuration =
        serde_json::from_slice(&serde_json::to_vec(&active).unwrap()).unwrap();
    assert_eq!(decoded.candidate.unwrap(), candidate);
}

#[test]
fn candidate_schema_model_space_and_identity_corruption_fail_closed() {
    let active = active();
    let wire = serde_json::to_value(candidate()).unwrap();
    for (field, value) in [
        ("schema_version", serde_json::json!(2)),
        ("id", serde_json::json!("invalid")),
        ("base_epoch", serde_json::json!("invalid")),
        ("generation", serde_json::json!("another-model")),
        ("dimensions", serde_json::json!(5)),
        ("build", serde_json::to_value(Build::default()).unwrap()),
    ] {
        let mut invalid = wire.clone();
        invalid[field] = value;
        let decoded: Candidate = serde_json::from_value(invalid).unwrap();
        assert!(decoded.validate(&active).is_err(), "{field}");
    }
    let mut unknown = wire;
    unknown["ready"] = serde_json::json!(true);
    assert!(serde_json::from_value::<Candidate>(unknown).is_err());
    let mut candidate = candidate();
    candidate.nodes.insert(
        "node".into(),
        Pin {
            publication: "publication".into(),
            silver: silver_revision::State {
                schema_version: 1,
                revision: 0,
                fingerprint: "e".repeat(64),
            },
        },
    );
    assert!(candidate.validate(&active).is_err());
}
