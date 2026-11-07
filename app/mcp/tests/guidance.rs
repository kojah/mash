use super::*;

#[tokio::test]
async fn history_skill_help_works_without_storage_models_or_mutation_permits() {
    let directory = tempfile::tempdir().unwrap();
    let server = Mash {
        paths: Arc::new(Paths {
            config: Some(directory.path().join("absent.toml")),
            db: Some(directory.path().join("absent.sqlite3")),
            models_dir: None,
        }),
        runtime: None,
        gate: Arc::default(),
        model_work: Arc::new(tokio::sync::Semaphore::new(0)),
    };
    let gate = server.gate.clone();
    let _guard = gate.lock().await;
    let (server_io, client_io) = tokio::io::duplex(128 * 1024);
    let handle = tokio::spawn(async move { server.serve(server_io).await.unwrap() });
    let client = ().serve(client_io).await.unwrap();
    let server = handle.await.unwrap();
    let result = tokio::time::timeout(
        crate::test_support::HANG_GUARD,
        client.call_tool(
            CallToolRequestParams::new("help").with_arguments(
                json!({"tool":"search","expected_contract":"0".repeat(64)})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_ne!(result.is_error, Some(true));
    let data = result.structured_content.unwrap();
    assert_eq!(data["tool"]["name"], "search");
    assert!(
        data["tool"]["inputSchema"]["properties"]
            .get("chat")
            .is_some()
    );
    assert!(
        data["tool"]["inputSchema"]["properties"]
            .get("conversation")
            .is_none()
    );
    assert_eq!(data["runtime"]["contract_matches"], false);
    assert_eq!(
        data["runtime"]["contract_fingerprint"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert!(
        data["examples"]
            .as_array()
            .unwrap()
            .iter()
            .any(|example| example["arguments"]["chat"].is_array())
    );
    let bad = client
        .call_tool(
            CallToolRequestParams::new("help")
                .with_arguments(json!({"tool":"invented"}).as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    assert_eq!(bad.is_error, Some(true));
    assert_eq!(bad.structured_content.unwrap()["error"]["kind"], "usage");
    let records = client
        .call_tool(
            CallToolRequestParams::new("help")
                .with_arguments(json!({"topic":"records"}).as_object().unwrap().clone()),
        )
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_record_help_examples(&records);
    assert!(!directory.path().join("absent.sqlite3").exists());
    client.cancel().await.unwrap();
    server.waiting().await.unwrap();
}

fn assert_record_help_examples(records: &serde_json::Value) {
    let examples = records["examples"].as_array().unwrap();
    for tool in ["record", "list_records", "get_record", "recall_records"] {
        assert!(examples.iter().any(|item| item["tool"] == tool));
    }
    for action in ["create", "revise", "resolve", "forget"] {
        assert!(examples.iter().any(
            |item| item["tool"] == "record" && item["arguments"]["change"]["action"] == action
        ));
    }
    assert!(examples.iter().all(|item| matches!(
        item["tool"].as_str(),
        Some("record" | "list_records" | "get_record" | "recall_records")
    )));
    assert!(
        records["guidance"]
            .as_str()
            .unwrap()
            .contains("review set to established")
    );
}
