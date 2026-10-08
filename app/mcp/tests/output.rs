use super::*;

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "one SDK session checks presentation and canonical evidence"
)]
async fn sdk_compact_and_extended_output_preserve_evidence_and_status() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("silver.sqlite3");
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        "version=1\nproviders={}\n[gold.build]\nrepresentations='lexical-only'\n[gold.query]\nintent='exact-mention'\n",
    )
    .unwrap();
    let text = format!("{}needle {}", "界 ".repeat(500), "終 ".repeat(400));
    let mut writer = crate::storage::Storage::open_writer(&database).unwrap();
    writer.begin_silver_scan(Provider::Codex).unwrap();
    writer
        .replace_conversation(&crate::silver::Conversation {
            id: "thread".into(),
            provider: Provider::Codex,
            source: crate::silver::SourceIdentity::file("fixture"),
            claude_lineage: None,
            title_origin: None,
            timestamp_origins: None,
            project: None,
            repository: None,
            title: Some("needle discussion".into()),
            created_at: Some(1),
            updated_at: Some(2),
            messages: vec![crate::silver::NormalizedMessage {
                id: "message".into(),
                ordinal: 0,
                context: crate::message_context::MessageContext::default(),
                role: crate::role::Role::User,
                content: text.clone(),
                dialogue_text: None,
                created_at: Some(1),
                attachments: Vec::new(),
            }]
            .into(),
        })
        .unwrap();
    writer.finish_silver_scan(Provider::Codex, true).unwrap();
    writer.finish_silver_ingestion().unwrap();
    writer.rebuild_derived_search_state().unwrap();
    writer.publish_lexical_manifest().unwrap();
    writer.commit_writer().unwrap();
    drop(writer);
    let server = Mash {
        watcher: Arc::default(),
        paths: Arc::new(Paths {
            config: Some(config),
            db: Some(database),
            models_dir: Some(directory.path().join("missing-models")),
        }),
        gate: Arc::default(),
        model_work: Arc::new(tokio::sync::Semaphore::new(0)),
    };
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let serving = tokio::spawn(async move { server.serve(server_io).await.unwrap() });
    let client = ().serve(client_io).await.unwrap();
    let service = serving.await.unwrap();
    let call = |name: &'static str, arguments: serde_json::Value| {
        client.call_tool(
            CallToolRequestParams::new(name).with_arguments(arguments.as_object().unwrap().clone()),
        )
    };
    let compact = call("search",json!({"query":"needle","intent":"exact-mention","unit":"messages","local":true,"diagnostics":true})).await.unwrap();
    assert_ne!(compact.is_error, Some(true), "{compact:?}");
    let compact = compact.structured_content.unwrap();
    let hit = &compact["results"][0];
    assert_eq!(hit["content_truncated"], true);
    assert!(hit["content"].as_str().unwrap().contains("needle"));
    assert!(compact["diagnostics"].is_object());
    let full = call("search",json!({"query":"needle","intent":"exact-mention","unit":"messages","local":true,"detail":"extended"})).await.unwrap().structured_content.unwrap();
    assert_eq!(hit["id"], full["results"][0]["id"]);
    assert_eq!(full["results"][0]["content"], text);
    assert!(full["results"][0].get("fusion_score").is_some());
    let view = call("view", json!({"id":hit["id"]}))
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(view["messages"][0]["content"], text);
    let listed = call("list_threads", json!({"local":true,"detail":"extended"}))
        .await
        .unwrap()
        .structured_content
        .unwrap();
    let compact_list = call("list_threads", json!({"local":true}))
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(listed["next_offset"], compact_list["next_offset"]);
    assert_eq!(
        listed["threads"][0]["conversation_id"],
        compact_list["threads"][0]["conversation_id"]
    );
    assert!(listed["threads"][0].get("enrichment").is_some());
    assert!(compact_list["threads"][0].get("enrichment").is_none());
    let status = call("status", json!({}))
        .await
        .unwrap()
        .structured_content
        .unwrap();
    let full_status = call("status", json!({"detail":"extended"}))
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(status["local_artifacts"], full_status["local_artifacts"]);
    assert_eq!(
        status["recommended_action"],
        full_status["recommended_action"]
    );
    assert!(status.get("configuration").is_none());
    assert!(full_status.get("configuration").is_some());
    let invalid = call("status", json!({"detail":"invented"})).await.unwrap();
    assert_eq!(invalid.is_error, Some(true));
    client.cancel().await.unwrap();
    service.cancel().await.unwrap();
}
