use super::*;
use axum::{Router, body::Bytes, extract::State, http::StatusCode, routing::put};
use serde::Deserialize;
use std::sync::{Arc, Mutex};

#[derive(Serialize, Deserialize)]
struct Payload {
    text: String,
}
#[derive(Deserialize)]
struct Request {
    points: Vec<Received>,
}
#[derive(Deserialize)]
struct Received {
    id: String,
    payload: Payload,
}
#[derive(Default)]
struct Captured {
    requests: Vec<(usize, Vec<Received>)>,
    fail_request: Option<usize>,
}
type Shared = Arc<Mutex<Captured>>;

async fn receive(State(state): State<Shared>, bytes: Bytes) -> (StatusCode, &'static str) {
    let request: Request = serde_json::from_slice(&bytes).unwrap();
    let mut state = state.lock().unwrap();
    state.requests.push((bytes.len(), request.points));
    if state.fail_request == Some(state.requests.len()) {
        return (StatusCode::SERVICE_UNAVAILABLE, "{}");
    }
    (
        StatusCode::OK,
        r#"{"status":"ok","result":{"status":"completed"}}"#,
    )
}

async fn fixture(fail_request: Option<usize>) -> (Client, Shared, tokio::task::JoinHandle<()>) {
    let captured = Arc::new(Mutex::new(Captured {
        fail_request,
        ..Captured::default()
    }));
    let router = Router::new()
        .route("/collections/test/points", put(receive))
        .layer(axum::extract::DefaultBodyLimit::max(32 * 1024 * 1024))
        .with_state(captured.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::new(
        &format!("http://{}", listener.local_addr().unwrap()),
        None,
        64 * 1024 * 1024,
    )
    .unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (client, captured, server)
}

fn point(id: usize, text: &str) -> Point<Payload> {
    Point {
        id: id.to_string(),
        vector: super::super::Vectors::default(),
        payload: Payload { text: text.into() },
    }
}

#[tokio::test]
async fn staging_bounds_record_count_and_preserves_typed_unicode_records() {
    let (client, captured, server) = fixture(None).await;
    let mut batch = StagingBatch::default();
    batch.flush(&client, "test").await.unwrap();
    for id in 0..=MAX_POINTS * 2 {
        batch
            .push(&client, "test", point(id, "λ\n\"quoted\"\\"))
            .await
            .unwrap();
    }
    batch.flush(&client, "test").await.unwrap();
    let captured = captured.lock().unwrap();
    assert_eq!(
        captured
            .requests
            .iter()
            .map(|(_, p)| p.len())
            .collect::<Vec<_>>(),
        [512, 512, 1]
    );
    for (id, point) in captured.requests.iter().flat_map(|(_, p)| p).enumerate() {
        assert_eq!(point.id, id.to_string());
        assert_eq!(point.payload.text, "λ\n\"quoted\"\\");
    }
    server.abort();
}

#[tokio::test]
async fn staging_bounds_encoded_bytes_and_sends_large_valid_points_alone() {
    let (client, captured, server) = fixture(None).await;
    let mut batch = StagingBatch::default();
    for id in 0..3 {
        batch
            .push(&client, "test", point(id, &"x".repeat(TARGET_BYTES / 2)))
            .await
            .unwrap();
    }
    batch
        .push(&client, "test", point(3, &"x".repeat(TARGET_BYTES + 1)))
        .await
        .unwrap();
    batch.flush(&client, "test").await.unwrap();
    let captured = captured.lock().unwrap();
    assert_eq!(captured.requests.len(), 4);
    assert!(captured.requests.iter().all(|(_, p)| p.len() == 1));
    assert!(
        captured.requests[..3]
            .iter()
            .all(|(bytes, _)| *bytes <= TARGET_BYTES + 1024)
    );
    assert!(captured.requests[3].0 > TARGET_BYTES);
    server.abort();
}

#[tokio::test]
async fn staging_rejects_oversized_points_before_network_io() {
    let (client, captured, server) = fixture(None).await;
    let error = StagingBatch::default()
        .push(&client, "test", point(0, &"x".repeat(MAX_POINT_BYTES + 1)))
        .await
        .unwrap_err();
    assert_eq!(error.error.kind, crate::ErrorKind::Usage);
    assert!(captured.lock().unwrap().requests.is_empty());
    server.abort();
}

#[tokio::test]
async fn staging_stops_at_a_failed_batch_without_acknowledging_it() {
    let (client, captured, server) = fixture(Some(2)).await;
    let mut batch = StagingBatch::default();
    for id in 0..MAX_POINTS {
        batch
            .push(&client, "test", point(id, "first"))
            .await
            .unwrap();
    }
    for id in MAX_POINTS..MAX_POINTS * 2 - 1 {
        batch
            .push(&client, "test", point(id, "second"))
            .await
            .unwrap();
    }
    assert!(
        batch
            .push(&client, "test", point(MAX_POINTS * 2 - 1, "second"))
            .await
            .is_err()
    );
    assert_eq!(captured.lock().unwrap().requests.len(), 2);
    assert_eq!(batch.points.len(), MAX_POINTS);
    server.abort();
}
