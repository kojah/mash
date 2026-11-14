use super::*;

#[tokio::test]
async fn sdk_export_import_is_model_free_and_keeps_the_sessions_database() {
    let root = tempfile::tempdir().unwrap();
    let db = root.path().join("original.sqlite3");
    let mut writer = crate::storage::Storage::open_writer(&db).unwrap();
    writer.finish_silver_ingestion().unwrap();
    writer.commit_writer().unwrap();
    drop(writer);
    let config = root.path().join("node.toml");
    std::fs::write(&config, "version=1\nproviders={}\n").unwrap();
    let mash = Mash {
        runtime: None,
        paths: Arc::new(Paths {
            config: Some(config),
            db: Some(db.clone()),
            models_dir: Some(root.path().join("missing-models")),
        }),
        gate: Arc::default(),
        model_work: Arc::new(tokio::sync::Semaphore::new(0)),
    };
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let serving = tokio::spawn(async move { mash.serve(server_io).await.unwrap() });
    let client = ().serve(client_io).await.unwrap();
    let server = serving.await.unwrap();
    let export = root.path().join("export");
    let imported = root.path().join("imported.sqlite3");
    for (name, args) in [
        ("export_silver", json!({"directory":export})),
        (
            "import_silver",
            json!({"directory":export,"database":imported}),
        ),
    ] {
        let result = tokio::time::timeout(
            // Export/import includes durable filesystem writes; this checks that
            // the zero model permit cannot block it, not a five-second latency SLA.
            std::time::Duration::from_secs(30),
            client.call_tool(
                CallToolRequestParams::new(name).with_arguments(args.as_object().unwrap().clone()),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_ne!(result.is_error, Some(true), "{result:?}");
        let text = &result.content[0].as_text().unwrap().text;
        assert!(text.starts_with("Mash silver"));
        if name == "import_silver" {
            assert!(text.contains("Search ready: false"));
            assert!(text.contains("rebuild-local-views"));
        }
    }
    let result = client
        .call_tool(
            CallToolRequestParams::new("import_silver").with_arguments(
                json!({"directory":export,"database":db})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    assert!(crate::storage::Storage::open_silver(&db).is_ok());
    assert!(crate::storage::Storage::open_silver(&imported).is_ok());
    client.cancel().await.unwrap();
    server.waiting().await.unwrap();
}
