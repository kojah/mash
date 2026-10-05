use super::*;

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
async fn awaited_mutations_outlive_read_deadlines_without_replaying_requests() {
    use axum::{
        Router,
        routing::{post, put},
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let writes = Arc::new(AtomicUsize::new(0));
    let captured = writes.clone();
    let mutation = move || {
        let captured = captured.clone();
        async move {
            captured.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(200)).await;
            r#"{"status":"ok","result":{"status":"completed"}}"#
        }
    };
    let router = Router::new()
        .route(
            "/collections/test/points",
            put(mutation.clone()).post(|| async {
                tokio::time::sleep(Duration::from_millis(200)).await;
                r#"{"status":"ok","result":[]}"#
            }),
        )
        .route("/collections/test/points/delete", post(mutation));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client {
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
    client
        .upsert(
            "test",
            &Upsert {
                points: vec![Point {
                    id: "one".into(),
                    vector: Vectors::default(),
                    payload: (),
                }],
                update_filter: None,
                update_mode: None,
            },
        )
        .await
        .unwrap();
    client
        .delete_versions("test", &["old".into()])
        .await
        .unwrap();
    assert!(
        client
            .retrieve::<()>("test", &["one".into()])
            .await
            .is_err()
    );
    assert_eq!(
        writes.load(Ordering::SeqCst),
        2,
        "each mutation is sent once"
    );
    server.abort();
    let _ = server.await;
}

#[test]
fn ordinal_matches_preserve_integers_above_float_precision() {
    let condition = integers("message.ordinal", vec![9_007_199_254_740_993, i64::MAX]);
    let value = serde_json::to_value(condition).unwrap();
    assert_eq!(
        value["match"]["any"][0].as_i64(),
        Some(9_007_199_254_740_993)
    );
    assert_eq!(value["match"]["any"][1].as_i64(), Some(i64::MAX));
}

fn legacy() -> serde_json::Value {
    serde_json::json!({
        "config": {
            "params": {
                "vectors": {"dense": {"size":4, "distance":"Cosine"}},
                "sparse_vectors": {"lexical": {"modifier":"idf"}}
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
    assert!(placement.matches(MemoryTier::Cold));
    let placement: Placement = serde_json::from_str(r#"{"on_disk":true}"#).unwrap();
    assert!(placement.matches(MemoryTier::Cold));
    assert!(serde_json::from_str::<Placement>(r#"{"memory":"unknown"}"#).is_err());
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
