use super::*;

#[tokio::test]
async fn invalid_fusion_overrides_bypass_busy_model_work_and_leave_paths_untouched() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("node.toml");
    std::fs::write(&config, "version=1\nproviders={}\n").unwrap();
    let database = root.path().join("missing.sqlite3");
    let models = root.path().join("missing-models");
    let server = Mash {
        paths: Arc::new(Paths {
            config: Some(config),
            db: Some(database.clone()),
            models_dir: Some(models.clone()),
        }),
        gate: Arc::new(tokio::sync::Mutex::new(())),
        model_work: Arc::new(tokio::sync::Semaphore::new(0)),
    };
    for (intent, fusion) in [
        ("related-discussion", json!({"lexical_weight":0})),
        ("related-discussion", json!({"rank_constant":1001})),
        ("related-discussion", json!({"semantic_weight":101})),
        ("exact-mention", json!({"semantic_weight":4})),
    ] {
        let request: Search = serde_json::from_value(
            json!({"query":"database", "local":true, "intent":intent, "fusion":fusion}),
        )
        .unwrap();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            server.search(Parameters(request)),
        )
        .await
        .unwrap();
        assert_eq!(result.is_error, Some(true));
        assert_eq!(result.structured_content.unwrap()["error"]["kind"], "usage");
    }
    assert!(!database.exists());
    assert!(!models.exists());
}

fn configuration(root: &std::path::Path, constant: u16, lexical: u16) -> String {
    format!(
        "version=1\n[providers.codex]\nroots=[{:?}]\n[gold.query]\nunit='messages'\n[gold.query.fusion]\nrank_constant={constant}\nlexical_weight={lexical}\nsemantic_weight=2\n",
        root.join("codex").to_str().unwrap()
    )
}

async fn indexed_fixture() -> (Mash, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("codex");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("session.jsonl"), concat!(
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"thread\"}}\n",
        "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"cache model\"}]}}\n"
    )).unwrap();
    let config = root.path().join("node.toml");
    std::fs::write(&config, configuration(root.path(), 10, 4)).unwrap();
    let server = Mash {
        paths: Arc::new(Paths {
            config: Some(config),
            db: Some(root.path().join("index.sqlite3")),
            models_dir: Some(
                std::env::var_os("MASH_TEST_MODELS_DIR")
                    .expect("explicit installed models")
                    .into(),
            ),
        }),
        gate: Arc::new(tokio::sync::Mutex::new(())),
        model_work: Arc::new(tokio::sync::Semaphore::new(1)),
    };
    core::execute(Request {
        paths: server.paths.as_ref().clone(),
        operation: Operation::Index(IndexRequest {
            all_history: true,
            ..Default::default()
        }),
    })
    .await
    .unwrap();
    std::fs::remove_dir_all(source).unwrap();
    (server, root)
}

#[tokio::test]
#[ignore = "requires explicitly installed MASH_TEST_MODELS_DIR"]
async fn real_models_mcp_fusion_overrides_inherit_reloaded_defaults_without_indexing() {
    let (server, root) = indexed_fixture().await;
    let database = root.path().join("index.sqlite3");
    let before = std::fs::read(&database).unwrap();
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(async move { server.serve(server_io).await.unwrap() });
    let client = ().serve(client_io).await.unwrap();
    let service = task.await.unwrap();
    for (constant, lexical, override_weight, expected) in [
        (10, 4, None, 6.0 / 11.0),
        (10, 4, Some(5), 9.0 / 11.0),
        (20, 7, Some(5), 12.0 / 21.0),
        (20, 7, None, 9.0 / 21.0),
    ] {
        std::fs::write(
            root.path().join("node.toml"),
            configuration(root.path(), constant, lexical),
        )
        .unwrap();
        let mut args = json!({"query":"cache model", "local":true});
        if let Some(weight) = override_weight {
            args["fusion"] = json!({"semantic_weight":weight});
        }
        let response = client
            .call_tool(
                CallToolRequestParams::new("search")
                    .with_arguments(args.as_object().unwrap().clone()),
            )
            .await
            .unwrap();
        assert_ne!(response.is_error, Some(true), "{response:?}");
        let results = response.structured_content.unwrap();
        let hits = results["results"].as_array().unwrap();
        assert_eq!(hits.len(), 1);
        assert!((hits[0]["fusion_score"].as_f64().unwrap() - expected).abs() < 1e-6);
        assert_eq!(std::fs::read(&database).unwrap(), before);
    }
    client.cancel().await.unwrap();
    service.cancel().await.unwrap();
}
