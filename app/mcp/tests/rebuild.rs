use super::*;

#[test]
fn built_in_views_rebuild_wire_key_replaces_gold_without_aliases() {
    assert!(serde_json::from_value::<Index>(json!({"rebuild_local_gold": true})).is_err());
    let request: Index = serde_json::from_value(json!({"rebuild_local_views": true})).unwrap();
    assert!(request.rebuild_local_views);
}

#[tokio::test]
async fn lexical_views_rebuild_streams_mcp_progress_without_models_sources_or_shared_service() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("index.sqlite3");
    let models = directory.path().join("models");
    let mut storage = crate::storage::Storage::open_writer(&database).unwrap();
    crate::silver::local::ingest(
        &mut storage,
        &crate::silver::IndexOptions {
            providers: std::collections::BTreeMap::default(),
            roots_are_authoritative: true,
            since_days: None,
        },
    )
    .unwrap();
    storage.commit_writer().unwrap();
    let certificate = storage.silver_certificate().unwrap();
    drop(storage);
    let config = directory.path().join("config.toml");
    let missing = directory.path().join("missing-sources");
    std::fs::write(&config, format!("version=1\nproviders={{codex={{roots=[{:?}]}}}}\n[index]\ndestination='both'\n[shared]\ngroup_id='group'\nnode_id='node'\napi_url='http://127.0.0.1:9'\ntoken_env='MASH_VIEWS_REBUILD_NO_TOKEN'\n[views.build]\nrepresentations='lexical-only'\n[views.query]\nintent='exact-mention'\n", missing.to_str().unwrap())).unwrap();
    let server = Mash {
        watcher: Arc::default(),
        paths: Arc::new(Paths {
            config: Some(config),
            db: Some(database.clone()),
            models_dir: Some(models),
        }),
        gate: Arc::new(tokio::sync::Mutex::new(())),
        model_work: Arc::new(tokio::sync::Semaphore::new(0)),
    };
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let serving = tokio::spawn(async move { server.serve(server_io).await.unwrap() });
    let (read, mut write) = tokio::io::split(client_io);
    let mut read = tokio::io::BufReader::new(read);
    wire_send(&mut write, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"views-rebuild-test","version":"1"}}})).await;
    assert_eq!(wire_next(&mut read).await["id"], 1);
    wire_send(
        &mut write,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )
    .await;
    let server = serving.await.unwrap();
    wire_send(&mut write,json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"index","arguments":{"rebuild_local_views":true,"full":true},"_meta":{"progressToken":"views"}}})).await;
    let mut rebuilt = false;
    let mut complete = false;
    loop {
        let message = wire_next(&mut read).await;
        if message.get("id").is_some() {
            assert_eq!(message["id"], 2);
            assert_ne!(message["result"]["isError"], true, "{message}");
            let result = &message["result"]["structuredContent"];
            assert_eq!(result["rebuild_local_views"], true);
            assert_eq!(result["realized_mode"], "lexical");
            assert_eq!(result["scanned_files"], 0);
            assert!(result["shared"].is_null());
            assert_eq!(
                result["silver"],
                serde_json::to_value(&certificate).unwrap()
            );
            break;
        }
        assert_eq!(message["params"]["progressToken"], "views");
        let event: crate::progress::Event = serde_json::from_value(
            message["params"]["_meta"]["dev.jamesqo.mash/index-progress"].clone(),
        )
        .unwrap();
        rebuilt |= event.phase == crate::progress::Phase::SearchRebuild;
        complete |= event.phase == crate::progress::Phase::Complete;
    }
    assert!(rebuilt && complete);
    wire_send(&mut write,json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"index","arguments":{"rebuild_local_views":true,"provider":["codex"]}}})).await;
    let invalid = wire_next(&mut read).await;
    assert_eq!(invalid["result"]["isError"], true);
    assert_eq!(
        invalid["result"]["structuredContent"]["error"]["kind"],
        "usage"
    );
    assert!(!missing.exists());
    assert_eq!(
        crate::storage::Storage::open_catalog(&database)
            .unwrap()
            .silver_certificate()
            .unwrap(),
        certificate
    );
    drop(write);
    drop(read);
    server.cancel().await.unwrap();
}
