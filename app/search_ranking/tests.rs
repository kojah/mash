use super::*;
use crate::provider::Provider;
use crate::role::Role;
use crate::search_results::ThreadMetadata;

fn candidate(thread: &str, id: &str, content: &str) -> SearchHit {
    SearchHit {
        id: id.into(),
        conversation_id: thread.into(),
        provider: Provider::Codex,
        role: Role::User,
        content: content.into(),
        copies: Vec::new(),
        created_at: None,
        metadata: ThreadMetadata::default(),
        lexical_score: None,
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
    let messages = finish_fusion(hits(), SearchUnit::Messages, 1);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].id, "first");
    let threads = finish_fusion(hits(), SearchUnit::Threads, 1);
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
    let hits = search_results::diversify(hits, SearchUnit::Threads);
    let selection = select(&mut backend, query, hits, SearchUnit::Threads, 5).unwrap();
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
        SearchUnit::Threads,
        5,
    )
    .unwrap();
    assert_eq!(hits[0].id, "warm-cache");
}
