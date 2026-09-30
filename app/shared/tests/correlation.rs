use super::*;
use crate::observability::http::{HEADER, TraceId};
use serde::Deserialize;
use tracing::instrument::WithSubscriber;

#[derive(Deserialize)]
struct TraceEvent {
    fields: TraceFields,
    span: Option<TraceSpan>,
}
#[derive(Deserialize)]
struct TraceFields {
    message: String,
}
#[derive(Deserialize)]
struct TraceSpan {
    name: String,
    trace_id: Option<String>,
    status: Option<u16>,
    outcome: Option<String>,
}

#[tokio::test]
async fn client_success_and_decode_failure_correlate_but_disabled_tracing_sends_no_metadata() {
    use axum::{Json, Router, extract::State, http::HeaderMap, routing::get};
    use std::sync::{Arc, Mutex};
    type ReceivedHeaders = Arc<Mutex<Vec<Option<String>>>>;
    let headers = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
    let records = headers.clone();
    let app = Router::new()
        .route(
            "/success",
            get(
                |State(records): State<ReceivedHeaders>, headers: HeaderMap| async move {
                    records
                        .lock()
                        .unwrap()
                        .push(headers.get(HEADER).map(|h| h.to_str().unwrap().into()));
                    Json(crate::storage::Counts {
                        conversations: 3,
                        messages: 7,
                        searchable_messages: 7,
                        embeddings: 7,
                    })
                },
            ),
        )
        .route("/invalid", get(|| async { "response-secret-canary" }))
        .with_state(records);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::new();
    let (bytes, dispatcher) = crate::observability::test_support::capture();
    let result: crate::storage::Counts = response(client.get(format!("{url}/success")))
        .with_subscriber(dispatcher.clone())
        .await
        .unwrap();
    assert_eq!(result.messages, 7);
    let failure = response::<crate::storage::Counts>(client.get(format!("{url}/invalid")))
        .with_subscriber(dispatcher)
        .await;
    assert!(failure.is_err());
    let result: crate::storage::Counts = response(client.get(format!("{url}/success")))
        .with_subscriber(tracing::subscriber::NoSubscriber::default())
        .await
        .unwrap();
    assert_eq!(result.conversations, 3);
    server.abort();
    let received = headers.lock().unwrap();
    assert_eq!(received.len(), 2);
    let wire = received[0].as_deref().unwrap();
    assert!(TraceId::parse(wire.as_bytes()).is_some());
    assert!(received[1].is_none());
    let output = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    assert!(!output.contains("response-secret-canary"));
    assert!(!output.contains(&url));
    let spans: Vec<_> = output
        .lines()
        .map(|line| serde_json::from_str::<TraceEvent>(line).unwrap())
        .filter(|event| event.fields.message == "close")
        .filter_map(|event| event.span)
        .filter(|span| span.name == "shared_http_request")
        .collect();
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].trace_id.as_deref(), Some(wire));
    assert_ne!(spans[0].trace_id, spans[1].trace_id);
    assert_eq!(spans[0].outcome.as_deref(), Some("success"));
    assert_eq!(spans[1].outcome.as_deref(), Some("error"));
    assert!(spans.iter().all(|span| span.status == Some(200)));
}
