use super::*;
use crate::chat_catalog::Page;
use axum::extract::State;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[tokio::test]
async fn malformed_catalog_json_falls_back_without_forwarding_response_contents() {
    let app = Router::new().route(
        "/v1/chats",
        post(|| async { "private-response-canary-not-valid-json" }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path(), &url);
    let database = database(directory.path());
    let response = run(&config, &database, Request::default(), false, false, false)
        .await
        .unwrap();
    assert_eq!(response.chats.len(), 1);
    let error = response.shared_error.unwrap();
    assert!(error.contains("invalid shared index HTTP response"));
    assert!(!error.contains("private-response-canary"));
    assert!(
        run(&config, &database, Request::default(), false, true, false)
            .await
            .is_err()
    );
    server.abort();
}

#[tokio::test]
async fn malformed_catalog_pages_fail_once_and_preserve_local_metadata() {
    let requests = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/v1/chats",
            post(
                |State(requests): State<Arc<AtomicUsize>>, Json(input): Json<Request>| async move {
                    requests.fetch_add(1, Ordering::Relaxed);
                    Json(Page {
                        chats: Vec::new(),
                        next_offset: Some(input.offset + input.limit),
                    })
                },
            ),
        )
        .with_state(requests.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path(), &url);
    let database = database(directory.path());
    let response = run(&config, &database, Request::default(), false, false, false)
        .await
        .unwrap();
    assert_eq!(response.chats.len(), 1);
    assert!(!response.shared);
    assert!(response.shared_error.unwrap().contains("pagination"));
    assert_eq!(
        requests.load(Ordering::Relaxed),
        1,
        "a nonterminal page must contain the requested number of rows"
    );
    server.abort();
}

#[tokio::test]
async fn shared_catalog_deadline_covers_all_pages_and_discards_partial_remote_results() {
    let requests = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/v1/chats",
            post(
                |State(requests): State<Arc<AtomicUsize>>, Json(input): Json<Request>| async move {
                    requests.fetch_add(1, Ordering::Relaxed);
                    tokio::time::sleep(Duration::from_millis(1200)).await;
                    let mut collector = Collector::new(input).unwrap();
                    for number in 0..120 {
                        collector.push(Chat {
                            chat_id: format!("peer-{number:03}"),
                            first_message_id: None,
                            provider: Provider::Codex,
                            node_id: "peer".into(),
                            metadata: ChatMetadata {
                                enrichment: None,
                                updated_at: Some(number),
                                ..Default::default()
                            },
                        });
                    }
                    Json(collector.finish())
                },
            ),
        )
        .with_state(requests.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path(), &url);
    let database = database(directory.path());
    let start = std::time::Instant::now();
    let response = run(
        &config,
        &database,
        Request {
            limit: 100,
            ..Default::default()
        },
        false,
        false,
        false,
    )
    .await
    .unwrap();
    let elapsed = start.elapsed();
    assert!(
        elapsed >= Duration::from_millis(1800) && elapsed < Duration::from_secs(3),
        "{elapsed:?}"
    );
    assert_eq!(requests.load(Ordering::Relaxed), 2);
    assert_eq!(response.chats.len(), 1);
    assert_eq!(response.chats[0].chat.node_id, "laptop");
    assert!(!response.shared);
    assert!(response.shared_error.unwrap().contains("deadline"));
    server.abort();
}
