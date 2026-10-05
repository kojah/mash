use super::*;
use crate::provider::Provider;
use crate::shared::client::{Admission, response_admitted};
use crate::shared::protocol::{Checkpoint, NodeState, Tombstone};
use axum::{Router, body::Body, response::Response, routing::get};
use serde::{Deserialize, Deserializer};
use std::{convert::Infallible, sync::Mutex};

mod control;

struct Service {
    url: String,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Service {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(router: Router) -> Service {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    Service { url, task }
}

fn chunked(bytes: Bytes) -> Body {
    let frames = (0..bytes.len()).step_by(4093).map(move |offset| {
        Ok::<_, Infallible>(bytes.slice(offset..(offset + 4093).min(bytes.len())))
    });
    Body::from_stream(stream::iter(frames))
}

fn state(records: usize) -> NodeState {
    NodeState {
        publication_version: "fixture-head-version".into(),
        gold_build: crate::gold::config::Build::default(),
        silver_coverage: Vec::new(),
        silver_revision: None,
        silver_scans: Vec::new(),
        source_silver: None,
        instance: Some("unicode-instance-🧪".into()),
        revision: i64::MAX,
        sources: (0..records)
            .map(|ordinal| Checkpoint {
                provider: Provider::Codex,
                path_hash: format!("{ordinal:064x}"),
                size_bytes: i64::try_from(ordinal).unwrap(),
                modified_ns: i64::MAX - i64::try_from(ordinal).unwrap(),
                fingerprint: format!("{ordinal:064x}"),
                interpretation_version: crate::silver::certificate::INTERPRETATION_VERSION,
                conversation_id: Some(format!("thread-🧪-{ordinal}-{}", "payload ".repeat(64))),
                ready: ordinal % 2 == 0,
            })
            .collect(),
        tombstones: vec![Tombstone {
            provider: Provider::ClaudeCode,
            conversation_id: "deleted-🧪".into(),
        }],
    }
}

#[tokio::test]
async fn chunked_preview_boundary_preserves_data_and_selects_decode_mode() {
    use tracing::instrument::WithSubscriber;
    #[derive(Deserialize)]
    struct Event {
        span: Option<Span>,
    }
    #[derive(Deserialize)]
    struct Span {
        name: String,
        mode: Option<String>,
    }
    static CAPACITY: Admission = Admission::new(1, 1);
    for (endpoint, length, mode) in [
        (Endpoint::Freshness, PREVIEW_BYTES, "buffered"),
        (Endpoint::Freshness, PREVIEW_BYTES + 1, "streamed"),
        (Endpoint::Search, SEARCH_PREVIEW_BYTES, "buffered"),
        (Endpoint::Search, SEARCH_PREVIEW_BYTES + 1, "streamed"),
    ] {
        let expected = "x".repeat(length - 2);
        let bytes = Bytes::from(serde_json::to_vec(&expected).unwrap());
        assert_eq!(bytes.len(), length);
        let service =
            serve(Router::new().route("/", get(move || async { Response::new(chunked(bytes)) })))
                .await;
        let (trace, subscriber) = crate::observability::test_support::capture();
        let actual: String = response_admitted(
            reqwest::Client::new().get(format!("{}/", service.url)),
            endpoint,
            &CAPACITY,
        )
        .with_subscriber(subscriber)
        .await
        .unwrap();
        assert_eq!(actual, expected);
        let output = String::from_utf8(trace.lock().unwrap().clone()).unwrap();
        let modes: Vec<_> = output
            .lines()
            .map(|line| serde_json::from_str::<Event>(line).unwrap())
            .filter_map(|event| event.span)
            .filter(|span| span.name == "shared_http_decode")
            .filter_map(|span| span.mode)
            .collect();
        assert_ne!(modes.len(), 0);
        assert!(modes.iter().all(|value| value == mode));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn complete_large_state_matches_typed_records_across_header_and_chunked_modes() {
    static CAPACITY: Admission = Admission::new(1, 1);
    let expected = state(2048);
    let bytes = Bytes::from(serde_json::to_vec(&expected).unwrap());
    assert!(bytes.len() > PREVIEW_BYTES);
    let fixed = bytes.clone();
    let router = Router::new()
        .route(
            "/fixed",
            get(move || async { Response::new(Body::from(fixed)) }),
        )
        .route(
            "/chunked",
            get(move || async { Response::new(chunked(bytes)) }),
        )
        .layer(
            tower_http::compression::CompressionLayer::new()
                .quality(tower_http::CompressionLevel::Fastest),
        );
    let service = serve(router).await;
    for (path, endpoint, gzip) in [
        ("fixed", Endpoint::State, false),
        ("fixed", Endpoint::Freshness, false),
        ("chunked", Endpoint::Freshness, false),
        ("fixed", Endpoint::State, true),
        ("fixed", Endpoint::Freshness, true),
        ("chunked", Endpoint::Freshness, true),
        ("fixed", Endpoint::Search, false),
        ("chunked", Endpoint::Search, false),
        ("fixed", Endpoint::Search, true),
        ("chunked", Endpoint::Search, true),
    ] {
        // Inspect negotiation without automatic decompression, then decode
        // through the real pooled shared client (or an older plain client).
        let wire = reqwest::Client::builder()
            .no_gzip()
            .build()
            .unwrap()
            .get(format!("{}/{path}", service.url))
            .header("accept-encoding", if gzip { "gzip" } else { "identity" })
            .send()
            .await
            .unwrap();
        if gzip {
            assert_eq!(wire.headers()["content-encoding"], "gzip");
            assert!(wire.bytes().await.unwrap().len() < PREVIEW_BYTES);
        } else {
            assert!(wire.headers().get("content-encoding").is_none());
            assert!(wire.bytes().await.unwrap().len() > PREVIEW_BYTES);
        }
        let http = if gzip {
            crate::shared::client::HTTP_CLIENT.as_ref().unwrap().clone()
        } else {
            reqwest::Client::builder().no_gzip().build().unwrap()
        };
        let actual = response_admitted::<NodeState>(
            http.get(format!("{}/{path}", service.url)),
            endpoint,
            &CAPACITY,
        )
        .await
        .unwrap();
        assert_eq!(actual, expected);
    }
}

static ENTERED: Mutex<Option<tokio::sync::oneshot::Sender<()>>> = Mutex::new(None);
struct Prefix;
impl<'de> Deserialize<'de> for Prefix {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        assert!(bool::deserialize(deserializer)?);
        ENTERED.lock().unwrap().take().unwrap().send(()).unwrap();
        Ok(Self)
    }
}

#[tokio::test(flavor = "current_thread")]
async fn state_decode_starts_before_eof_and_cancellation_wakes_blocked_io() {
    static CAPACITY: Admission = Admission::new(1, 1);
    let (entered, started) = tokio::sync::oneshot::channel();
    *ENTERED.lock().unwrap() = Some(entered);
    let (mut tail, receiver) = tokio::sync::oneshot::channel::<()>();
    let receiver = std::sync::Arc::new(Mutex::new(Some(receiver)));
    let service = serve(Router::new().route(
        "/",
        get(move || {
            let receiver = receiver.lock().unwrap().take().unwrap();
            async move {
                let frames =
                    stream::once(async { Ok::<_, Infallible>(Bytes::from_static(b"true ")) })
                        .chain(stream::once(async move {
                            let _ = receiver.await;
                            Ok(Bytes::from_static(b" "))
                        }));
                Response::new(Body::from_stream(frames))
            }
        }),
    ))
    .await;
    let request = reqwest::Client::new().get(format!("{}/", service.url));
    let pending = tokio::spawn(async move {
        response_admitted::<Prefix>(request, Endpoint::State, &CAPACITY).await
    });
    tokio::time::timeout(std::time::Duration::from_secs(1), started)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !pending.is_finished(),
        "complete response still requires EOF"
    );
    assert_eq!(CAPACITY.bulk.available_permits(), 0);
    pending.abort();
    assert!(pending.await.err().unwrap().is_cancelled());
    let returned = tokio::time::timeout(std::time::Duration::from_secs(1), CAPACITY.bulk.acquire())
        .await
        .unwrap()
        .unwrap();
    drop(returned);
    // The reader and upstream body must be gone, not just the waiting caller.
    tokio::time::timeout(std::time::Duration::from_secs(1), tail.closed())
        .await
        .unwrap();
}

#[tokio::test]
async fn malformed_trailing_and_truncated_state_never_return_partial_data() {
    static CAPACITY: Admission = Admission::new(1, 1);
    let valid = serde_json::to_vec(&state(2)).unwrap();
    let truncated = Bytes::copy_from_slice(&valid[..valid.len() - 1]);
    let mut trailing = valid;
    trailing.extend_from_slice(b" private-body-canary");
    let trailing = Bytes::from(trailing);
    let service = serve(
        Router::new()
            .route(
                "/truncated",
                get(move || async { Response::new(chunked(truncated)) }),
            )
            .route(
                "/trailing",
                get(move || async { Response::new(chunked(trailing)) }),
            )
            .route(
                "/schema",
                get(|| async {
                    Response::new(Body::from("{\"revision\":\"private-body-canary\"}"))
                }),
            ),
    )
    .await;
    for path in ["truncated", "trailing", "schema"] {
        let error = response_admitted::<NodeState>(
            reqwest::Client::new().get(format!("{}/{path}", service.url)),
            Endpoint::State,
            &CAPACITY,
        )
        .await
        .unwrap_err();
        assert_eq!(error.error.kind, crate::ErrorKind::SharedIndex);
        assert_eq!(error.error.message, "invalid shared index HTTP response");
        assert_eq!(CAPACITY.bulk.available_permits(), 1);
    }
}

#[tokio::test]
async fn transport_failure_after_valid_json_is_not_accepted_as_complete_state() {
    static CAPACITY: Admission = Admission::new(1, 1);
    let bytes = Bytes::from(serde_json::to_vec(&state(1)).unwrap());
    let service = serve(Router::new().route(
        "/",
        get(move || async {
            let frames = stream::once(async move { Ok(bytes) }).chain(stream::once(async {
                tokio::task::yield_now().await;
                Err::<Bytes, _>(io::Error::other("private-transport-canary"))
            }));
            Response::new(Body::from_stream(frames))
        }),
    ))
    .await;
    let error = response_admitted::<NodeState>(
        reqwest::Client::new().get(format!("{}/", service.url)),
        Endpoint::State,
        &CAPACITY,
    )
    .await
    .unwrap_err();
    assert_eq!(error.error.kind, crate::ErrorKind::SharedIndex);
    assert!(!error.error.message.contains("private-transport-canary"));
    assert_eq!(CAPACITY.bulk.available_permits(), 1);
}
