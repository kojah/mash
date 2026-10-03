//! A held decoder must not prevent unrelated I/O on a single runtime worker.
use super::*;
use serde::{Deserialize, Deserializer};
use std::sync::{Mutex, OnceLock, mpsc};

struct Gate {
    entered: tokio::sync::oneshot::Sender<std::thread::ThreadId>,
    release: mpsc::Receiver<()>,
}

// Used only by this fixture's decoder; ordinary response types never use it.
static GATE: OnceLock<Mutex<Option<Gate>>> = OnceLock::new();

struct HeldDecode;

impl<'de> Deserialize<'de> for HeldDecode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        assert!(bool::deserialize(deserializer)?);
        let gate = GATE.get().unwrap().lock().unwrap().take().unwrap();
        gate.entered.send(std::thread::current().id()).unwrap();
        // A regressed implementation exits rather than hanging the test runtime.
        gate.release
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| serde::de::Error::custom("decoder was not released"))?;
        Ok(Self)
    }
}

#[tokio::test(flavor = "current_thread")]
async fn busy_response_decoder_does_not_block_another_shared_read() {
    use axum::{Json, Router, routing::get};

    let (entered, receiver) = tokio::sync::oneshot::channel();
    let (release, wait) = mpsc::channel();
    GATE.set(Mutex::new(Some(Gate {
        entered,
        release: wait,
    })))
    .unwrap_or_else(|_| panic!("decoder fixture must run once"));
    let app = Router::new()
        .route("/busy", get(|| async { Json(true) }))
        .route(
            "/healthy",
            get(|| async {
                Json(crate::storage::Counts {
                    conversations: 2,
                    messages: 5,
                    searchable_messages: 4,
                    embeddings: 4,
                })
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let runtime_thread = std::thread::current().id();
    let busy = tokio::spawn(async move {
        response::<HeldDecode>(
            reqwest::Client::new().get(format!("http://{address}/busy")),
            Endpoint::Freshness,
        )
        .await
    });
    let decoder_thread = tokio::time::timeout(Duration::from_secs(1), receiver)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(decoder_thread, runtime_thread);
    assert!(!busy.is_finished());
    let healthy = tokio::time::timeout(
        Duration::from_secs(1),
        response::<crate::storage::Counts>(
            reqwest::Client::new().get(format!("http://{address}/healthy")),
            Endpoint::Freshness,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(healthy.messages, 5);
    assert_eq!(healthy.searchable_messages, 4);
    assert!(!busy.is_finished(), "the first decoder must still be held");
    release.send(()).unwrap();
    assert!(busy.await.unwrap().is_ok());
    server.abort();
}
