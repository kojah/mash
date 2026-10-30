use super::*;

#[test]
fn database_lock_error_preserves_evidence_in_mcp_text_and_structure() {
    for detail in [Detail::Compact, Detail::Extended] {
        let failure = crate::AppError::database_at(crate::database_error::Operation::InspectStatus)(
            rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(517),
                Some("database snapshot is busy".into()),
            ),
        );
        let result = tool_result_detailed(Err(failure), detail);
        assert_eq!(result.is_error, Some(true));
        let data = result.structured_content.unwrap();
        assert_eq!(data["error"]["database"]["operation"], "inspect-status");
        assert_eq!(data["error"]["database"]["sqlite_extended_code"], 517);
        let text = &result.content[0].as_text().unwrap().text;
        assert!(text.contains("database snapshot is busy"));
        assert!(text.contains("517"));
    }
}

fn presentation_bytes(response: &CallToolResult) {
    eprintln!(
        "MCP presentation bytes: text={}, structured={}, whole_result={}",
        response.content[0].as_text().unwrap().text.len(),
        response
            .structured_content
            .as_ref()
            .unwrap()
            .to_string()
            .len(),
        serde_json::to_vec(response).unwrap().len(),
    );
}

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
        "version=1\nproviders={}\n[views.build]\nrepresentations='lexical-only'\n[views.query]\nintent='exact-mention'\n",
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
                tools: crate::tools::Activity::default(),
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
        runtime: None,
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
    let readable = compact.content[0].as_text().unwrap().text.clone();
    assert!(readable.starts_with("Mash search"));
    assert!(readable.contains("Excerpt truncated"));
    presentation_bytes(&compact);
    let text_lookup: serde_json::Value = serde_json::from_str(
        readable
            .lines()
            .find_map(|line| line.strip_prefix("  lookup.view: "))
            .unwrap(),
    )
    .unwrap();
    let compact = compact.structured_content.unwrap();
    let hit = &compact["results"][0];
    assert_eq!(hit["content_truncated"], true);
    assert!(hit["content"].as_str().unwrap().contains("needle"));
    assert!(compact["diagnostics"].is_object());
    let full = call("search",json!({"query":"needle","intent":"exact-mention","unit":"messages","local":true,"detail":"extended"})).await.unwrap();
    assert!(
        full.content[0]
            .as_text()
            .unwrap()
            .text
            .ends_with(&format!("{}\n", full.structured_content.as_ref().unwrap()))
    );
    let full = full.structured_content.unwrap();
    assert_eq!(hit["id"], full["results"][0]["id"]);
    assert_eq!(full["results"][0]["content"], text);
    assert!(full["results"][0].get("fusion_score").is_some());
    assert_eq!(hit["lookup"], full["results"][0]["lookup"]);
    assert!(hit["lookup"]["view"].get("shared").is_none());
    assert_eq!(text_lookup, hit["lookup"]["view"]);
    let view = call("view", text_lookup).await.unwrap();
    assert!(
        view.content[0]
            .as_text()
            .unwrap()
            .text
            .starts_with("Mash view")
    );
    assert!(view.content[0].as_text().unwrap().text.contains(&text));
    let view = view.structured_content.unwrap();
    assert_eq!(view["messages"][0]["content"], text);
    let attachments = call(
        "list_attachments",
        hit["lookup"]["list_attachments"].clone(),
    )
    .await
    .unwrap();
    assert_ne!(attachments.is_error, Some(true));
    assert!(
        attachments.content[0]
            .as_text()
            .unwrap()
            .text
            .starts_with("Mash attachments")
    );
    assert_eq!(
        attachments.structured_content.unwrap()["message_id"],
        hit["id"]
    );
    let threads = call(
        "search",
        json!({"query":"needle", "intent":"exact-mention", "local":true, "unit":"chats"}),
    )
    .await
    .unwrap()
    .structured_content
    .unwrap();
    let lookup = &threads["results"][0]["matches"][0]["lookup"];
    assert_eq!(lookup, &hit["lookup"]);
    let anchored = call("view", lookup["view"].clone()).await.unwrap();
    assert_eq!(
        anchored.structured_content.unwrap()["messages"][0]["content"],
        text
    );
    let listed = call("list_chats", json!({"local":true,"detail":"extended"}))
        .await
        .unwrap()
        .structured_content
        .unwrap();
    let compact_list = call("list_chats", json!({"local":true})).await.unwrap();
    assert!(
        compact_list.content[0]
            .as_text()
            .unwrap()
            .text
            .starts_with("Mash chats")
    );
    presentation_bytes(&compact_list);
    let compact_list = compact_list.structured_content.unwrap();
    assert_eq!(listed["next_offset"], compact_list["next_offset"]);
    assert_eq!(
        listed["chats"][0]["chat_id"],
        compact_list["chats"][0]["chat_id"]
    );
    assert!(listed["chats"][0].get("enrichment").is_some());
    assert!(compact_list["chats"][0].get("enrichment").is_none());
    let status = call("status", json!({})).await.unwrap();
    assert!(
        status.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("local search ready")
    );
    presentation_bytes(&status);
    let status = status.structured_content.unwrap();
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
