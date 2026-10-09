use super::*;

#[tokio::test]
async fn sdk_outputs_mask_credentials_in_view_discovery_and_search() {
    let root = tempfile::tempdir().unwrap();
    let chats = root.path().join("chats");
    std::fs::create_dir(&chats).unwrap();
    std::fs::write(chats.join("session.jsonl"), "{\"type\":\"user\",\"sessionId\":\"session\",\"uuid\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"credentialneedle password=\\\"hunter2\\\"\"}}\n").unwrap();
    let config = root.path().join("node.toml");
    std::fs::write(&config, format!("version=1\n[providers.claude-code]\nroots=['{}']\n[gold.build]\nrepresentations='lexical-only'\n[gold.query]\nintent='exact-mention'\n", chats.display())).unwrap();
    let mash = Mash {
        watcher: Arc::default(),
        paths: Arc::new(Paths {
            config: Some(config),
            db: Some(root.path().join("index.sqlite3")),
            models_dir: Some(root.path().join("models")),
        }),
        gate: Arc::default(),
        model_work: Arc::new(tokio::sync::Semaphore::new(1)),
    };
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let serving = tokio::spawn(async move { mash.serve(server_io).await.unwrap() });
    let client = ().serve(client_io).await.unwrap();
    let server = serving.await.unwrap();
    let mut message_id = String::new();
    for (tool, arguments) in [
        ("index", json!({})),
        ("view", json!({"id":"message"})),
        ("list_chats", json!({"local":true})),
        ("search", json!({"query":"credentialneedle","local":true})),
    ] {
        let arguments = if tool == "view" {
            json!({"id":message_id})
        } else {
            arguments
        };
        let result = client
            .call_tool(
                CallToolRequestParams::new(tool)
                    .with_arguments(arguments.as_object().unwrap().clone()),
            )
            .await
            .unwrap();
        assert_ne!(result.is_error, Some(true), "{result:?}");
        let output = serde_json::to_string(&result).unwrap();
        assert!(!output.contains("hunter2"));
        if tool == "index" {
            message_id = rusqlite::Connection::open(root.path().join("index.sqlite3"))
                .unwrap()
                .query_row("SELECT id FROM messages LIMIT 1", [], |row| row.get(0))
                .unwrap();
        }
        if tool != "index" {
            assert!(
                output.contains("REDACTED_SECRET") || output.contains("*****"),
                "{output}"
            );
        }
    }
    client.cancel().await.unwrap();
    server.waiting().await.unwrap();
}
