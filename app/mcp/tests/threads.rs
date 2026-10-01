use super::*;

#[tokio::test]
#[ignore = "requires MASH_TEST_MODELS_DIR containing an explicit model installation"]
async fn real_models_thread_discovery_and_scoped_messages() {
    let models = std::env::var_os("MASH_TEST_MODELS_DIR").unwrap();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("claude");
    std::fs::create_dir(&root).unwrap();
    for id in ["selected", "excluded"] {
        std::fs::write(root.join(format!("{id}.jsonl")), format!("{{\"type\":\"user\",\"sessionId\":\"{id}\",\"uuid\":\"message\",\"message\":{{\"role\":\"user\",\"content\":\"repair authentication credentials\"}}}}\n")).unwrap();
    }
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        format!(
            "version = 1\n[providers.claude-code]\nroots = ['{}']\n",
            root.display()
        ),
    )
    .unwrap();
    let server = Mash {
        paths: Arc::new(Paths {
            config: Some(config),
            db: Some(directory.path().join("index.sqlite3")),
            models_dir: Some(models.into()),
        }),
        gate: Arc::new(tokio::sync::Mutex::new(())),
        model_work: Arc::new(tokio::sync::Semaphore::new(1)),
    };
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let serving = tokio::spawn(async move { server.serve(server_io).await.unwrap() });
    let client = ().serve(client_io).await.unwrap();
    let server = serving.await.unwrap();
    let indexed = client
        .call_tool(CallToolRequestParams::new("index"))
        .await
        .unwrap();
    assert_ne!(indexed.is_error, Some(true), "{indexed:?}");
    let listed = client
        .call_tool(
            CallToolRequestParams::new("list_threads").with_arguments(
                json!({"local":true,"provider":["claude-code"],"sort":"title"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_ne!(listed.is_error, Some(true), "{listed:?}");
    let listed = listed.structured_content.unwrap();
    let threads = listed["threads"].as_array().unwrap();
    assert_eq!(threads.len(), 2);
    let selected = threads
        .iter()
        .find(|thread| thread["conversation_id"] == "selected")
        .unwrap();
    let found = client.call_tool(CallToolRequestParams::new("search").with_arguments(json!({"query":"authentication", "local":true,"unit":"messages","provider":[selected["provider"]],"conversation":[selected["conversation_id"]]}).as_object().unwrap().clone())).await.unwrap();
    assert_ne!(found.is_error, Some(true), "{found:?}");
    let found = found.structured_content.unwrap();
    let results = found["results"].as_array().unwrap();
    assert_ne!(results.as_slice(), &[] as &[serde_json::Value]);
    assert!(
        results
            .iter()
            .all(|message| message["conversation_id"] == "selected")
    );
    client.cancel().await.unwrap();
    server.cancel().await.unwrap();
}

#[tokio::test]
async fn sdk_lists_metadata_without_model_or_mutation_permits() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    std::fs::write(&config, "version = 1\nproviders = {}\n").unwrap();
    let database = directory.path().join("index.sqlite3");
    let models = directory.path().join("models");
    let mut storage = crate::storage::Storage::open_writer(&database).unwrap();
    storage
        .replace_conversation(&crate::ingestion::Conversation {
            id: "orientation".into(),
            provider: Provider::Codex,
            claude_lineage: None,
            source: crate::ingestion::SourceIdentity::file("history"),
            title: Some("cBioPortal migration".into()),
            project: Some("/work/cbioportal".into()),
            repository: None,
            created_at: Some(1000),
            updated_at: Some(3000),
            messages: Vec::new().into(),
        })
        .unwrap();
    storage.commit_writer().unwrap();
    let server = Mash {
        paths: Arc::new(Paths {
            config: Some(config),
            db: Some(database),
            models_dir: Some(models.clone()),
        }),
        gate: Arc::new(tokio::sync::Mutex::new(())),
        model_work: Arc::new(tokio::sync::Semaphore::new(1)),
    };
    let gate = server.gate.clone();
    let models_gate = server.model_work.clone();
    let mutation = gate.lock().await;
    let model = models_gate.acquire().await.unwrap();
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let serving = tokio::spawn(async move { server.serve(server_io).await.unwrap() });
    let client = ().serve(client_io).await.unwrap();
    let server = serving.await.unwrap();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client.call_tool(
            CallToolRequestParams::new("list_threads").with_arguments(
                json!({
                    "provider":["codex"], "title_contains":"CBIOportal", "after":2,
                    "sort":"title", "direction":"asc", "limit":1, "local":true
                })
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
    let body = result.structured_content.unwrap();
    assert_eq!(body["threads"][0]["conversation_id"], "orientation");
    assert_eq!(body["threads"][0]["provider"], "codex");
    assert_eq!(body["threads"][0]["updated_at"], 3000);
    assert_eq!(body["threads"][0]["node_id"], "local");
    assert_eq!(body["next_offset"], json!(null));
    assert!(!models.exists());
    drop(model);
    drop(mutation);
    for arguments in [
        json!({"limit":0}),
        json!({"sort":"unknown"}),
        json!({"extra":true}),
    ] {
        let response = client
            .call_tool(
                CallToolRequestParams::new("list_threads")
                    .with_arguments(arguments.as_object().unwrap().clone()),
            )
            .await
            .unwrap();
        assert_eq!(response.is_error, Some(true));
    }
    client.cancel().await.unwrap();
    server.cancel().await.unwrap();
}
