//! Controlled late CAS completion exercises the real HTTP/publication/read paths.
use super::*;
use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::post};
use serde_json::value::{RawValue, to_raw_value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy)]
enum Outcome {
    DelayedFailure,
    DelayedAcknowledgment,
    UnreadableVerification,
    AppliedFailure,
    CompletedLoser,
}

#[derive(Clone, Serialize, Deserialize)]
struct MockPoint {
    id: String,
    payload: Box<RawValue>,
}
#[derive(Deserialize)]
struct Incoming {
    points: Vec<MockPoint>,
}
#[derive(Deserialize)]
struct Retrieve {
    ids: Vec<String>,
}
#[derive(Serialize)]
struct Reply<T> {
    status: &'static str,
    result: T,
}
#[derive(Serialize)]
struct Operation {
    status: &'static str,
}
#[derive(Deserialize)]
struct Delete {
    filter: Filter,
}
#[derive(Deserialize)]
struct Filter {
    must: Vec<Match>,
}
#[derive(Deserialize)]
struct Match {
    key: String,
    r#match: Values,
}
#[derive(Deserialize)]
struct Values {
    any: Vec<String>,
}
#[derive(Deserialize)]
struct Version {
    version: String,
}

struct Backend {
    outcome: Outcome,
    points: BTreeMap<String, MockPoint>,
    queued: Option<MockPoint>,
    deletes: usize,
}
type Shared = Arc<Mutex<Backend>>;

fn completed() -> axum::response::Response {
    Json(Reply {
        status: "ok",
        result: Operation {
            status: "completed",
        },
    })
    .into_response()
}

async fn upsert(
    State(shared): State<Shared>,
    Json(input): Json<Incoming>,
) -> axum::response::Response {
    let mut backend = shared.lock().unwrap();
    for point in input.points {
        let Ok(node) = serde_json::from_str::<NodeHead>(point.payload.get()) else {
            backend.points.insert(point.id.clone(), point);
            continue;
        };
        match backend.outcome {
            Outcome::DelayedFailure
            | Outcome::DelayedAcknowledgment
            | Outcome::UnreadableVerification => {
                backend.queued = Some(point);
                return if matches!(backend.outcome, Outcome::DelayedAcknowledgment) {
                    Json(Reply {
                        status: "ok",
                        result: Operation {
                            status: "acknowledged",
                        },
                    })
                    .into_response()
                } else {
                    StatusCode::SERVICE_UNAVAILABLE.into_response()
                };
            }
            Outcome::AppliedFailure => {
                backend.points.insert(point.id.clone(), point);
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
            Outcome::CompletedLoser => {
                // Another writer inserted a complete snapshot before this insert-only
                // operation completed without replacing its head. Retain real chunks.
                let mut winner = node;
                let version = winner.version.clone();
                winner.version = "concurrent-winner".into();
                winner
                    .head
                    .as_mut()
                    .unwrap()
                    .version
                    .clone_from(&winner.version);
                for index in 0..winner.manifest.as_ref().unwrap().chunks {
                    let stored =
                        &backend.points[&point_id(&["chunk", &version, &index.to_string()])];
                    let mut chunk: Chunk = serde_json::from_str(stored.payload.get()).unwrap();
                    chunk.version.clone_from(&winner.version);
                    let id = point_id(&["chunk", &winner.version, &index.to_string()]);
                    backend.points.insert(
                        id.clone(),
                        MockPoint {
                            id,
                            payload: to_raw_value(&chunk).unwrap(),
                        },
                    );
                }
                backend.points.insert(
                    point.id.clone(),
                    MockPoint {
                        id: point.id,
                        payload: to_raw_value(&winner).unwrap(),
                    },
                );
            }
        }
    }
    completed()
}

async fn retrieve(
    State(shared): State<Shared>,
    Json(input): Json<Retrieve>,
) -> axum::response::Response {
    let backend = shared.lock().unwrap();
    if matches!(backend.outcome, Outcome::UnreadableVerification) && backend.queued.is_some() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    Json(Reply {
        status: "ok",
        result: input
            .ids
            .into_iter()
            .filter_map(|id| backend.points.get(&id).cloned())
            .collect::<Vec<_>>(),
    })
    .into_response()
}

async fn delete(
    State(shared): State<Shared>,
    Json(input): Json<Delete>,
) -> axum::response::Response {
    assert_eq!(input.filter.must.len(), 1);
    assert_eq!(input.filter.must[0].key, "version");
    let versions = &input.filter.must[0].r#match.any;
    let mut backend = shared.lock().unwrap();
    backend.deletes += 1;
    backend.points.retain(|_, point| {
        let version: Version = serde_json::from_str(point.payload.get()).unwrap();
        !versions.contains(&version.version)
    });
    completed()
}

async fn exercise(outcome: Outcome) {
    let shared = Arc::new(Mutex::new(Backend {
        outcome,
        points: BTreeMap::new(),
        queued: None,
        deletes: 0,
    }));
    let router = Router::new()
        .route("/collections/{name}/points", post(retrieve).put(upsert))
        .route("/collections/{name}/points/delete", post(delete))
        .with_state(shared.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let store = Store {
        api: api::Client::new(&url, None, 1024 * 1024).unwrap(),
        max_node_bytes: 1024 * 1024,
        vector_index: VectorIndex::default(),
        snapshot_budget: crate::shared::snapshot_budget::Budget::default(),
    };
    let identity = Identity {
        group_id: "group".into(),
        node_id: "node".into(),
        generation: "generation".into(),
        dimensions: 4,
    };
    let plan = crate::shared::snapshot::Plan {
        loaded: crate::shared::snapshot::PendingSnapshot {
            payload: None,
            head: None,
            expected: String::new(),
        }
        .decode()
        .unwrap(),
        revision: 1,
        changed: true,
        removed: 0,
        purged: 0,
    };
    let result = store.commit(&identity, plan, "instance".into(), true).await;
    if matches!(outcome, Outcome::AppliedFailure) {
        assert_eq!(result.unwrap().revision, 1);
    } else {
        assert!(result.is_err());
    }
    {
        let mut backend = shared.lock().unwrap();
        if matches!(outcome, Outcome::CompletedLoser) {
            assert_eq!(
                backend.deletes, 2,
                "confirmed losing CAS can reclaim staging"
            );
        } else {
            assert_eq!(
                backend.deletes, 0,
                "ambiguous CAS must retain its staged snapshot"
            );
        }
        if let Some(point) = backend.queued.take() {
            backend.points.insert(point.id.clone(), point);
        }
    }
    let loaded = store
        .load(&identity)
        .await
        .expect("late or winning publication must retain complete verifiable chunks");
    assert_eq!(loaded.head.unwrap().revision, 1);
    server.abort();
}

#[tokio::test]
async fn ambiguous_cas_can_publish_after_the_failed_caller_without_losing_chunks() {
    exercise(Outcome::DelayedFailure).await;
    exercise(Outcome::DelayedAcknowledgment).await;
    exercise(Outcome::UnreadableVerification).await;
}

#[tokio::test]
async fn observed_publication_recovers_success_and_confirmed_loser_cleanup_preserves_winner() {
    exercise(Outcome::AppliedFailure).await;
    exercise(Outcome::CompletedLoser).await;
}
