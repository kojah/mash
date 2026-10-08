use super::*;
use crate::{provider::Provider, role::Role, search_results::ThreadMetadata};

fn hit(node: &str, content: &str) -> SearchHit {
    SearchHit {
        id: "message".into(),
        conversation_id: "thread".into(),
        provider: Provider::Codex,
        role: Role::User,
        content: content.into(),
        created_at: Some(123),
        metadata: ThreadMetadata {
            title: Some("password=\"rosebud\"".into()),
            ..ThreadMetadata::default()
        },
        copies: Vec::new(),
        lexical_score: Some(0.25),
        lexical_match: None,
        semantic_score: Some(0.5),
        fusion_score: 0.02,
        rerank_score: None,
        node_id: Some(node.into()),
    }
}

#[test]
fn compact_candidates_restore_identical_redacted_hits_before_ranking() {
    let text = format!(
        "🦀 password=\"hunter2\" {}",
        "useful evidence ".repeat(1000)
    );
    let mut candidates = protocol::Candidates {
        lexical: vec![hit("first", &text), hit("second", "other node evidence")],
        semantic: vec![hit("first", &text)],
        documents: vec![
            protocol::Document {
                node_id: "first".into(),
                id: "message".into(),
                projection: text,
            },
            protocol::Document {
                node_id: "second".into(),
                id: "message".into(),
                projection: "other node evidence".into(),
            },
        ],
    };
    // The baseline response carries the same redacted text in both arrays.
    for hit in candidates
        .lexical
        .iter_mut()
        .chain(&mut candidates.semantic)
    {
        hit.content = crate::redaction::projection(&hit.content);
        crate::redaction::metadata(&mut hit.metadata);
    }
    for document in &mut candidates.documents {
        document.projection = crate::redaction::projection(&document.projection);
    }
    let baseline = serde_json::to_vec(&candidates).unwrap();
    super::super::server::prepare_candidates(&mut candidates);
    assert!(
        candidates
            .lexical
            .iter()
            .chain(&candidates.semantic)
            .all(|h| h.content.is_empty())
    );
    let compact = serde_json::to_vec(&candidates).unwrap();
    assert!(compact.len() * 2 < baseline.len());
    let encoded = std::str::from_utf8(&compact).unwrap();
    assert!(!encoded.contains("hunter2") && !encoded.contains("rosebud"));
    let mut decoded: protocol::Candidates = serde_json::from_slice(&compact).unwrap();
    let documents = projected_documents(std::mem::take(&mut decoded.documents));
    apply_projections(
        decoded.lexical.iter_mut().chain(&mut decoded.semantic),
        &documents,
    );
    let expected: protocol::Candidates = serde_json::from_slice(&baseline).unwrap();
    assert_eq!(
        serde_json::to_value(&decoded.lexical).unwrap(),
        serde_json::to_value(expected.lexical).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&decoded.semantic).unwrap(),
        serde_json::to_value(expected.semantic).unwrap()
    );
}

#[test]
fn compaction_preserves_inline_text_without_an_identical_node_message_projection() {
    let mut candidates = protocol::Candidates {
        lexical: vec![
            hit("first", "literal evidence"),
            hit("second", "different projection"),
        ],
        semantic: Vec::new(),
        documents: vec![protocol::Document {
            node_id: "second".into(),
            id: "message".into(),
            projection: "literal evidence".into(),
        }],
    };
    super::super::server::prepare_candidates(&mut candidates);
    assert_eq!(candidates.lexical[0].content, "literal evidence");
    assert_eq!(candidates.lexical[1].content, "different projection");
    candidates.documents.clear();
    super::super::server::prepare_candidates(&mut candidates);
    assert_eq!(candidates.lexical[0].content, "literal evidence");
    assert_eq!(candidates.lexical[1].content, "different projection");
}
