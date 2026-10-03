use super::*;

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
