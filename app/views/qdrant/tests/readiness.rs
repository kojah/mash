//! Real writes and reads with deterministic, continuously busy optimizer health.
use super::*;
use axum::{Router, body::Body, extract::State, http::Request, response::Response};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

#[derive(Clone)]
struct Proxy {
    http: reqwest::Client,
    upstream: String,
    collection_path: String,
    health_reads: Arc<AtomicUsize>,
    optimizer_reads: Arc<AtomicUsize>,
}

async fn forward(State(proxy): State<Proxy>, request: Request<Body>) -> Response {
    let (parts, body) = request.into_parts();
    if parts.uri.path().ends_with("/optimizations") {
        proxy.optimizer_reads.fetch_add(1, Ordering::SeqCst);
        return Response::builder().status(503).body(Body::empty()).unwrap();
    }
    let body = axum::body::to_bytes(body, 64 * 1024 * 1024).await.unwrap();
    let response = proxy
        .http
        .request(
            parts.method.clone(),
            format!("{}{}", proxy.upstream, parts.uri),
        )
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.bytes().await.unwrap();
    if parts.method == reqwest::Method::GET && parts.uri.path() == proxy.collection_path {
        proxy.health_reads.fetch_add(1, Ordering::SeqCst);
        // Preserve the real config and counts; only the background status is
        // controlled, so this does not fabricate applied points or search hits.
        let mut info: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        info["result"]["status"] = "yellow".into();
        return Response::builder()
            .status(status)
            .body(Body::from(serde_json::to_vec(&info).unwrap()))
            .unwrap();
    }
    Response::builder()
        .status(status)
        .body(Body::from(bytes))
        .unwrap()
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_publication_searches_applied_data_while_optimizer_stays_busy() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    let proxy = Proxy {
        http: reqwest::Client::new(),
        upstream: fixture.url.clone(),
        collection_path: format!(
            "/collections/{}",
            collection(&fixture.identity.group_id, false)
        ),
        health_reads: Arc::new(AtomicUsize::new(0)),
        optimizer_reads: Arc::new(AtomicUsize::new(0)),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new().fallback(forward).with_state(proxy.clone());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut views = fixture.store.clone();
    views.api = api::Client::new(&url, None, 64 * 1024 * 1024)
        .unwrap()
        .with_mutations(&fixture.grpc_url, None)
        .unwrap();
    let server = fixture.serve(views);
    let started = Instant::now();
    for (revision, marker) in [(1, "initial"), (2, "replacement")] {
        tokio::time::timeout(
            Duration::from_secs(20),
            server.index(&fixture.identity, fixture.file(revision, marker)),
        )
        .await
        .expect("must not wait for the perpetually yellow collection")
        .unwrap();
        assert!(server.freshness(&fixture.identity).await.unwrap().nodes[0].views_current);
        let results = server
            .search(&fixture.identity, fixture.query())
            .await
            .unwrap();
        assert_eq!(results.lexical.len(), 2);
        assert_eq!(results.semantic.len(), 2);
        assert!(
            results
                .lexical
                .iter()
                .chain(&results.semantic)
                .all(|hit| hit.content.contains(marker))
        );
        // Search through a fresh handle too: readiness must not depend on a
        // request-local cache or the proxy reporting a completed optimization.
        let reopened = fixture
            .reopen(VectorIndex::default(), MemoryTier::Cached)
            .await
            .unwrap();
        assert_eq!(
            reopened
                .search(&fixture.identity, fixture.query())
                .await
                .unwrap()
                .semantic
                .len(),
            2
        );
    }
    server
        .index(&fixture.identity, Fixture::forget(3, &["thread"]))
        .await
        .unwrap();
    assert!(server.freshness(&fixture.identity).await.unwrap().nodes[0].views_current);
    let results = server
        .search(&fixture.identity, fixture.query())
        .await
        .unwrap();
    assert!(results.lexical.is_empty());
    assert!(results.semantic.is_empty());
    assert!(proxy.health_reads.load(Ordering::SeqCst) >= 3);
    assert_eq!(proxy.optimizer_reads.load(Ordering::SeqCst), 0);
    eprintln!(
        "busy-optimizer insert/replace/delete and search contract: {:?}",
        started.elapsed()
    );
    task.abort();
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_search_remains_available_during_other_node_publication() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    let mut other = fixture.identity.clone();
    other.node_id = "other-node".into();
    fixture.store.select_views(&other, false).await.unwrap();
    let directory = tempfile::tempdir().unwrap();
    let silver = SilverStore::open(
        &directory.path().join("silver"),
        [&fixture.identity, &other],
    )
    .unwrap();
    let server = Server::new(silver, fixture.store.clone(), 64 * 1024 * 1024);
    server
        .index(&fixture.identity, fixture.file(1, "retained"))
        .await
        .unwrap();
    // Force the small synthetic fixture over the indexing threshold. These
    // settings affect this disposable collection only.
    reqwest::Client::new()
        .patch(format!(
            "{}/collections/{}",
            fixture.url,
            collection(&fixture.identity.group_id, false)
        ))
        .json(&serde_json::json!({"optimizers_config": {"indexing_threshold": 1}}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let template = fixture.messages("background").remove(0);
    let messages: Vec<_> = (0..4096)
        .map(|i| {
            let mut message = template.clone();
            message.id = format!("background-{i}");
            message.ordinal = i;
            message.content = format!("needle background item {i} token{}", i * 7919);
            message.projection.clone_from(&message.content);
            message
        })
        .collect();
    let mut events = vec![
        Event::Start(Fixture::start(1)),
        Event::Batch(Batch::Conversations {
            records: vec![Fixture::conversation()],
        }),
    ];
    events.extend(messages.chunks(128).map(|records| {
        Event::Batch(Batch::Messages {
            records: records.to_vec(),
        })
    }));
    events.push(Event::Finish);
    let file = Fixture::upload(&events);
    let reader = async {
        let mut latencies = Vec::new();
        let mut query = fixture.query();
        query.filter = Some(crate::search_filter::Filter::Node(vec![
            fixture.identity.node_id.clone(),
        ]));
        for _ in 0..20 {
            let start = Instant::now();
            let result = server
                .search(&fixture.identity, query.clone())
                .await
                .unwrap();
            latencies.push(start.elapsed());
            assert_eq!(result.lexical.len(), 2);
            assert_eq!(result.semantic.len(), 2);
            assert!(
                result
                    .semantic
                    .iter()
                    .all(|hit| hit.content.contains("retained"))
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        latencies.sort();
        eprintln!(
            "20 concurrent searches: p50={:?} p95={:?}",
            latencies[9], latencies[18]
        );
    };
    let write = async {
        let start = Instant::now();
        server.index(&other, file).await.unwrap();
        eprintln!("4096-message concurrent publication: {:?}", start.elapsed());
    };
    tokio::join!(reader, write);
    assert!(
        server
            .freshness(&fixture.identity)
            .await
            .unwrap()
            .nodes
            .iter()
            .all(|node| node.views_current)
    );
    fixture.cleanup().await;
}
