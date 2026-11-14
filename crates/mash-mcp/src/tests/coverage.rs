use super::*;

#[tokio::test]
async fn scoped_status_reports_scan_facts_without_proving_absence() {
    let root = tempfile::tempdir().unwrap();
    let server = Mash {
        paths: Arc::new(super::workflows::decision_history(root.path())),
        gate: Arc::default(),
        runtime: None,
        model_work: Arc::new(tokio::sync::Semaphore::new(0)),
    };
    for (after, before, expected) in [(0, 2, 4), (2, 3, 0)] {
        let response = server
            .status(Parameters(
                serde_json::from_value(json!({
                    "coverage":{"provider":["codex"],"after":after,"before":before}
                }))
                .unwrap(),
            ))
            .await;
        assert_ne!(response.is_error, Some(true), "{response:?}");
        let readable = &response.content[0].as_text().unwrap().text;
        assert!(readable.contains("Absence proven: false"));
        assert!(readable.contains("historical-scan-scope-unrecorded"));
        assert!(
            readable.contains("Recorded scan state (not historical completeness): \"complete\"")
        );
        let data = response.structured_content.unwrap();
        assert_eq!(data["coverage"]["absence_proven"], false);
        let member = &data["coverage"]["members"][0];
        assert_eq!(member["retained"]["messages"], expected);
        assert_eq!(member["providers"][0]["state"], "complete");
        assert_eq!(member["providers"][0]["configured"], false);
        assert!(
            data["coverage"]["limitations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "historical-scan-scope-unrecorded")
        );
    }
    let unknown = server
        .status(Parameters(
            serde_json::from_value(json!({"coverage":{"provider":["claude-code"]}})).unwrap(),
        ))
        .await
        .structured_content
        .unwrap();
    assert_eq!(
        unknown["coverage"]["members"][0]["providers"][0]["state"],
        "unverified"
    );
    let unavailable = server
        .status(Parameters(
            serde_json::from_value(json!({"coverage":{"node":["missing-peer"]}})).unwrap(),
        ))
        .await
        .structured_content
        .unwrap();
    assert_eq!(
        unavailable["coverage"]["unavailable_nodes"],
        json!(["missing-peer"])
    );
    assert_eq!(unavailable["coverage"]["members"], json!([]));
    let invalid = server
        .status(Parameters(
            serde_json::from_value(json!({"coverage":{"after":3,"before":2}})).unwrap(),
        ))
        .await;
    assert_eq!(invalid.is_error, Some(true));
}

#[test]
fn shared_aggregate_certificates_do_not_become_scoped_coverage() {
    let freshness: crate::shared::Freshness = serde_json::from_value(json!({
        "group_id":"group", "nodes":[{"node_id":"peer", "instance":"instance", "revision":1,
            "last_synced_at":null,"last_updated_at":null,"chats":2,"messages":20}]
    }))
    .unwrap();
    for scoped in [false, true] {
        let scope = crate::core::coverage::Scope {
            provider: if scoped {
                vec![Provider::Codex]
            } else {
                vec![]
            },
            ..Default::default()
        };
        let value = serde_json::to_value(
            mash_app::core::coverage::shared_members(&freshness, &scope).unwrap(),
        )
        .unwrap();
        assert!(value[0]["providers"].is_null());
        if scoped {
            assert!(value[0]["retained"].is_null());
        } else {
            assert_eq!(value[0]["retained"]["messages"], 20);
        }
    }
}

#[tokio::test]
async fn unavailable_shared_status_preserves_local_coverage() {
    let root = tempfile::tempdir().unwrap();
    let paths = super::workflows::decision_history(root.path());
    let config = paths.config.as_ref().unwrap();
    let mut text = std::fs::read_to_string(config).unwrap();
    text.push_str("\n[shared]\ngroup_id='group'\nnode_id='self'\napi_url='http://127.0.0.1:9'\ntoken_env='HOME'\n");
    std::fs::write(config, text).unwrap();
    let server = Mash {
        paths: Arc::new(paths),
        gate: Arc::default(),
        runtime: None,
        model_work: Arc::new(tokio::sync::Semaphore::new(0)),
    };
    let result = server
        .status(Parameters(
            serde_json::from_value(json!({"shared":true,"coverage":{"provider":["codex"]}}))
                .unwrap(),
        ))
        .await;
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let readable = &result.content[0].as_text().unwrap().text;
    assert!(readable.contains("Warning — shared status:"));
    assert!(readable.contains("Readiness does not certify complete provider history"));
    let value = result.structured_content.unwrap();
    assert!(value["shared_error"].is_string());
    assert_eq!(value["coverage"]["members"][0]["retained"]["messages"], 4);
    assert_eq!(value["coverage"]["absence_proven"], false);
    assert!(value.get("shared_index").is_none());
}
