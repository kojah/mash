use super::*;

#[tokio::test]
async fn invalid_fusion_overrides_bypass_busy_model_work_and_leave_paths_untouched() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("node.toml");
    std::fs::write(&config, "version=1\nproviders={}\n").unwrap();
    let database = root.path().join("missing.sqlite3");
    let models = root.path().join("missing-models");
    let server = Mash {
        runtime: None,
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
            crate::test_support::HANG_GUARD,
            server.search(Parameters(request)),
        )
        .await
        .unwrap();
        assert_eq!(result.is_error, Some(true));
        assert_eq!(result.structured_content.unwrap()["error"]["kind"], "usage");
    }
    for ranking in ["fusion", "contextual-rerank"] {
        let request: Search = serde_json::from_value(
            json!({"query":"database", "intent":"exact-mention", "ranking":ranking}),
        )
        .unwrap();
        let result = tokio::time::timeout(
            crate::test_support::HANG_GUARD,
            server.search(Parameters(request)),
        )
        .await
        .unwrap();
        assert_eq!(result.is_error, Some(true));
        assert_eq!(result.structured_content.unwrap()["error"]["kind"], "usage");
    }
    assert!(
        serde_json::from_value::<Search>(json!({"query":"database", "ranking":"unknown"})).is_err()
    );
    std::fs::write(
        root.path().join("node.toml"),
        "version=1\nproviders={}\n[views.query]\nintent='exact-mention'\n",
    )
    .unwrap();
    let request: Search =
        serde_json::from_value(json!({"query":"database", "ranking":"fusion"})).unwrap();
    let result = tokio::time::timeout(
        crate::test_support::HANG_GUARD,
        server.search(Parameters(request)),
    )
    .await
    .unwrap();
    assert_eq!(result.is_error, Some(true));
    assert_eq!(result.structured_content.unwrap()["error"]["kind"], "usage");
    assert!(!database.exists());
    assert!(!models.exists());
}

#[tokio::test]
async fn vector_overrides_reject_invalid_or_irrelevant_requests_before_model_admission() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("node.toml");
    std::fs::write(&config, "version=1\nproviders={}\n").unwrap();
    let server = Mash {
        runtime: None,
        paths: Arc::new(Paths {
            config: Some(config),
            db: Some(root.path().join("absent.sqlite3")),
            models_dir: Some(root.path().join("absent-models")),
        }),
        gate: Arc::new(tokio::sync::Mutex::new(())),
        model_work: Arc::new(tokio::sync::Semaphore::new(0)),
    };
    for (intent, local, policy) in [
        (
            "related-discussion",
            false,
            json!({"algorithm":"hnsw","ef_runtime":0}),
        ),
        (
            "related-discussion",
            false,
            json!({"algorithm":"hnsw","ef_runtime":4097}),
        ),
        ("related-discussion", true, json!({"algorithm":"hnsw"})),
        ("exact-mention", false, json!({"algorithm":"exact"})),
        ("related-discussion", false, json!({"algorithm":"exact"})),
    ] {
        let request = serde_json::from_value(json!({"query":"database", "intent":intent, "local":local, "shared_vector_search":policy})).unwrap();
        let result = tokio::time::timeout(
            crate::test_support::HANG_GUARD,
            server.search(Parameters(request)),
        )
        .await
        .unwrap();
        assert_eq!(result.is_error, Some(true));
        assert_eq!(result.structured_content.unwrap()["error"]["kind"], "usage");
    }
    for policy in [
        json!({"algorithm":"unknown"}),
        json!({"algorithm":"exact","ef_runtime":80}),
    ] {
        assert!(
            serde_json::from_value::<Search>(
                json!({"query":"database","shared_vector_search":policy})
            )
            .is_err()
        );
    }
    assert!(!root.path().join("absent.sqlite3").exists());
    assert!(!root.path().join("absent-models").exists());
}

#[tokio::test]
async fn vector_override_checks_shared_capabilities_before_busy_model_admission() {
    let root = tempfile::tempdir().unwrap();
    let transport = crate::test_support::vector::Service::with_dense(false).await;
    let config = root.path().join("node.toml");
    std::fs::write(&config, transport.configuration()).unwrap();
    let server = Mash {
        runtime: None,
        paths: Arc::new(Paths {
            config: Some(config),
            db: Some(root.path().join("absent.sqlite3")),
            models_dir: Some(root.path().join("absent-models")),
        }),
        gate: Arc::new(tokio::sync::Mutex::new(())),
        model_work: Arc::new(tokio::sync::Semaphore::new(0)),
    };
    let request = serde_json::from_value(json!({"query":"database","shared":true,"intent":"related-discussion","shared_vector_search":{"algorithm":"hnsw"}})).unwrap();
    let result = tokio::time::timeout(
        crate::test_support::HANG_GUARD,
        server.search(Parameters(request)),
    )
    .await
    .unwrap();
    assert_eq!(result.structured_content.unwrap()["error"]["kind"], "usage");
    assert!(!root.path().join("absent.sqlite3").exists());
    assert!(!root.path().join("absent-models").exists());
}

#[tokio::test]
#[ignore = "requires explicitly installed MASH_TEST_MODELS_DIR"]
async fn mcp_protocol_forwards_vector_overrides_and_restores_omission() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("node.toml");
    let mut transport = crate::test_support::vector::Service::new().await;
    std::fs::write(&config, "version=1\nproviders={}\n[views.build]\nrepresentations='lexical-only'\n[views.query]\nintent='exact-mention'\n").unwrap();
    let server = Mash {
        runtime: None,
        paths: Arc::new(Paths {
            config: Some(config.clone()),
            db: Some(root.path().join("unused.sqlite3")),
            models_dir: Some(std::env::var_os("MASH_TEST_MODELS_DIR").unwrap().into()),
        }),
        gate: Arc::new(tokio::sync::Mutex::new(())),
        model_work: Arc::new(tokio::sync::Semaphore::new(1)),
    };
    // Query inference uses installed models; no local Qdrant is required.
    let mut storage =
        crate::storage::Storage::open_writer(&root.path().join("unused.sqlite3")).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let before = std::fs::read(root.path().join("unused.sqlite3")).unwrap();
    std::fs::write(&config, transport.configuration()).unwrap();
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(async move { server.serve(server_io).await.unwrap() });
    let client = ().serve(client_io).await.unwrap();
    let service = task.await.unwrap();
    for policy in [
        None,
        Some(json!({"algorithm":"hnsw","ef_runtime":80})),
        None,
        Some(json!({"algorithm":"exact"})),
        None,
    ] {
        let mut args = json!({"query":"database","shared":true,"intent":"related-discussion","ranking":"fusion","diagnostics":true});
        if let Some(policy) = &policy {
            args["shared_vector_search"] = policy.clone();
        }
        let response = client
            .call_tool(
                CallToolRequestParams::new("search")
                    .with_arguments(args.as_object().unwrap().clone()),
            )
            .await
            .unwrap();
        assert_ne!(response.is_error, Some(true), "{response:?}");
        assert_eq!(
            transport.received().await.get("vector_search"),
            policy.as_ref()
        );
        assert_eq!(
            response.structured_content.unwrap()["diagnostics"].get("shared_vector_override"),
            policy.as_ref()
        );
    }
    assert_eq!(
        std::fs::read(root.path().join("unused.sqlite3")).unwrap(),
        before
    );
    assert!(!root.path().join("uninstalled-models").exists());
    client.cancel().await.unwrap();
    service.cancel().await.unwrap();
}

fn configuration(root: &std::path::Path, constant: u16, lexical: u16) -> String {
    let qdrant = crate::views::local::test_support::qdrant_configuration();
    format!(
        "version=1\n[providers.codex]\nroots=[{:?}]\n[views.query]\nunit='messages'\n[views.query.fusion]\nrank_constant={constant}\nlexical_weight={lexical}\nsemantic_weight=2\n[qdrant]\nurl={:?}\ngrpc_url={:?}\n",
        root.join("codex").to_str().unwrap(),
        qdrant.url,
        qdrant.grpc_url
    )
}

async fn cleanup_fixture(root: &std::path::Path) {
    let config = crate::config::load(Some(&root.join("node.toml"))).unwrap();
    let local = crate::views::local::Local::open(&root.join("index.sqlite3"), &config)
        .await
        .unwrap();
    crate::views::local::test_support::cleanup(&local, &config.qdrant.url).await;
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
        runtime: None,
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
#[ignore = "requires installed MASH_TEST_MODELS_DIR and disposable Qdrant REST/gRPC endpoints"]
async fn real_models_mcp_thread_excerpts_preserve_projection_locations() {
    let (server, root) = indexed_fixture().await;
    let database = root.path().join("index.sqlite3");
    let before = std::fs::read(&database).unwrap();
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(async move { server.serve(server_io).await.unwrap() });
    let client = ().serve(client_io).await.unwrap();
    let service = task.await.unwrap();
    let args = json!({"query":"cache model", "intent":"exact-mention", "unit":"chats", "local":true,"detail":"extended"});
    let result = client
        .call_tool(
            CallToolRequestParams::new("search").with_arguments(args.as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let data = result.structured_content.unwrap();
    let crate::search_results::Results::Chats(threads) =
        serde_json::from_value(data["results"].clone()).unwrap()
    else {
        panic!("chats")
    };
    assert_eq!(threads.len(), 1);
    let anchor = &threads[0].matches[0];
    assert_eq!(anchor.snippet, "cache model");
    assert_eq!(
        anchor.projection_span,
        crate::search_results::ProjectionSpan {
            start_char: 0,
            end_char: 11
        }
    );
    assert_eq!(std::fs::read(database).unwrap(), before);
    client.cancel().await.unwrap();
    service.cancel().await.unwrap();
    cleanup_fixture(root.path()).await;
}

#[tokio::test]
#[ignore = "requires installed MASH_TEST_MODELS_DIR and disposable Qdrant REST/gRPC endpoints"]
async fn real_models_mcp_ranking_defaults_reload_without_indexing() {
    let (server, root) = indexed_fixture().await;
    let database = root.path().join("index.sqlite3");
    let before = std::fs::read(&database).unwrap();
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(async move { server.serve(server_io).await.unwrap() });
    let client = ().serve(client_io).await.unwrap();
    let service = task.await.unwrap();
    for (ranking, override_ranking, expected) in [
        ("contextual-rerank", None, "contextual-rerank"),
        ("contextual-rerank", Some("fusion"), "fusion"),
        ("contextual-rerank", None, "contextual-rerank"),
        ("fusion", Some("contextual-rerank"), "contextual-rerank"),
        ("fusion", None, "fusion"),
    ] {
        let config = configuration(root.path(), 10, 4).replace(
            "unit='messages'",
            &format!("unit='messages'\nranking='{ranking}'"),
        );
        std::fs::write(root.path().join("node.toml"), config).unwrap();
        let mut args = json!({"query":"cache model", "local":true, "diagnostics":true});
        if let Some(override_ranking) = override_ranking {
            args["ranking"] = json!(override_ranking);
        }
        let response = client
            .call_tool(
                CallToolRequestParams::new("search")
                    .with_arguments(args.as_object().unwrap().clone()),
            )
            .await
            .unwrap();
        assert_ne!(response.is_error, Some(true), "{response:?}");
        let result = response.structured_content.unwrap();
        assert_eq!(result["results"].as_array().unwrap().len(), 1);
        assert_eq!(result["diagnostics"]["ranking"], expected);
        let stages = result["diagnostics"]["stages"].as_array().unwrap();
        assert_eq!(
            stages.iter().any(|s| s["stage"] == "rerank"),
            expected == "contextual-rerank"
        );
        assert_eq!(std::fs::read(&database).unwrap(), before);
    }
    client.cancel().await.unwrap();
    service.cancel().await.unwrap();
    cleanup_fixture(root.path()).await;
}

#[tokio::test]
#[ignore = "requires installed MASH_TEST_MODELS_DIR and disposable Qdrant REST/gRPC endpoints"]
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
        let mut args = json!({"query":"cache model", "local":true,"detail":"extended"});
        if let Some(weight) = override_weight {
            args["fusion"] = json!({"semantic_weight":weight});
            args["diagnostics"] = json!(true);
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
        if let Some(weight) = override_weight {
            let diagnostics = &results["diagnostics"];
            assert_eq!(diagnostics["fusion"]["semantic_weight"], weight);
            assert_eq!(diagnostics["fusion"]["rank_constant"], constant);
            assert_eq!(diagnostics["requested_scope"], "local");
            assert_eq!(diagnostics["returned_results"], 1);
            assert_eq!(
                diagnostics["selected_artifacts"],
                json!(["metadata", "lexical", "dense"])
            );
            assert!(diagnostics["elapsed_ms"].is_u64());
            let stages = diagnostics["stages"].as_array().unwrap();
            for name in ["local-candidates", "embed", "rerank", "fusion"] {
                let stage = stages.iter().find(|s| s["stage"] == name).unwrap();
                assert_eq!(stage["calls"], 1);
                assert_eq!(stage["completed_calls"], 1);
                assert_eq!(
                    stage["items_out"],
                    if name == "local-candidates" { 2 } else { 1 }
                );
                assert!(stage["total_us"].as_u64().unwrap() >= stage["max_us"].as_u64().unwrap());
            }
        } else {
            assert!(results.get("diagnostics").is_none());
        }
        assert_eq!(std::fs::read(&database).unwrap(), before);
    }
    client.cancel().await.unwrap();
    service.cancel().await.unwrap();
    cleanup_fixture(root.path()).await;
}
