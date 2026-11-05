use super::*;
use axum::{Json, Router, extract::State, routing::get};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct StateData {
    scan: usize,
    indexing: usize,
    reads: usize,
    patches: usize,
}
type Shared = Arc<Mutex<StateData>>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Patch {
    hnsw_config: ScanPatch,
    optimizers_config: IndexPatch,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScanPatch {
    full_scan_threshold: Option<usize>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexPatch {
    indexing_threshold: Option<usize>,
}

async fn fixture(scan: usize, indexing: usize) -> (Client, Shared, tokio::task::JoinHandle<()>) {
    let state = Arc::new(Mutex::new(StateData {
        scan,
        indexing,
        ..StateData::default()
    }));
    let router = Router::new()
        .route(
            "/collections/test",
            get(|State(state): State<Shared>| async move {
                let mut state = state.lock().unwrap();
                state.reads += 1;
                format!(
                    r#"{{"status":"ok","result":{{"config":{{"params":{{"vectors":{{}}}},"hnsw_config":{{"m":16,"ef_construct":200,"full_scan_threshold":{}}},"optimizer_config":{{"indexing_threshold":{}}}}},"payload_schema":{{}}}}}}"#,
                    state.scan, state.indexing,
                )
            })
            .patch(
                |State(state): State<Shared>, Json(patch): Json<Patch>| async move {
                    let mut state = state.lock().unwrap();
                    state.patches += 1;
                    if let Some(scan) = patch.hnsw_config.full_scan_threshold {
                        state.scan = scan;
                    }
                    if let Some(indexing) = patch.optimizers_config.indexing_threshold {
                        state.indexing = indexing;
                    }
                    r#"{"status":"ok","result":true}"#
                },
            ),
        )
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::new(
        &format!("http://{}", listener.local_addr().unwrap()),
        None,
        4096,
    )
    .unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (client, state, server)
}

#[tokio::test]
async fn legacy_planner_needs_explicit_activation_even_when_policy_already_says_hnsw() {
    let (client, state, server) = fixture(10_000, 10_000).await;
    let error = client
        .prepare_query("test", VectorIndex::default(), false)
        .await
        .unwrap_err();
    assert_eq!(error.error.kind, crate::ErrorKind::Configuration);
    assert!(error.error.message.contains("--activate-query"));
    assert_eq!(state.lock().unwrap().patches, 0);
    client
        .prepare_query("test", VectorIndex::default(), true)
        .await
        .unwrap();
    client
        .prepare_query("test", VectorIndex::default(), false)
        .await
        .unwrap();
    let state = state.lock().unwrap();
    assert_eq!(state.patches, 1, "repeated startup does not rebuild graphs");
    assert_eq!(state.scan, FULL_SCAN_KIB);
    assert_eq!(state.indexing, INDEXING_KIB);
    server.abort();
}

#[tokio::test]
async fn activation_preserves_stricter_thresholds_and_reenables_disabled_indexes() {
    for (scan, indexing, expected_scan, expected_indexing) in [
        (10, 0, 10, 1024),
        (10_000, 512, 10, 512),
        (10, 512, 10, 512),
    ] {
        let (client, state, server) = fixture(scan, indexing).await;
        client
            .prepare_query("test", VectorIndex::default(), true)
            .await
            .unwrap();
        let state = state.lock().unwrap();
        assert_eq!(
            (state.scan, state.indexing),
            (expected_scan, expected_indexing)
        );
        assert_eq!(state.patches, usize::from(scan > 10 || indexing == 0));
        server.abort();
    }
}

#[tokio::test]
async fn exact_policy_neither_reads_nor_changes_the_physical_planner() {
    let (client, state, server) = fixture(10_000, 0).await;
    client
        .prepare_query("test", VectorIndex::Exact {}, true)
        .await
        .unwrap();
    let state = state.lock().unwrap();
    assert_eq!((state.reads, state.patches), (0, 0));
    server.abort();
}
