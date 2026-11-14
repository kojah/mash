use crate::provider::Provider;
use crate::search_results::SearchUnit;
use crate::storage::SearchHit;

fn hit(thread: &str, id: &str) -> SearchHit {
    SearchHit {
        id: id.into(),
        chat_id: thread.into(),
        provider: Provider::Codex,
        turn_id: None,
        role: crate::role::Role::User,
        sequence: None,
        content: id.into(),
        copies: Vec::new(),
        created_at: Some(123),
        metadata: crate::search_results::ChatMetadata {
            enrichment: None,
            title: Some(thread.into()),
            ..Default::default()
        },
        lexical_score: Some(1.0),
        lookup: None,
        lexical_match: None,
        semantic_score: None,
        fusion_score: 0.03,
        rerank_score: None,
        node_id: None,
    }
}

#[test]
fn message_replicas_fuse_once_even_when_rankers_choose_different_nodes() {
    let mut lexical_copy = hit("thread", "a");
    lexical_copy.node_id = Some("peer".into());
    let mut semantic_copy = hit("thread", "a");
    semantic_copy.node_id = Some("other-peer".into());
    semantic_copy.semantic_score = Some(0.9);
    let rows = crate::semantic::fuse(
        vec![hit("thread", "a"), lexical_copy, hit("thread", "b")],
        vec![semantic_copy, hit("thread", "b")],
        SearchUnit::Messages,
        crate::views::config::Fusion::default(),
    );
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, "a");
    assert_eq!(rows[0].copies.len(), 2);
    assert_eq!(rows[0].semantic_score, Some(0.9));
    assert!((rows[0].fusion_score - 2.0 / 61.0).abs() < 1e-6);
    assert!((rows[1].fusion_score - 2.0 / 62.0).abs() < 1e-6);
}
