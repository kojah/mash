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

fn traced_router(dispatcher: tracing::Dispatch) -> Router {
    let app = App {
        store: Store::unavailable(),
        identities: Arc::new(HashMap::from([(
            blake3::hash(FIRST.as_bytes()),
            Identity {
                group_id: "group-canary".into(),
                node_id: "node-canary".into(),
                generation: "test".into(),
                dimensions: 2,
            },
        )])),
        uploads: Arc::new(tokio::sync::Semaphore::new(1)),
    };
    router(app).layer(middleware::from_fn(move |request: Request, next: Next| {
        next.run(request).with_subscriber(dispatcher.clone())
    }))
}

// Real authenticated routing and real client transport, without requiring a database.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_http_calls_correlate_failures_and_bound_untrusted_metadata() {
    let (bytes, dispatcher) = crate::observability::test_support::capture();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/missing?query-canary",
        listener.local_addr().unwrap()
    );
    let app = traced_router(dispatcher.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::new();
    let call = |token| {
        super::super::super::client::response::<crate::storage::Counts>(
            client.get(&url).bearer_auth(token),
            super::super::super::client::Endpoint::Freshness,
        )
        .with_subscriber(dispatcher.clone())
    };
    let (authorized, unauthorized) = tokio::join!(call(FIRST), call("credential-canary"));
    assert!(authorized.is_err());
    assert!(unauthorized.is_err());

    // Disabled client tracing sends no ID: the tracing server creates its own.
    assert!(
        super::super::super::client::response::<crate::storage::Counts>(
            client.get(&url).bearer_auth(FIRST),
            super::super::super::client::Endpoint::Freshness,
        )
        .with_subscriber(tracing::subscriber::NoSubscriber::default())
        .await
        .is_err()
    );
    for value in [
        "header-secret-canary".to_string(),
        "f".repeat(4096),
        "A".repeat(32),
    ] {
        assert_eq!(
            client
                .get(&url)
                .bearer_auth(FIRST)
                .header(HEADER, value)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
    }
    let duplicate = "1".repeat(32);
    assert_eq!(
        client
            .get(&url)
            .bearer_auth(FIRST)
            .header(HEADER, &duplicate)
            .header(HEADER, &duplicate)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    server.abort();

    let output = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    assert_correlated_traces(&output, &duplicate);
}

fn assert_correlated_traces(output: &str, duplicate: &str) {
    for secret in [
        FIRST,
        "credential-canary",
        "query-canary",
        "header-secret-canary",
        "group-canary",
        "node-canary",
        duplicate,
    ] {
        assert!(!output.contains(secret), "leaked {secret}");
    }
    let spans: Vec<TraceSpan> = output
        .lines()
        .map(|line| serde_json::from_str::<TraceEvent>(line).unwrap())
        .filter(|event| event.fields.message == "close")
        .filter_map(|event| event.span)
        .collect();
    let clients: Vec<_> = spans
        .iter()
        .filter(|span| span.name == "shared_http_request")
        .collect();
    let servers: Vec<_> = spans
        .iter()
        .filter(|span| span.name == "http_request")
        .collect();
    assert_eq!(clients.len(), 2);
    assert_eq!(servers.len(), 7);
    assert_ne!(clients[0].trace_id, clients[1].trace_id);
    let ids: HashSet<_> = servers
        .iter()
        .map(|span| {
            let id = span.trace_id.as_deref().unwrap();
            assert!(TraceId::parse(id.as_bytes()).is_some());
            id
        })
        .collect();
    assert_eq!(ids.len(), servers.len());
    for client in clients {
        assert_eq!(client.outcome.as_deref(), Some("error"));
        let peer = servers
            .iter()
            .find(|server| server.trace_id == client.trace_id)
            .unwrap();
        assert_eq!(peer.status, client.status);
        assert!(matches!(client.status, Some(401 | 404)));
    }
}
