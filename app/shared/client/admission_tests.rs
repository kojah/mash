//! Controlled admission and decoder lifecycle checks, independent of global slots.
use super::*;
use crate::test_support::HANG_GUARD;
use serde::{Deserialize, Deserializer};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};

struct Server {
    url: String,
    received: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn server() -> Server {
    use axum::{Json, Router, extract::State, routing::get};
    let received = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/",
            get(|State(received): State<Arc<AtomicUsize>>| async move {
                received.fetch_add(1, Ordering::SeqCst);
                Json(true)
            }),
        )
        .with_state(received.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Server {
        url,
        received,
        task,
    }
}

#[tokio::test]
async fn fleet_catalog_can_finish_after_short_interactive_deadline() {
    static CAPACITY: Admission = Admission::new(1, 1);
    let app = axum::Router::new().route(
        "/",
        axum::routing::get(|| async {
            tokio::time::sleep(Duration::from_millis(2100)).await;
            axum::Json(true)
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let response = response_admitted::<bool>(
        reqwest::Client::new().get(url),
        Endpoint::Catalog,
        &CAPACITY,
    )
    .await;
    task.abort();
    assert!(response.unwrap());
}

#[tokio::test]
async fn admission_deadline_expires_without_sending_or_exposing_request_metadata() {
    static CAPACITY: Admission = Admission::new(0, 1);
    let server = server().await;
    let error = response_admitted::<bool>(
        reqwest::Client::new()
            .get(format!("{}?private-query-canary", server.url))
            .bearer_auth("credential-canary"),
        Endpoint::Freshness,
        &CAPACITY,
    )
    .await
    .unwrap_err();
    assert_eq!(server.received.load(Ordering::SeqCst), 0);
    assert_eq!(error.error.kind, crate::ErrorKind::SharedIndex);
    assert_eq!(error.error.message, "shared index HTTP request timed out");
    assert!(error.error.retryable);
}

#[tokio::test]
async fn exhausted_bulk_admission_does_not_delay_interactive_reads() {
    static CAPACITY: Admission = Admission::new(1, 0);
    let server = server().await;
    assert!(Endpoint::Index.capacity(&CAPACITY).try_acquire().is_err());
    let result = tokio::time::timeout(
        HANG_GUARD,
        response_admitted::<bool>(
            reqwest::Client::new().get(&server.url),
            Endpoint::Freshness,
            &CAPACITY,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(result);
    assert_eq!(server.received.load(Ordering::SeqCst), 1);
}

struct Gate {
    entered: tokio::sync::oneshot::Sender<()>,
    release: mpsc::Receiver<()>,
}
static GATE: Mutex<Option<Gate>> = Mutex::new(None);

struct HeldDecode;
impl<'de> Deserialize<'de> for HeldDecode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        assert!(bool::deserialize(deserializer)?);
        let gate = GATE.lock().unwrap().take().unwrap();
        gate.entered.send(()).unwrap();
        // Only the test's explicit release ends the decode. The hang guard must
        // outlast the request deadline so the deadline case cannot observe this
        // decoder failing instead of the deadline expiring under load.
        gate.release
            .recv_timeout(HANG_GUARD)
            .map_err(|_| serde::de::Error::custom("decoder was not released"))?;
        Ok(Self)
    }
}

enum EndRequest {
    Cancel,
    Deadline,
}

#[tokio::test(flavor = "current_thread")]
async fn canceled_or_expired_decoder_keeps_capacity_until_actual_completion() {
    static CAPACITY: Admission = Admission::new(1, 1);
    let server = server().await;
    for end in [EndRequest::Cancel, EndRequest::Deadline] {
        let (entered, started) = tokio::sync::oneshot::channel();
        let (release, waiting) = mpsc::channel();
        *GATE.lock().unwrap() = Some(Gate {
            entered,
            release: waiting,
        });
        let request = reqwest::Client::new().get(&server.url);
        let busy = tokio::spawn(async move {
            response_admitted::<HeldDecode>(request, Endpoint::Freshness, &CAPACITY).await
        });
        tokio::time::timeout(HANG_GUARD, started)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(CAPACITY.interactive.available_permits(), 0);
        match end {
            EndRequest::Cancel => {
                busy.abort();
                assert!(busy.await.err().unwrap().is_cancelled());
            }
            EndRequest::Deadline => {
                // The production request deadline expires while the decoder is
                // still held; it is far shorter than the decoder's hang guard.
                assert!(Endpoint::Freshness.timeout() < HANG_GUARD / 2);
                let error = busy.await.unwrap().err().unwrap();
                assert_eq!(error.error.message, "shared index HTTP request timed out");
            }
        }
        // Returning from the request is not proof the blocking task finished.
        assert_eq!(CAPACITY.interactive.available_permits(), 0);
        release.send(()).unwrap();
        let returned = tokio::time::timeout(HANG_GUARD, CAPACITY.interactive.acquire())
            .await
            .unwrap()
            .unwrap();
        drop(returned);
        assert!(
            response_admitted::<bool>(
                reqwest::Client::new().get(&server.url),
                Endpoint::Freshness,
                &CAPACITY,
            )
            .await
            .unwrap()
        );
    }
    assert_eq!(server.received.load(Ordering::SeqCst), 4);
}
