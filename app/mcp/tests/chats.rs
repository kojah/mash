use super::*;

#[tokio::test]
#[ignore = "requires installed MASH_TEST_MODELS_DIR and disposable Qdrant REST/gRPC endpoints"]
#[allow(
    clippy::too_many_lines,
    reason = "keep the MCP index/discovery/search journey and owned-backend cleanup together"
)]
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
            "version = 1\n[providers.claude-code]\nroots = ['{}']\n[views.query]\nunit = 'messages'\nlimit = 1\n",
            root.display()
        ),
    )
    .unwrap();
    let qdrant_url = crate::views::local::test_support::configure(&config);
    let configuration = crate::config::load(Some(&config)).unwrap();
    let server = Mash {
        runtime: None,
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
            CallToolRequestParams::new("list_chats").with_arguments(
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
    let threads = listed["chats"].as_array().unwrap();
    assert_eq!(threads.len(), 2);
    let selected = threads
        .iter()
        .find(|thread| thread["chat_id"] == "selected")
        .unwrap();
    let database = directory.path().join("index.sqlite3");
    let before = std::fs::read(&database).unwrap();
    // Search must use existing views even when bronze no longer exists.
    std::fs::remove_dir_all(&root).unwrap();
    let defaults = client
        .call_tool(
            CallToolRequestParams::new("search").with_arguments(
                json!({"query":"authentication", "local":true})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_ne!(defaults.is_error, Some(true), "{defaults:?}");
    let defaults = defaults.structured_content.unwrap();
    assert_eq!(defaults["unit"], "messages");
    assert_eq!(defaults["results"].as_array().unwrap().len(), 1);
    let overridden = client
        .call_tool(
            CallToolRequestParams::new("search").with_arguments(
                json!({"query":"authentication", "local":true,"unit":"chats", "limit":2})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_ne!(overridden.is_error, Some(true), "{overridden:?}");
    assert_eq!(overridden.structured_content.unwrap()["unit"], "chats");
    let found = client.call_tool(CallToolRequestParams::new("search").with_arguments(json!({"query":"authentication", "local":true,"unit":"messages","provider":[selected["provider"]],"chat":[selected["chat_id"]]}).as_object().unwrap().clone())).await.unwrap();
    assert_ne!(found.is_error, Some(true), "{found:?}");
    let found = found.structured_content.unwrap();
    let results = found["results"].as_array().unwrap();
    assert_ne!(results.as_slice(), &[] as &[serde_json::Value]);
    assert!(
        results
            .iter()
            .all(|message| message["chat_id"] == "selected")
    );
    assert_eq!(std::fs::read(&database).unwrap(), before);
    client.cancel().await.unwrap();
    server.cancel().await.unwrap();
    let local = crate::views::local::Local::open(&database, &configuration)
        .await
        .unwrap();
    crate::views::local::test_support::cleanup(&local, &qdrant_url).await;
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
        .replace_conversation(&crate::silver::Conversation {
            id: "orientation".into(),
            provider: Provider::Codex,
            title_origin: None,
            timestamp_origins: None,
            claude_lineage: None,
            source: crate::silver::SourceIdentity::file("history"),
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
        runtime: None,
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
        crate::test_support::HANG_GUARD,
        client.call_tool(
            CallToolRequestParams::new("list_chats").with_arguments(
                json!({
                    "provider":["codex"], "title_contains":"CBIOportal", "after":2,
                    "sort":"title", "direction":"asc", "limit":1, "local":true,
                    "diagnostics":true
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
    assert_eq!(body["chats"][0]["chat_id"], "orientation");
    assert_eq!(body["chats"][0]["provider"], "codex");
    assert_eq!(body["chats"][0]["updated_at"], 3000);
    assert_eq!(body["chats"][0]["node_id"], "local");
    assert_eq!(body["next_offset"], json!(null));
    assert_eq!(
        body["diagnostics"]["selected_artifacts"],
        json!(["metadata"])
    );
    assert_eq!(body["diagnostics"]["completed_sources"], json!(["local"]));
    assert_eq!(body["diagnostics"]["stages"][0]["stage"], "local-metadata");
    assert_eq!(body["diagnostics"]["stages"][1]["stage"], "metadata-merge");
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
                CallToolRequestParams::new("list_chats")
                    .with_arguments(arguments.as_object().unwrap().clone()),
            )
            .await
            .unwrap();
        assert_eq!(response.is_error, Some(true));
    }
    client.cancel().await.unwrap();
    server.cancel().await.unwrap();
}
