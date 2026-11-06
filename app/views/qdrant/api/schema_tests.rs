use super::*;

#[tokio::test]
async fn unavailable_qdrant_names_endpoint_and_repair_without_exposing_api_key() {
    // Hold the port but never serve HTTP: the bounded client times out reliably
    // without racing another process for a released port.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let mut client = Client::new(&url, Some("private-api-key"), 1024).unwrap();
    client.http = reqwest::Client::builder()
        .timeout(Duration::from_millis(25))
        .build()
        .unwrap();
    let startup = client.verify_version().await.unwrap_err();
    let search = client
        .retrieve::<()>("private-collection", &[])
        .await
        .err()
        .expect("an unavailable service must fail the read");
    for error in [startup, search] {
        assert_eq!(error.error.kind, crate::ErrorKind::QdrantUnavailable);
        assert!(error.error.retryable);
        assert!(error.error.message.contains(&url));
        assert!(
            error
                .error
                .message
                .contains("start the configured Qdrant service")
        );
        assert!(error.error.recommended_action.is_none());
        let json = serde_json::to_string(&error).unwrap();
        assert!(!json.contains("private-api-key"));
        assert!(!json.contains("private-collection"));
    }
}

#[test]
fn native_accounting_accepts_optional_and_extensible_typed_response_fields() {
    let bare: Envelope<Vec<u64>> =
        serde_json::from_str(r#"{"result":[42],"status":"ok"}"#).unwrap();
    assert_eq!(bare.result, [42]);
    assert!(bare.time.is_none() && bare.usage.is_none());

    let response: Envelope<Vec<u64>> = serde_json::from_str(
        r#"{"result":[42],"status":"ok","time":0.123,"usage":{"hardware":{
          "cpu":7,"payload_io_read":0,"vector_io_read":1024,"future_counter":1
        },"inference":{"models":{}}}}"#,
    )
    .unwrap();
    assert_eq!(response.result, bare.result);
    assert_eq!(response.time, Some(0.123));
    let hardware = response.usage.unwrap().hardware.unwrap();
    assert_eq!(hardware.cpu, Some(7));
    assert_eq!(hardware.payload_io_read, Some(0));
    assert_eq!(hardware.vector_io_read, Some(1024));
    assert!(hardware.payload_index_io_read.is_none());
    assert!(
        serde_json::from_str::<Envelope<Vec<u64>>>(
            r#"{"result":[42],"status":"ok","usage":{"hardware":{"cpu":"7"}}}"#
        )
        .is_err()
    );
}

#[tokio::test]
async fn missing_views_are_distinct_from_backend_authentication_failures() {
    use axum::{Router, routing::get};
    let router = Router::new()
        .route("/missing", get(|| async { StatusCode::NOT_FOUND }))
        .route("/denied", get(|| async { StatusCode::UNAUTHORIZED }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::new(
        &format!("http://{}", listener.local_addr().unwrap()),
        None,
        1024,
    )
    .unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    for (path, expected) in [
        ("missing", crate::ErrorKind::SearchNotReady),
        ("denied", crate::ErrorKind::QdrantFailure),
    ] {
        let error = client
            .call::<()>(Method::GET, path, None::<&()>)
            .await
            .unwrap_err();
        assert_eq!(error.error.kind, expected);
        if path == "denied" {
            assert!(error.error.message.contains("HTTP 401"));
            assert!(error.error.message.contains("API key"));
        }
    }
    server.abort();
}

#[tokio::test]
async fn qdrant_server_failure_explains_status_without_echoing_private_body() {
    use axum::{Router, routing::get};
    let router = Router::new().route(
        "/",
        get(|| async {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "private-storage-path and secret-token",
            )
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let client = Client::new(&url, None, 1024).unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    for error in [
        client.verify_version().await.unwrap_err(),
        client
            .call::<()>(Method::GET, "", None::<&()>)
            .await
            .unwrap_err(),
    ] {
        assert_eq!(error.error.kind, crate::ErrorKind::QdrantFailure);
        assert!(error.error.message.contains(&url));
        assert!(error.error.message.contains("HTTP 500"));
        assert!(error.error.message.contains("docker logs CONTAINER"));
        assert!(!error.error.message.contains("private-storage-path"));
        assert!(!error.error.message.contains("secret-token"));
    }
    server.abort();
}

#[tokio::test]
async fn rest_reads_keep_their_bounded_deadline() {
    use axum::{Router, routing::post};
    let router = Router::new().route(
        "/collections/test/points",
        post(|| async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            r#"{"status":"ok","result":[]}"#
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client {
        mutations: None,
        http: reqwest::Client::builder()
            .timeout(Duration::from_millis(50))
            .build()
            .unwrap(),
        url: format!("http://{}/", listener.local_addr().unwrap())
            .parse()
            .unwrap(),
        max_response: 1024,
    };
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    assert!(
        client
            .retrieve::<()>("test", &["one".into()])
            .await
            .is_err()
    );
    server.abort();
}

fn legacy() -> serde_json::Value {
    serde_json::json!({
        "config": {
            "params": {
                "vectors": {"dense": {"size":4, "distance":"Cosine"}},
                "sparse_vectors": {"lexical": {"modifier":"idf", "index":{"memory":"cached"}}}
            },
            "hnsw_config": {"m":16, "ef_construct":200, "on_disk":false}
        },
        "payload_schema": {}
    })
}

#[test]
fn dimension_fields_preserve_the_original_schema_and_reject_foreign_shapes() {
    let mut document = legacy();
    document["config"]["params"]["vectors"]["dense_8"] =
        serde_json::json!({"size":8,"distance":"Cosine"});
    let info: Info = serde_json::from_value(document.clone()).unwrap();
    assert_eq!(dense_name(&info, 4).as_deref(), Some("dense"));
    assert_eq!(dense_name(&info, 8).as_deref(), Some("dense_8"));
    assert!(schema_fields(&info, Some(4), MemoryTier::Cached).is_ok());
    assert!(schema_fields(&info, Some(8), MemoryTier::Cached).is_ok());
    for (name, size, distance) in [
        ("foreign", 8, "Cosine"),
        ("dense_8", 9, "Cosine"),
        ("dense_8", 8, "Dot"),
    ] {
        let mut invalid = document.clone();
        invalid["config"]["params"]["vectors"][name] =
            serde_json::json!({"size":size,"distance":distance});
        let info: Info = serde_json::from_value(invalid).unwrap();
        assert!(schema_fields(&info, Some(4), MemoryTier::Cached).is_err());
    }
    let vectors = Vectors {
        dense: Some(("dense_8".into(), vec![1.0, 0.0])),
        lexical: None,
    };
    assert_eq!(
        serde_json::to_value(vectors).unwrap(),
        serde_json::json!({"dense_8":[1.0,0.0]})
    );
}

#[test]
fn memory_schema_checks_each_component_and_preserves_legacy_cached_collections() {
    let info: Info = serde_json::from_value(legacy()).unwrap();
    assert!(schema_fields(&info, Some(4), MemoryTier::Cached).is_ok());
    assert!(schema_fields(&info, Some(4), MemoryTier::Cold).is_err());
    for path in [
        "/config/params/vectors/dense",
        "/config/hnsw_config",
        "/config/params/sparse_vectors/lexical",
    ] {
        let mut document = legacy();
        let component = document.pointer_mut(path).unwrap().as_object_mut().unwrap();
        if path.ends_with("lexical") {
            component.insert("index".into(), serde_json::json!({"memory":"cold"}));
        } else {
            component.insert("memory".into(), "cold".into());
        }
        let info: Info = serde_json::from_value(document).unwrap();
        assert!(
            schema_fields(&info, Some(4), MemoryTier::Cached).is_err(),
            "{path} must be checked independently"
        );
    }
}

#[test]
fn explicit_memory_precedes_deprecated_on_disk_without_accepting_unknown_tiers() {
    let placement: Placement =
        serde_json::from_str(r#"{"memory":"cold","on_disk":false}"#).unwrap();
    assert!(placement.matches(PhysicalMemory::Cold));
    let placement: Placement = serde_json::from_str(r#"{"on_disk":true}"#).unwrap();
    assert!(placement.matches(PhysicalMemory::Cold));
    assert!(serde_json::from_str::<Placement>(r#"{"memory":"unknown"}"#).is_err());
}

#[test]
fn durable_profile_pins_indexes_caches_dense_vectors_and_keeps_payloads_cold() {
    assert_eq!(MemoryTier::default(), MemoryTier::Durable);
    assert_eq!(MemoryTier::Durable.dense(), PhysicalMemory::Cached);
    assert_eq!(MemoryTier::Durable.index(), PhysicalMemory::Pinned);
    assert_eq!(PAYLOAD_MEMORY, PhysicalMemory::Cold);
    let mut document = legacy();
    document["config"]["params"]["payload"] = serde_json::json!({"memory":"cold"});
    document["config"]["params"]["sparse_vectors"]["lexical"]["index"] =
        serde_json::json!({"memory":"pinned"});
    document["config"]["hnsw_config"]["memory"] = "pinned".into();
    let info: Info = serde_json::from_value(document.clone()).unwrap();
    assert!(schema_fields(&info, Some(4), MemoryTier::Durable).is_ok());
    assert!(schema_fields(&info, Some(4), MemoryTier::Cached).is_err());
    document["config"]["params"]["vectors"]["dense"]["memory"] = "pinned".into();
    let invalid: Info = serde_json::from_value(document).unwrap();
    assert!(schema_fields_mode(&invalid, Some(4), MemoryTier::Durable, false).is_err());
}

#[test]
fn memory_activation_preflight_rejects_foreign_vector_and_payload_schemas() {
    let mut document = legacy();
    document["payload_schema"]["node_id"] = serde_json::json!({"data_type":"integer"});
    let info: Info = serde_json::from_value(document).unwrap();
    assert!(schema_fields_mode(&info, Some(4), MemoryTier::Durable, false).is_err());
    let mut document = legacy();
    document["config"]["params"]["vectors"]["dense"]["distance"] = "Dot".into();
    let info: Info = serde_json::from_value(document).unwrap();
    assert!(schema_fields_mode(&info, Some(4), MemoryTier::Durable, false).is_err());
}

#[test]
fn physical_graph_topology_is_not_a_query_override_or_an_implicit_migration() {
    for (m, ef_construct) in [(0, 200), (32, 200), (16, 100), (16, 400)] {
        let mut document = legacy();
        document["config"]["hnsw_config"]["m"] = m.into();
        document["config"]["hnsw_config"]["ef_construct"] = ef_construct.into();
        let info: Info = serde_json::from_value(document).unwrap();
        assert!(
            schema_fields(&info, Some(4), MemoryTier::Cached).is_err(),
            "accepted m={m}, ef_construct={ef_construct}"
        );
    }
}
