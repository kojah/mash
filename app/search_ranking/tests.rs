use super::*;
use crate::provider::Provider;
use crate::role::Role;
use crate::search_results::{self, ChatMetadata};
use crate::views::config::Fusion;

fn candidate(thread: &str, id: &str, content: &str) -> SearchHit {
    SearchHit {
        id: id.into(),
        chat_id: thread.into(),
        provider: Provider::Codex,
        turn_id: None,
        role: Role::User,
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
fn reranking_ranks_a_selected_threads_fourth_message_first() {
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
    let selection = select(hits, SearchUnit::Chats);
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
    let pending = Pending::new(
        query,
        selection,
        &documents,
        SearchUnit::Chats,
        Fusion::default(),
    )
    .unwrap();
    let refs = pending
        .windows
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let scores = backend.rerank_cpu(query, &refs).unwrap();
    let hits = pending.order(&scores);
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

#[test]
fn chat_selection_takes_the_best_fused_threads_without_inference() {
    // Twelve threads in fused order, interleaved; thread 0 has eight messages.
    let mut hits = (0..12)
        .map(|thread| candidate(&format!("t{thread}"), &format!("t{thread}-0"), "text"))
        .collect::<Vec<_>>();
    hits.extend((1..8).map(|message| candidate("t0", &format!("t0-{message}"), "text")));
    hits.push(candidate("t11", "t11-1", "text"));

    let selection = select(hits, SearchUnit::Chats);

    let ids = |hits: &[SearchHit]| hits.iter().map(|hit| hit.id.clone()).collect::<Vec<_>>();
    let mut expected = (0..10)
        .map(|thread| format!("t{thread}-0"))
        .collect::<Vec<_>>();
    expected.extend((1..6).map(|message| format!("t0-{message}")));
    assert_eq!(ids(&selection.hits[..selection.count]), expected);
    // Later threads and a thread's further messages stay, unranked, in order.
    assert_eq!(
        ids(&selection.hits[selection.count..]),
        ["t10-0", "t11-0", "t0-6", "t0-7", "t11-1"]
    );
}

#[test]
fn one_pass_uses_every_remote_score_or_reranks_entirely_on_cpu() {
    use crate::semantic::remote_reranker::Scores;
    let remote = || {
        Some(Ok(Scores {
            scores: vec![3.0, 2.0],
            inference_milliseconds: 1,
        }))
    };
    let (scores, engine, fallback) = choose(remote(), || panic!("CPU must not run")).unwrap();
    assert_eq!(
        (scores, engine, fallback),
        (vec![3.0, 2.0], RerankerEngine::Remote, None)
    );
    let (scores, engine, fallback) = choose(Some(Err(RerankerFallback::Timeout)), || {
        Ok(vec![-1.0, -2.0])
    })
    .unwrap();
    assert_eq!(
        (scores, engine, fallback),
        (
            vec![-1.0, -2.0],
            RerankerEngine::CpuFallback,
            Some(RerankerFallback::Timeout)
        )
    );
    let (_, engine, fallback) = choose(None, || Ok(vec![0.0, 0.0])).unwrap();
    assert_eq!((engine, fallback), (RerankerEngine::Cpu, None));
    assert!(
        choose(Some(Err(RerankerFallback::Unavailable)), || Err(
            AppError::model("cpu")
        ))
        .is_err()
    );
}

#[tokio::test]
async fn a_timed_out_remote_pass_is_reranked_and_ordered_by_cpu_scores_alone() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let app = axum::Router::new().route(
            "/",
            axum::routing::post(|| async {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                Vec::<u8>::new()
            }),
        );
        axum::serve(listener, app).await.unwrap();
    });
    let root = tempfile::tempdir().unwrap();
    let repository = root.path().join(crate::semantic::RERANKER_REPOSITORY);
    let revision = crate::semantic::remote_reranker::REVISION;
    std::fs::create_dir_all(repository.join("snapshots").join(revision)).unwrap();
    std::fs::create_dir_all(repository.join("refs")).unwrap();
    std::fs::write(repository.join("refs/main"), revision).unwrap();
    let configuration = crate::config::Reranking::Remote {
        url,
        token_env: "HOME".into(),
        timeout_milliseconds: 100,
    };
    let client = crate::semantic::remote_reranker::connect(&configuration, root.path())
        .unwrap()
        .unwrap();
    let hits = vec![
        candidate("a", "first", "one"),
        candidate("b", "second", "two"),
    ];
    let selection = select(hits, SearchUnit::Messages);
    let documents = vec!["one".to_owned(), "two".to_owned()];
    let pending = Pending::new(
        "query",
        selection,
        &documents,
        SearchUnit::Messages,
        Fusion::default(),
    )
    .unwrap();
    let refs = pending
        .windows
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let attempt = Some(client.rerank(&pending.query, &refs).await);
    server.abort();
    let (scores, engine, fallback) = choose(attempt, || Ok(vec![-5.0, 5.0])).unwrap();
    assert_eq!(engine, RerankerEngine::CpuFallback);
    assert_eq!(fallback, Some(RerankerFallback::Timeout));
    let ordered = pending.order(&scores);
    assert_eq!(ids(&ordered), vec!["second", "first"]);
    assert_eq!(ordered[0].rerank_score, Some(5.0));
    assert_eq!(ordered[1].rerank_score, Some(-5.0));
}
