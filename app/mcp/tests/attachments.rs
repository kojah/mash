use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};

#[tokio::test]
async fn sdk_attachments_survive_restart_and_source_removal_and_fail_closed_after_forget() {
    let root = tempfile::tempdir().unwrap();
    let chats = root.path().join("chats");
    std::fs::create_dir(&chats).unwrap();
    let source = chats.join("session.jsonl");
    std::fs::write(&source, serde_json::to_string(&json!({"type":"user","sessionId":"session","uuid":"message","message":{"role":"user","content":[
        {"type":"text","text":"Analyze this attachment"},
        {"type":"document","title":"notes.txt","source":{"type":"base64","media_type":"text/plain","data":STANDARD.encode("password=\"hunter2\"\nuseful notes")}},
        {"type":"image","source":{"type":"base64","media_type":"image/png","data":STANDARD.encode([137,80,78,71,13,10,26,10])}}
    ]}})).unwrap()).unwrap();
    let config = root.path().join("node.toml");
    std::fs::write(&config, format!("version=1\n[providers.claude-code]\nroots=['{}']\n[gold.build]\nrepresentations='lexical-only'\n[gold.query]\nintent='exact-mention'\n", chats.display())).unwrap();
    let db = root.path().join("index.sqlite3");
    let paths = Paths {
        config: Some(config),
        db: Some(db.clone()),
        models_dir: Some(root.path().join("models")),
    };
    let indexed = core::execute(Request {
        paths: paths.clone(),
        operation: Operation::Index(IndexRequest::default()),
    })
    .await
    .unwrap();
    assert!(!indexed.needs_retry());
    let id: String = rusqlite::Connection::open(&db)
        .unwrap()
        .query_row("SELECT id FROM messages LIMIT 1", [], |r| r.get(0))
        .unwrap();
    std::fs::remove_file(source).unwrap();
    let mash = Mash {
        watcher: Arc::default(),
        paths: Arc::new(paths),
        gate: Arc::default(),
        model_work: Arc::new(tokio::sync::Semaphore::new(0)),
    };
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let serving = tokio::spawn(async move { mash.serve(server_io).await.unwrap() });
    let client = ().serve(client_io).await.unwrap();
    let server = serving.await.unwrap();
    let call = |name: &'static str, args: serde_json::Value| {
        client.call_tool(
            CallToolRequestParams::new(name).with_arguments(args.as_object().unwrap().clone()),
        )
    };
    let listing = call("list_attachments", json!({"message_id":id}))
        .await
        .unwrap();
    assert_ne!(listing.is_error, Some(true), "{listing:?}");
    let metadata = listing.structured_content.unwrap();
    let records = metadata["attachments"].as_array().unwrap();
    assert_eq!(records.len(), 2);
    for record in records {
        let result = call(
            "get_attachment",
            json!({"message_id":id,"attachment_id":record["id"]}),
        )
        .await
        .unwrap();
        assert_ne!(result.is_error, Some(true), "{result:?}");
        if record["media_type"] == "image/png" {
            assert!(result.content.iter().any(|c| c.as_image().is_some()));
        } else {
            let embedded = result.content.iter().find_map(|c| c.as_resource()).unwrap();
            let rmcp::model::ResourceContents::BlobResourceContents { blob, .. } =
                &embedded.resource
            else {
                panic!("expected binary resource")
            };
            let text = String::from_utf8(STANDARD.decode(blob).unwrap()).unwrap();
            assert!(!text.contains("hunter2"));
            assert!(text.contains("useful notes"));
        }
    }
    let wrong = call(
        "get_attachment",
        json!({"message_id":id,"attachment_id":"../../etc/passwd"}),
    )
    .await
    .unwrap();
    assert_eq!(wrong.is_error, Some(true));
    let forgotten = call("forget", json!({"conversation_id":"session"}))
        .await
        .unwrap();
    assert_ne!(forgotten.is_error, Some(true));
    let removed = call(
        "get_attachment",
        json!({"message_id":id,"attachment_id":records[0]["id"]}),
    )
    .await
    .unwrap();
    assert_eq!(removed.is_error, Some(true));
    client.cancel().await.unwrap();
    server.waiting().await.unwrap();
}
