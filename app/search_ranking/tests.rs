use super::*;
use crate::provider::Provider;
use crate::role::Role;
use crate::search_results::ChatMetadata;
use crate::views::config::Fusion;

fn candidate(thread: &str, id: &str, content: &str) -> SearchHit {
    SearchHit {
        id: id.into(),
        conversation_id: thread.into(),
        source: crate::source::Source::Provider(Provider::Codex),
        object: crate::source::ObjectType::native_message(Provider::Codex),
        ancestors: Vec::new(),
        role: Some(Role::User),
        author_kind: None,
        author: None,
        sequence: None,
        content: content.into(),
        copies: Vec::new(),
        created_at: None,
        metadata: ChatMetadata::default(),
        lexical_score: None,
        lookup: None,
        lexical_match: None,
        semantic_score: Some(0.4),
        fusion_score: 0.02,
        rerank_score: None,
        node_id: None,
    }
}

#[test]
fn fusion_finish_preserves_order_and_thread_evidence_without_inference() {
    let hits = || {
        vec![
            candidate("a", "first", "one"),
            candidate("a", "second", "two"),
            candidate("b", "third", "three"),
        ]
    };
    let threads = finish_fusion(hits(), Fusion::default());
    assert_eq!(
        threads
            .iter()
            .map(|hit| hit.id.as_str())
            .collect::<Vec<_>>(),
        vec!["first", "second", "third"]
    );
    assert!(threads.iter().all(|hit| hit.rerank_score.is_none()));
}

#[test]
#[ignore = "requires MASH_TEST_MODELS_DIR containing an explicit model installation"]
fn thread_selection_keeps_fourth_anchor_then_ranks_its_actual_evidence() {
    let root = std::env::var_os("MASH_TEST_MODELS_DIR").unwrap().into();
    let mut backend = crate::semantic::Models::new(root).load().unwrap().unwrap();
    let query = "keeping a model warm between CLI searches";
    let mut hits = vec![
        candidate("cache", "generic-1", "how can we improve search"),
        candidate(
            "cache",
            "generic-2",
            "please improve search however you think appropriate",
        ),
        candidate("cache", "generic-3", "what other parts can we optimize"),
        candidate(
            "cache",
            "warm-cache",
            "Can the CLI cache the loaded embedding model in a worker, so multiple searches avoid model loading?",
        ),
    ];
    hits.extend((0..25).map(|i| {
        candidate(
            &format!("gardening-{i}"),
            &format!("carrot-{i}"),
            "I planted carrots in my garden.",
        )
    }));
    let hits = search_results::diversify(hits, SearchUnit::Chats);
    let selection = select(&mut backend, query, hits, SearchUnit::Chats).unwrap();
    assert!(selection.count <= 30);
    assert!(
        selection.hits[..selection.count]
            .iter()
            .any(|hit| hit.id == "warm-cache")
    );
    let documents = selection.hits[..selection.count]
        .iter()
        .map(|hit| search_results::contextual_document(hit, query, &[]))
        .collect::<Vec<_>>();
    let hits = score(
        &mut backend,
        query,
        selection,
        &documents,
        SearchUnit::Chats,
        Fusion::default(),
    )
    .unwrap();
    assert_eq!(hits[0].id, "warm-cache");
}

/// Twenty hits in relevance order; `newest` is dated last, `oldest` first.
fn dated(oldest: usize, newest: usize) -> Vec<SearchHit> {
    (0..20)
        .map(|rank| {
            let mut hit = candidate("chat", &format!("r{rank}"), "text");
            hit.created_at = Some(match rank {
                _ if rank == oldest => 0,
                _ if rank == newest => 10_000,
                _ => 5_000 - i64::try_from(rank).unwrap(),
            });
            hit
        })
        .collect()
}

fn ids(hits: &[SearchHit]) -> Vec<&str> {
    hits.iter().map(|hit| hit.id.as_str()).collect()
}

#[test]
fn recency_lifts_a_newer_message_among_comparably_relevant_ones() {
    // The newest message one place below an old top match overtakes it.
    let mut hits = dated(0, 1);
    favour_recent(&mut hits, Fusion::default());
    assert_eq!(hits[0].id, "r1");
    // Ten places down, being newest is not enough to reach the top.
    let mut hits = dated(0, 10);
    favour_recent(&mut hits, Fusion::default());
    assert_ne!(hits[0].id, "r10");
    // A zero weight ranks by relevance alone.
    let mut hits = dated(0, 1);
    favour_recent(
        &mut hits,
        Fusion {
            recency_weight: 0,
            ..Fusion::default()
        },
    );
    assert_eq!(ids(&hits)[..2], ["r0", "r1"]);
}

#[test]
fn recent_sort_orders_messages_and_chats_newest_first() {
    let mut old = candidate("a", "old", "text");
    old.created_at = Some(1);
    let mut undated = candidate("b", "undated", "text");
    undated.created_at = None;
    let mut new = candidate("c", "new", "text");
    new.created_at = Some(9);
    let mut results = search_results::Results::Messages(vec![old, undated, new]);
    results.newest_first();
    let search_results::Results::Messages(hits) = results else {
        unreachable!()
    };
    // Undated results keep their relevance order, after dated ones.
    assert_eq!(ids(&hits), ["new", "old", "undated"]);
}
