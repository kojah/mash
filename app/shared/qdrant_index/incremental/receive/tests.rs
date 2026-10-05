use super::*;
use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    response::{IntoResponse, Response},
    routing::post,
};
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

#[derive(Clone, Copy)]
enum Mode {
    Normal,
    Duplicate,
    Missing,
    Foreign,
    Kind,
    Version,
    Text,
    Body,
    Digest,
    Stall,
}

struct Captured {
    mode: Mode,
    values: BTreeMap<String, (String, String)>,
    requests: AtomicUsize,
    active: AtomicUsize,
    maximum: AtomicUsize,
    dropped: AtomicUsize,
}

struct Lease(Arc<Captured>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
        self.0.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Deserialize)]
struct Request {
    ids: Vec<String>,
    with_payload: bool,
    with_vector: bool,
}
#[derive(Serialize)]
struct Point {
    id: String,
    payload: Chunk,
}
#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum Status {
    Ok,
}
#[derive(Serialize)]
struct Envelope {
    status: Status,
    result: Vec<Point>,
}

fn point(id: &str, captured: &Captured) -> Point {
    let (version, text) = &captured.values[id];
    Point {
        id: id.into(),
        payload: Chunk {
            kind: PointKind::Chunk,
            version: version.clone(),
            text: text.clone(),
        },
    }
}

async fn receive(
    State(captured): State<Arc<Captured>>,
    axum::Json(request): axum::Json<Request>,
) -> Response {
    assert!(request.with_payload);
    assert!(!request.with_vector);
    let ordinal = captured.requests.fetch_add(1, Ordering::SeqCst);
    let active = captured.active.fetch_add(1, Ordering::SeqCst) + 1;
    captured.maximum.fetch_max(active, Ordering::SeqCst);
    let lease = Lease(captured.clone());
    // Every request waits for the four admitted batches. A sequential loader
    // cannot pass this test, while completed later batches must remain bounded.
    tokio::time::timeout(Duration::from_secs(3), async {
        while captured.requests.load(Ordering::SeqCst) < IN_FLIGHT {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    if matches!(captured.mode, Mode::Stall) {
        let pending = stream::poll_fn(move |_| {
            let _ = &lease;
            std::task::Poll::Pending::<Option<Result<Bytes, std::io::Error>>>
        });
        let prefix = stream::once(async {
            Ok::<_, std::io::Error>(Bytes::from_static(b"{\"status\":\"ok\",\"result\":["))
        });
        return Body::from_stream(prefix.chain(pending)).into_response();
    }
    if ordinal == 0 {
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let mut points: Vec<_> = request
        .ids
        .iter()
        .rev()
        .map(|id| point(id, &captured))
        .collect();
    match captured.mode {
        Mode::Duplicate => points[1] = point(&points[0].id, &captured),
        Mode::Missing => {
            points.pop();
        }
        Mode::Foreign => points[0].id = "unrequested".into(),
        Mode::Kind => points[0].payload.kind = PointKind::Head,
        Mode::Version => points[0].payload.version = "wrong-version".into(),
        Mode::Text => points[0].payload.text.push_str("too long"),
        Mode::Body => points[0].payload.text = "x".repeat(128 * 1024),
        Mode::Digest => {
            points[0].payload.text.replace_range(1..2, "x");
        }
        Mode::Normal | Mode::Stall => {}
    }
    axum::Json(Envelope {
        status: Status::Ok,
        result: points,
    })
    .into_response()
}

struct Fixture {
    store: Store,
    identity: Identity,
    parts: Vec<(String, Manifest)>,
    captured: Arc<Captured>,
    server: tokio::task::JoinHandle<()>,
}

impl Fixture {
    async fn new(mode: Mode) -> Self {
        let mut values = BTreeMap::new();
        let mut parts = Vec::new();
        // Small parts force the 100-record cap, so all four requests must be
        // issued even though the fixture contains only a few KiB of history.
        for index in 0..400 {
            let version = format!("version-{index}");
            let text = serde_json::to_string(&format!("part-{index}-é🦀\n")).unwrap();
            parts.push((
                version.clone(),
                Manifest {
                    bytes: text.len(),
                    chunks: 1,
                    digest: blake3::hash(text.as_bytes()).to_hex().to_string(),
                },
            ));
            values.insert(point_id(&["chunk", &version, "0"]), (version, text));
        }
        let captured = Arc::new(Captured {
            mode,
            values,
            requests: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            maximum: AtomicUsize::new(0),
            dropped: AtomicUsize::new(0),
        });
        let identity = Identity {
            group_id: "prefetch".into(),
            node_id: "node".into(),
            generation: "generation".into(),
            dimensions: 2,
        };
        let router = Router::new()
            .route(
                &format!(
                    "/collections/{}/points",
                    collection(&identity.group_id, true)
                ),
                post(receive),
            )
            .with_state(captured.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = api::Client::new(
            &format!("http://{}", listener.local_addr().unwrap()),
            None,
            64 * 1024 * 1024,
        )
        .unwrap();
        let store = Store {
            api: client,
            dense_names: BTreeMap::new(),
            build: crate::gold::config::Build::default(),
            max_node_bytes: 64 * 1024 * 1024,
            vector_index: super::super::super::VectorIndex::default(),
            snapshot_budget: crate::shared::snapshot_budget::Budget::default(),
        };
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self {
            store,
            identity,
            parts,
            captured,
            server,
        }
    }

    async fn receive(&self) -> Result<Vec<snapshot::payload::Received>, AppError> {
        let parts: Vec<_> = self
            .parts
            .iter()
            .map(|(version, manifest)| (version.as_str(), manifest))
            .collect();
        self.store.receive_parts(&self.identity, &parts).await
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

#[tokio::test]
async fn parallel_snapshot_reads_preserve_order_and_cap_in_flight_batches() {
    let fixture = Fixture::new(Mode::Normal).await;
    let parts = tokio::time::timeout(Duration::from_secs(5), fixture.receive())
        .await
        .unwrap()
        .unwrap();
    let decoded = crate::blocking(move || {
        parts
            .into_iter()
            .map(snapshot::payload::Received::decode::<String>)
            .collect::<Result<Vec<_>, _>>()
    })
    .await
    .unwrap();
    assert_eq!(
        decoded,
        (0..400)
            .map(|index| format!("part-{index}-é🦀\n"))
            .collect::<Vec<_>>()
    );
    assert_eq!(fixture.captured.requests.load(Ordering::SeqCst), 4);
    assert_eq!(fixture.captured.maximum.load(Ordering::SeqCst), IN_FLIGHT);
}

#[tokio::test]
async fn parallel_snapshot_reads_reject_corruption_and_batch_overflow() {
    for mode in [
        Mode::Duplicate,
        Mode::Missing,
        Mode::Foreign,
        Mode::Kind,
        Mode::Version,
        Mode::Text,
        Mode::Body,
        Mode::Digest,
    ] {
        let fixture = Fixture::new(mode).await;
        assert!(
            tokio::time::timeout(Duration::from_secs(5), fixture.receive())
                .await
                .unwrap()
                .is_err()
        );
        assert!(fixture.captured.maximum.load(Ordering::SeqCst) <= IN_FLIGHT);
    }
}

#[tokio::test]
async fn cancelling_snapshot_assembly_drops_pending_http_streams() {
    let fixture = Arc::new(Fixture::new(Mode::Stall).await);
    let owner = fixture.clone();
    let task = tokio::spawn(async move { owner.receive().await });
    tokio::time::timeout(Duration::from_secs(3), async {
        while fixture.captured.requests.load(Ordering::SeqCst) < IN_FLIGHT {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    task.abort();
    let Err(error) = task.await else {
        panic!("cancelled assembly completed unexpectedly");
    };
    assert!(error.is_cancelled());
    // Observe body ownership ending before aborting the mock server, otherwise
    // server teardown could hide orphaned requests from the assembly task.
    tokio::time::timeout(Duration::from_secs(2), async {
        while fixture.captured.dropped.load(Ordering::SeqCst) != IN_FLIGHT {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn snapshot_batches_bound_declared_bytes_and_reject_impossible_manifests() {
    let manifest = Manifest {
        bytes: 4 * CHUNK_BYTES,
        chunks: 4,
        digest: String::new(),
    };
    let packed = batches(&[("version", &manifest)]).unwrap();
    assert_eq!(packed.len(), 2);
    assert!(
        packed
            .iter()
            .all(|batch| batch.bytes == BATCH_BYTES && batch.limits.len() == 2)
    );
    for (bytes, chunks) in [(0, 1), (1, 0), (1, 2)] {
        let manifest = Manifest {
            bytes,
            chunks,
            digest: String::new(),
        };
        assert!(batches(&[("version", &manifest)]).is_err());
    }
}
