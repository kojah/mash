use super::*;
use crate::observability::http::{HEADER, TraceId};
use serde::Deserialize;
use tracing::instrument::WithSubscriber;

#[derive(Deserialize)]
struct TraceEvent {
    fields: TraceFields,
    span: Option<TraceSpan>,
    spans: Option<Vec<TraceSpan>>,
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
    bytes: Option<usize>,
    queue_ms: Option<u64>,
    endpoint: Option<String>,
    admission: Option<String>,
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
    let result: crate::storage::Counts =
        response(client.get(format!("{url}/success")), Endpoint::Freshness)
            .with_subscriber(dispatcher.clone())
            .await
            .unwrap();
    assert_eq!(result.messages, 7);
    let failure = response::<crate::storage::Counts>(
        client.get(format!("{url}/invalid")),
        Endpoint::Freshness,
    )
    .with_subscriber(dispatcher)
    .await;
    assert!(failure.is_err());
    let result: crate::storage::Counts =
        response(client.get(format!("{url}/success")), Endpoint::Freshness)
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
    assert_correlated_spans(&output, wire);
}

fn assert_correlated_spans(output: &str, wire: &str) {
    let closed: Vec<_> = output
        .lines()
        .map(|line| serde_json::from_str::<TraceEvent>(line).unwrap())
        .filter(|event| event.fields.message == "close")
        .collect();
    let spans: Vec<_> = closed
        .iter()
        .filter_map(|event| event.span.as_ref())
        .filter(|span| span.name == "shared_http_request")
        .collect();
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].trace_id.as_deref(), Some(wire));
    assert_ne!(spans[0].trace_id, spans[1].trace_id);
    assert_eq!(spans[0].outcome.as_deref(), Some("success"));
    assert_eq!(spans[1].outcome.as_deref(), Some("error"));
    assert!(spans.iter().all(|span| span.status == Some(200)));
    assert!(spans.iter().all(|span| {
        span.queue_ms.is_some()
            && span.endpoint.as_deref() == Some("freshness")
            && span.admission.as_deref() == Some("interactive")
    }));
    let decoded: Vec<_> = closed
        .iter()
        .filter(|event| {
            event
                .span
                .as_ref()
                .is_some_and(|span| span.name == "shared_http_decode")
        })
        .collect();
    assert_eq!(decoded.len(), 2);
    assert!(decoded.iter().all(|event| {
        event
            .span
            .as_ref()
            .unwrap()
            .bytes
            .is_some_and(|bytes| bytes > 0)
    }));
    assert_eq!(
        decoded[0].span.as_ref().unwrap().outcome.as_deref(),
        Some("success")
    );
    assert_eq!(
        decoded[1].span.as_ref().unwrap().outcome.as_deref(),
        Some("error")
    );
    for (event, request) in decoded.iter().zip(spans) {
        let parent = event
            .spans
            .as_ref()
            .unwrap()
            .iter()
            .find(|span| span.name == "shared_http_request")
            .unwrap();
        assert_eq!(parent.trace_id, request.trace_id);
    }
}
