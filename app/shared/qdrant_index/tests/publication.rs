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
    StagingFailure,
    DelayedFailure,
    DelayedAcknowledgment,
    UnreadableVerification,
    AppliedFailure,
    CompletedLoser,
    CleanupFailure,
    ActivatedDuringPublication,
}

#[derive(Clone, Serialize, Deserialize)]
struct MockPoint {
    id: String,
    payload: Box<RawValue>,
}
#[derive(Deserialize)]
struct Incoming {
    points: Vec<MockPoint>,
    update_filter: Option<Filter>,
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
#[derive(Clone, Deserialize)]
struct Filter {
    must: Vec<Match>,
}
#[derive(Clone, Deserialize)]
struct Match {
    key: String,
    r#match: Values,
}
#[derive(Clone, Deserialize)]
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
    queued: Option<Pending>,
    deletes: usize,
}
struct Pending {
    point: MockPoint,
    filter: Option<Filter>,
}
fn permitted(backend: &Backend, point: &MockPoint, filter: Option<&Filter>) -> bool {
    filter.is_none_or(|filter| {
        let Some(current) = backend.points.get(&point.id) else {
            return false;
        };
        let current: super::super::configuration::Configuration =
            serde_json::from_str(current.payload.get()).unwrap();
        filter.must.iter().all(|condition| {
            let value = match condition.key.as_str() {
                "version" => current.version.as_str(),
                "publication" => current.publication.as_deref().unwrap(),
                _ => panic!("unexpected conditional field"),
            };
            condition
                .r#match
                .any
                .iter()
                .any(|expected| expected == value)
        })
    })
}
type Shared = Arc<Mutex<Backend>>;
mod diagnostics;

fn completed() -> axum::response::Response {
    Json(Reply {
        status: "ok",
        result: Operation {
            status: "completed",
        },
    })
    .into_response()
}

fn activate_title(backend: &mut Backend) {
    let id = point_id(&["gold-configuration"]);
    let mut control: super::super::configuration::Configuration =
        serde_json::from_str(backend.points[&id].payload.get()).unwrap();
    control.version = "b".repeat(64);
    control.publication = Some("c".repeat(64));
    control.build.embedding_text = crate::gold::config::EmbeddingText::TitleAndDialogue;
    backend.points.insert(
        id.clone(),
        MockPoint {
            id,
            payload: to_raw_value(&control).unwrap(),
        },
    );
}

async fn upsert(
    State(shared): State<Shared>,
    Json(input): Json<Incoming>,
) -> axum::response::Response {
    let mut backend = shared.lock().unwrap();
    for point in input.points {
        let group =
            serde_json::from_str::<super::super::configuration::Configuration>(point.payload.get())
                .ok()
                .filter(|group| group.kind == PointKind::Configuration);
        let Some(mut group) = group else {
            if matches!(backend.outcome, Outcome::StagingFailure) {
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
            backend.points.insert(point.id.clone(), point);
            continue;
        };
        if !permitted(&backend, &point, input.update_filter.as_ref()) {
            continue;
        }
        let node = group
            .nodes
            .as_ref()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .clone();
        match backend.outcome {
            Outcome::StagingFailure => unreachable!("staging failed before publishing a head"),
            Outcome::DelayedFailure
            | Outcome::DelayedAcknowledgment
            | Outcome::UnreadableVerification => {
                backend.queued = Some(Pending {
                    point,
                    filter: input.update_filter.clone(),
                });
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
            Outcome::ActivatedDuringPublication => {
                backend.points.insert(point.id.clone(), point);
                activate_title(&mut backend);
            }
            Outcome::CompletedLoser | Outcome::CleanupFailure => {
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
                group
                    .nodes
                    .as_mut()
                    .unwrap()
                    .insert(winner.node_id.clone(), winner);
                group.publication = Some("e".repeat(64));
                backend.points.insert(
                    point.id.clone(),
                    MockPoint {
                        id: point.id,
                        payload: to_raw_value(&group).unwrap(),
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
    if matches!(backend.outcome, Outcome::CleanupFailure) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    backend.points.retain(|_, point| {
        let version: Version = serde_json::from_str(point.payload.get()).unwrap();
        !versions.contains(&version.version)
    });
    completed()
}

fn seed_authority(shared: &Shared, identity: &Identity) {
    let id = point_id(&["gold-configuration"]);
    shared.lock().unwrap().points.insert(
        id.clone(),
        MockPoint {
            id,
            payload: to_raw_value(&serde_json::json!({
                "kind":"configuration", "schema_version":2, "version":"a".repeat(64),
                "publication":"d".repeat(64), "phase":"ready", "nodes":{},
                "build":crate::gold::config::Build::default(),
                "generation":identity.generation, "dimensions":4
            }))
            .unwrap(),
        },
    );
}

async fn start(outcome: Outcome) -> (Fixture, Shared, tokio::task::JoinHandle<()>) {
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
        build: crate::gold::config::Build::default(),
        api: api::Client::new(&url, None, 1024 * 1024).unwrap(),
        max_node_bytes: 1024 * 1024,
        vector_index: VectorIndex::default(),
        snapshot_budget: crate::shared::snapshot_budget::Budget::default(),
    };
    let identity = Identity {
        group_id: "private-group-canary".into(),
        node_id: "private-node-canary".into(),
        generation: crate::semantic::model_space("test", 4),
        dimensions: 4,
    };
    let fixture = Fixture {
        store,
        identity,
        url,
    };
    seed_authority(&shared, &fixture.identity);
    (fixture, shared, server)
}

async fn exercise(outcome: Outcome) {
    let (mut fixture, shared, server) = start(outcome).await;
    let plan = crate::shared::snapshot::plan(
        &fixture.file(1, "private-history-canary"),
        crate::shared::snapshot::PendingSnapshot {
            payload: None,
            head: None,
            expected: String::new(),
        }
        .decode()
        .unwrap(),
        &fixture.identity,
        &crate::gold::config::Build::default(),
    )
    .unwrap();
    let result = diagnostics::commit(&fixture.store, &fixture.identity, plan, outcome).await;
    if matches!(outcome, Outcome::AppliedFailure) {
        assert_eq!(result.unwrap().revision, 1);
    } else {
        assert!(result.is_err());
    }
    {
        let mut backend = shared.lock().unwrap();
        if matches!(
            outcome,
            Outcome::CompletedLoser | Outcome::StagingFailure | Outcome::CleanupFailure
        ) {
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
        if let Some(pending) = backend.queued.take()
            && permitted(&backend, &pending.point, pending.filter.as_ref())
        {
            backend
                .points
                .insert(pending.point.id.clone(), pending.point);
        }
    }
    if matches!(outcome, Outcome::ActivatedDuringPublication) {
        assert!(fixture.store.state(&fixture.identity).await.is_err());
        fixture.store = fixture.store.with_build(crate::gold::config::Build {
            max_embedding_chars: None,
            representations: crate::gold::config::Representations::LexicalAndDense,
            text_preparation: crate::gold::config::TextPreparation::Preserve,
            embedding_text: crate::gold::config::EmbeddingText::TitleAndDialogue,
        });
    }
    let loaded = fixture
        .store
        .load(&fixture.identity)
        .await
        .expect("late or winning publication must retain complete verifiable chunks");
    if matches!(outcome, Outcome::StagingFailure) {
        assert!(loaded.head.is_none());
    } else {
        if matches!(outcome, Outcome::ActivatedDuringPublication) {
            let head = loaded.head.as_ref().unwrap();
            assert!(!head.gold_manifest.as_ref().unwrap().matches(
                &fixture.store.build,
                &fixture.identity.generation,
                head.silver_revision.as_ref()
            ));
        }
        assert_eq!(loaded.head.unwrap().revision, 1);
        assert_eq!(loaded.data.messages.len(), 2);
        assert!(
            loaded
                .data
                .messages
                .values()
                .all(|message| message.content.contains("private-history-canary"))
        );
    }
    server.abort();
}

#[tokio::test]
async fn ambiguous_cas_can_publish_after_the_failed_caller_without_losing_chunks() {
    exercise(Outcome::StagingFailure).await;
    exercise(Outcome::DelayedFailure).await;
    exercise(Outcome::DelayedAcknowledgment).await;
    exercise(Outcome::UnreadableVerification).await;
}

#[tokio::test]
async fn observed_publication_recovers_success_and_confirmed_loser_cleanup_preserves_winner() {
    exercise(Outcome::AppliedFailure).await;
    exercise(Outcome::CompletedLoser).await;
    exercise(Outcome::CleanupFailure).await;
}

#[tokio::test]
async fn activation_after_node_cas_rejects_acknowledgement_and_preserves_stale_history() {
    exercise(Outcome::ActivatedDuringPublication).await;
}
