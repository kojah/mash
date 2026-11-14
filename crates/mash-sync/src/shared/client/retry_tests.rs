use super::*;
use axum::{Router, extract::State, http::StatusCode, routing::any};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Replies {
    calls: AtomicUsize,
    failures: usize,
    status: StatusCode,
}
#[derive(Debug, serde::Deserialize)]
struct Reply {
    value: u32,
}
async fn reply(State(state): State<Arc<Replies>>) -> (StatusCode, &'static str) {
    if state.calls.fetch_add(1, Ordering::SeqCst) < state.failures {
        return (state.status, "{}");
    }
    (StatusCode::OK, r#"{"value":42}"#)
}
async fn fixture(
    status: StatusCode,
    failures: usize,
) -> (String, Arc<Replies>, tokio::task::JoinHandle<()>) {
    let state = Arc::new(Replies {
        calls: AtomicUsize::new(0),
        failures,
        status,
    });
    let app = Router::new()
        .route("/plan", any(reply))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/plan", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, state, server)
}

#[tokio::test]
async fn busy_publication_negotiation_retries_and_streams_waiting_progress() {
    let (url, state, server) = fixture(StatusCode::SERVICE_UNAVAILABLE, 1).await;
    let (sink, mut progress) = tokio::sync::mpsc::channel(4);
    let result: Reply = crate::progress::scope(
        sink,
        response(reqwest::Client::new().get(url), Endpoint::Freshness),
    )
    .await
    .unwrap();
    assert_eq!(result.value, 42);
    assert_eq!(state.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        progress.try_recv().unwrap().phase,
        crate::progress::Phase::SharedWaiting
    );
    server.abort();
}

#[tokio::test]
async fn search_revision_conflicts_retry_once_without_indexing_progress() {
    for failures in [1, usize::MAX] {
        let (url, state, server) = fixture(StatusCode::CONFLICT, failures).await;
        let (sink, mut progress) = tokio::sync::mpsc::channel(4);
        let result = crate::progress::scope(
            sink,
            response::<Reply>(reqwest::Client::new().post(url).json(&42), Endpoint::Search),
        )
        .await;
        assert_eq!(state.calls.load(Ordering::SeqCst), 2);
        assert!(progress.try_recv().is_err());
        if failures == 1 {
            assert_eq!(result.unwrap().value, 42);
        } else {
            let error = result.unwrap_err();
            assert!(error.error.retryable);
            assert!(error.error.message.contains("retry the search"));
        }
        server.abort();
    }
}

#[tokio::test]
async fn mutation_posts_and_authorization_failures_are_never_replayed() {
    for (endpoint, status) in [
        (Endpoint::Index, StatusCode::SERVICE_UNAVAILABLE),
        (Endpoint::Index, StatusCode::CONFLICT),
        (Endpoint::Search, StatusCode::SERVICE_UNAVAILABLE),
        (Endpoint::Search, StatusCode::UNAUTHORIZED),
        (Endpoint::Freshness, StatusCode::UNAUTHORIZED),
    ] {
        let (url, state, server) = fixture(status, usize::MAX).await;
        let request = if matches!(endpoint, Endpoint::Freshness) {
            reqwest::Client::new().get(url)
        } else {
            reqwest::Client::new().post(url)
        };
        let error = response::<Reply>(request, endpoint).await.unwrap_err();
        assert_eq!(state.calls.load(Ordering::SeqCst), 1);
        assert_eq!(error.error.retryable, status != StatusCode::UNAUTHORIZED);
        if status == StatusCode::SERVICE_UNAVAILABLE {
            assert!(error.error.message.contains("mash status --shared"));
            assert!(error.error.message.contains("phase and progress"));
        }
        server.abort();
    }
}

#[tokio::test]
async fn cancelling_busy_negotiation_releases_bulk_capacity() {
    static CAPACITY: Admission = Admission::new(8, 1);
    let (url, state, server) = fixture(StatusCode::SERVICE_UNAVAILABLE, usize::MAX).await;
    let (sink, mut events) = tokio::sync::mpsc::channel(4);
    let mut call = Box::pin(crate::progress::scope(
        sink,
        response_admitted::<Reply>(
            reqwest::Client::new().get(url),
            Endpoint::Freshness,
            &CAPACITY,
        ),
    ));
    tokio::time::timeout(crate::test_support::HANG_GUARD, async {
        tokio::select! {
            _ = &mut call => panic!("busy negotiation unexpectedly completed"),
            event = events.recv() => assert_eq!(
                event.unwrap().phase,
                crate::progress::Phase::SharedWaiting,
            ),
        }
    })
    .await
    .unwrap();
    drop(call);
    assert_eq!(state.calls.load(Ordering::SeqCst), 1);
    assert_eq!(CAPACITY.bulk.available_permits(), 1);
    assert_eq!(CAPACITY.interactive.available_permits(), 8);
    server.abort();
}

#[tokio::test]
async fn query_budget_errors_preserve_guidance_without_exposing_arbitrary_responses() {
    use axum::routing::post;
    let budget = crate::shared::EXACT_SEARCH_BUDGET_ERROR;
    let old_budget =
        "exact query exceeded the shared search budget; narrow the query or metadata filters";
    for (body, expected) in [
        (
            serde_json::to_string(&AppError::usage(budget)).unwrap(),
            Some(budget),
        ),
        (
            serde_json::to_string(&AppError::usage(old_budget)).unwrap(),
            Some(budget),
        ),
        (
            serde_json::to_string(&AppError::usage("PRIVATE-SERVER-TEXT")).unwrap(),
            None,
        ),
        (
            serde_json::to_string(&AppError::shared(budget)).unwrap(),
            None,
        ),
        ("PRIVATE-PROXY-TEXT".repeat(4096), None),
    ] {
        let app = Router::new().route(
            "/search",
            post(move || {
                let body = body.clone();
                async move { (StatusCode::BAD_REQUEST, body) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/search", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let error = response::<Reply>(reqwest::Client::new().post(url), Endpoint::Search)
            .await
            .unwrap_err();
        if let Some(message) = expected {
            assert_eq!(error.error.kind, crate::ErrorKind::Usage);
            assert_eq!(error.error.message, message);
            assert_eq!(error.code, 2);
        } else {
            assert_eq!(error.error.kind, crate::ErrorKind::SharedIndex);
            assert_eq!(
                error.error.message,
                "shared index rejected invalid or oversized data"
            );
        }
        assert!(!error.error.retryable);
        server.abort();
    }
}
