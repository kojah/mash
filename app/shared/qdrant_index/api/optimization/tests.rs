use super::*;
use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

struct Replies {
    health: VecDeque<&'static str>,
    progress: VecDeque<&'static str>,
    health_calls: usize,
    progress_calls: usize,
}

type Shared = Arc<Mutex<Replies>>;

async fn health(State(shared): State<Shared>) -> String {
    let mut state = shared.lock().unwrap();
    state.health_calls += 1;
    let reply = if state.health.len() == 1 {
        state.health[0]
    } else {
        state.health.pop_front().unwrap()
    };
    format!(r#"{{"status":"ok","result":{reply}}}"#)
}

async fn progress(State(shared): State<Shared>) -> String {
    let mut state = shared.lock().unwrap();
    state.progress_calls += 1;
    let reply = if state.progress.len() == 1 {
        state.progress[0]
    } else {
        state.progress.pop_front().unwrap()
    };
    format!(r#"{{"status":"ok","result":{reply}}}"#)
}

async fn fixture(
    health_replies: &[&'static str],
    progress_replies: &[&'static str],
) -> (Client, Shared, tokio::task::JoinHandle<()>) {
    let state = Arc::new(Mutex::new(Replies {
        health: health_replies.iter().copied().collect(),
        progress: progress_replies.iter().copied().collect(),
        health_calls: 0,
        progress_calls: 0,
    }));
    let app = Router::new()
        .route("/collections/test", get(health))
        .route("/collections/test/optimizations", get(progress))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::new(
        &format!("http://{}", listener.local_addr().unwrap()),
        None,
        4096,
    )
    .unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (client, state, server)
}

const GREEN: &str = r#"{"status":"green","optimizer_status":"ok"}"#;
const YELLOW: &str = r#"{"status":"yellow","optimizer_status":"ok"}"#;
const IDLE: &str = r#"{"summary":{"queued_optimizations":0},"running":[]}"#;

#[tokio::test]
async fn acknowledgement_waits_for_running_and_queued_optimizations() {
    let (client, state, server) = fixture(
        &[YELLOW, GREEN],
        &[
            r#"{"summary":{"queued_optimizations":0},"running":[{"diagnostic":"private-canary"}]}"#,
            r#"{"summary":{"queued_optimizations":1},"running":[]}"#,
            IDLE,
        ],
    )
    .await;
    client
        .settle_search_with_budget("test", Duration::from_secs(2), Duration::from_millis(1))
        .await
        .unwrap();
    let state = state.lock().unwrap();
    assert_eq!(state.health_calls, 4);
    assert_eq!(state.progress_calls, 3);
    server.abort();
}

#[tokio::test]
async fn healthy_small_publications_do_not_wait() {
    let (client, state, server) = fixture(&[GREEN], &[IDLE]).await;
    client
        .settle_search_with_budget("test", Duration::from_secs(2), Duration::from_secs(10))
        .await
        .unwrap();
    let state = state.lock().unwrap();
    assert_eq!(state.health_calls, 1);
    assert_eq!(state.progress_calls, 1);
    server.abort();
}

#[tokio::test]
async fn failed_paused_and_malformed_health_fail_closed_without_diagnostics() {
    for reply in [
        r#"{"status":"red","optimizer_status":"ok"}"#,
        r#"{"status":"grey","optimizer_status":"ok"}"#,
        r#"{"status":"green","optimizer_status":{"error":"private-canary"}}"#,
        r#"{"status":"foreign","optimizer_status":"ok"}"#,
    ] {
        let (client, state, server) = fixture(&[reply], &[IDLE]).await;
        let error = client
            .settle_search_with_budget("test", Duration::from_secs(2), Duration::from_millis(1))
            .await
            .unwrap_err();
        assert!(error.error.message.contains("committed"));
        assert!(!error.error.message.contains("private-canary"));
        assert_eq!(state.lock().unwrap().health_calls, 1);
        server.abort();
    }
}

#[tokio::test]
async fn drain_budget_cancels_stalled_health_reads() {
    let app = Router::new().route(
        "/collections/test",
        get(|| async { std::future::pending::<String>().await }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::new(
        &format!("http://{}", listener.local_addr().unwrap()),
        None,
        4096,
    )
    .unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let started = std::time::Instant::now();
    let error = client
        .settle_search_with_budget("test", Duration::from_millis(50), Duration::from_millis(1))
        .await
        .unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(error.error.message.contains("committed"));
    assert!(error.error.message.contains("still optimizing"));
    server.abort();
}

#[derive(Deserialize)]
struct Probe {
    query: Vec<f64>,
    using: String,
    params: ProbeParams,
    limit: usize,
    with_payload: bool,
    filter: ProbeFilter,
}

#[derive(Deserialize)]
struct ProbeParams {
    exact: bool,
    hnsw_ef: usize,
}

#[derive(Deserialize)]
struct ProbeFilter {
    must: Vec<ProbeCondition>,
}

#[derive(Deserialize)]
struct ProbeCondition {
    key: String,
    r#match: ProbeMatch,
}

#[derive(Deserialize)]
struct ProbeMatch {
    any: Vec<String>,
}

#[tokio::test]
async fn readiness_probe_scans_published_vectors_without_message_payloads() {
    let calls = Arc::new(Mutex::new(0_usize));
    let app = Router::new()
        .route(
            "/collections/test/points/query",
            post(
                |State(calls): State<Arc<Mutex<usize>>>, Json(input): Json<Probe>| async move {
                    let mut count = calls.lock().unwrap();
                    assert_eq!(input.query, [1.0, 0.0, 0.0, 0.0]);
                    assert_eq!(input.using, "dense");
                    assert_eq!(input.params.exact, *count == 0);
                    assert_eq!(input.params.hnsw_ef, if *count == 0 { 200 } else { 80 });
                    assert_eq!(input.limit, if *count == 0 { 1 } else { 200 });
                    assert!(!input.with_payload);
                    assert_eq!(input.filter.must.len(), 1);
                    assert_eq!(input.filter.must[0].key, "node_id");
                    assert_eq!(input.filter.must[0].r#match.any, ["node", "other-node"]);
                    *count += 1;
                    r#"{"status":"ok","result":{"points":[{"id":"point","score":0.5}]}}"#
                },
            ),
        )
        .with_state(calls.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::new(
        &format!("http://{}", listener.local_addr().unwrap()),
        None,
        4096,
    )
    .unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    client
        .warm_search(
            "test",
            "dense",
            4,
            &["node".into(), "other-node".into()],
            VectorIndex::Hnsw { ef_runtime: 80 },
        )
        .await
        .unwrap();
    assert_eq!(*calls.lock().unwrap(), 2);
    server.abort();
}

#[tokio::test]
async fn failed_readiness_probe_retains_committed_notice_without_upstream_body() {
    let app = Router::new().route(
        "/collections/test/points/query",
        post(|| async {
            (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "private-canary",
            )
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::new(
        &format!("http://{}", listener.local_addr().unwrap()),
        None,
        4096,
    )
    .unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let error = client
        .warm_search(
            "test",
            "dense",
            4,
            &["published-version".into()],
            VectorIndex::Exact {},
        )
        .await
        .unwrap_err();
    assert!(error.error.message.contains("committed"));
    assert!(!error.error.message.contains("private-canary"));
    assert!(
        client
            .warm_search("test", "dense", 0, &[], VectorIndex::Exact {})
            .await
            .is_err()
    );
    server.abort();
}
