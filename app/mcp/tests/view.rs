use super::*;

fn chat(id: &str, messages: &[&str]) -> crate::silver::Chat {
    crate::silver::Chat {
        id: id.into(),
        provider: Provider::Codex,
        source: crate::silver::SourceIdentity::file(id),
        claude_lineage: None,
        title_origin: None,
        timestamp_origins: None,
        project: None,
        repository: None,
        title: Some(format!("{id} title")),
        created_at: Some(1),
        updated_at: Some(5),
        messages: messages
            .iter()
            .zip(0_i64..)
            .map(|(message, ordinal)| crate::silver::NormalizedMessage {
                id: (*message).into(),
                ordinal,
                context: crate::message_context::MessageContext::default(),
                role: crate::role::Role::User,
                content: format!("{message} text"),
                dialogue_text: None,
                created_at: Some(ordinal + 1),
                attachments: Vec::new(),
                tools: crate::tools::Activity::default(),
            })
            .collect::<Vec<_>>()
            .into(),
    }
}

async fn client(
    directory: &std::path::Path,
) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    client_with(directory, "version = 1\nproviders = {}\n").await
}

async fn client_with(
    directory: &std::path::Path,
    configuration: &str,
) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let config = directory.join("config.toml");
    std::fs::write(&config, configuration).unwrap();
    let database = directory.join("index.sqlite3");
    let mut writer = crate::storage::Storage::open_writer(&database).unwrap();
    writer.begin_silver_scan(Provider::Codex).unwrap();
    writer
        .replace_chat(&chat("log-rotation", &["first-message", "second-message"]))
        .unwrap();
    writer.replace_chat(&chat("empty-chat", &[])).unwrap();
    writer.finish_silver_scan(Provider::Codex, true).unwrap();
    writer.finish_silver_ingestion().unwrap();
    writer.commit_writer().unwrap();
    let server = Mash {
        runtime: None,
        paths: Arc::new(Paths {
            config: Some(config),
            db: Some(database),
            models_dir: Some(directory.join("models")),
        }),
        gate: Arc::default(),
        model_work: Arc::new(tokio::sync::Semaphore::new(1)),
    };
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move { server.serve(server_io).await.unwrap().waiting().await });
    ().serve(client_io).await.unwrap()
}

async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    tool: &'static str,
    arguments: serde_json::Value,
) -> rmcp::model::CallToolResult {
    client
        .call_tool(
            CallToolRequestParams::new(tool).with_arguments(arguments.as_object().unwrap().clone()),
        )
        .await
        .unwrap()
}

/// S-002: a listed chat carries the `view` arguments for its first message.
#[tokio::test]
async fn sdk_list_chats_rows_carry_a_first_message_view_lookup() {
    let directory = tempfile::tempdir().unwrap();
    let client = client(directory.path()).await;
    let listed = call(&client, "list_chats", json!({"local": true})).await;
    assert_ne!(listed.is_error, Some(true), "{listed:?}");
    let body = listed.structured_content.unwrap();
    let rows = body["chats"].as_array().unwrap();
    let row = |id: &str| rows.iter().find(|row| row["chat_id"] == id).unwrap();
    assert_eq!(
        row("log-rotation")["lookup"],
        json!({"view": {"id": "first-message"}})
    );
    assert!(row("log-rotation").get("first_message_id").is_none());
    assert!(row("empty-chat").get("lookup").is_none());
    let viewed = call(
        &client,
        "view",
        row("log-rotation")["lookup"]["view"].clone(),
    )
    .await;
    assert_ne!(viewed.is_error, Some(true), "{viewed:?}");
    // R2 S-003: the default window says the chat continues.
    let body = viewed.structured_content.clone().unwrap();
    assert_eq!(body["messages"][0]["id"], "first-message");
    assert_eq!(body["messages"].as_array().unwrap().len(), 1, "{body}");
    assert_eq!(body["chat_id"], "log-rotation", "{body}");
    assert_eq!(body["message_count"], 2, "{body}");
    assert_eq!(body["omitted_before"], 0, "{body}");
    assert_eq!(body["omitted_after"], 1, "{body}");
    let notice = body["notice"].as_str().unwrap();
    assert!(
        notice.starts_with("Showing 1 of this chat's 2 messages; 1 later message is not shown."),
        "{notice}"
    );
    let text = viewed
        .content
        .iter()
        .filter_map(|content| content.as_text())
        .map(|text| text.text.as_str())
        .collect::<String>();
    assert!(text.contains("omitted after: 1"), "{text}");
    assert!(text.contains("Notice: Showing 1 of"), "{text}");
    let whole = call(
        &client,
        "view",
        json!({"id": "second-message", "context": 1}),
    )
    .await;
    let whole = whole.structured_content.unwrap();
    assert_eq!(whole["messages"].as_array().unwrap().len(), 2, "{whole}");
    assert_eq!(whole["omitted_before"], 0, "{whole}");
    assert_eq!(whole["omitted_after"], 0, "{whole}");
    assert!(whole.get("notice").is_none(), "{whole}");
    let extended = call(
        &client,
        "list_chats",
        json!({"local": true, "detail": "extended"}),
    )
    .await;
    let extended = extended.structured_content.unwrap();
    assert!(
        extended["chats"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["lookup"] == json!({"view": {"id": "first-message"}}))
    );
}

/// S-001: chat and unknown IDs are typed not-found errors, never empty success.
#[tokio::test]
async fn sdk_view_rejects_chat_and_unknown_ids_as_not_found() {
    let directory = tempfile::tempdir().unwrap();
    let client = client(directory.path()).await;
    for (id, expected) in [
        ("log-rotation", "\"first-message\""),
        ("empty-chat", "retains no messages"),
        ("no-such-thing", "not a chat ID"),
    ] {
        let result = call(&client, "view", json!({"id": id, "context": 10})).await;
        assert_eq!(result.is_error, Some(true), "{id}: {result:?}");
        let error = &result.structured_content.unwrap()["error"];
        assert_eq!(error["kind"], "not-found", "{id}");
        assert_eq!(error["retryable"], false, "{id}");
        let message = error["message"].as_str().unwrap();
        assert!(message.contains(expected), "{id}: {message}");
    }
}

/// B-010: search fails closed without Qdrant, but says the saved history is
/// still readable through `list_chats` and `view`, which keep working.
#[tokio::test]
async fn sdk_search_without_qdrant_names_the_saved_history_and_working_tools() {
    let directory = tempfile::tempdir().unwrap();
    let client = client_with(
        directory.path(),
        concat!(
            "version = 1\nproviders = {}\n",
            "[views.build]\nrepresentations = 'lexical-only'\n",
            "[views.query]\nintent = 'exact-mention'\n",
            "[qdrant]\nurl = 'http://127.0.0.1:9'\ngrpc_url = 'http://127.0.0.1:9'\n",
        ),
    )
    .await;
    let searched = call(&client, "search", json!({"query": "rotation"})).await;
    assert_eq!(searched.is_error, Some(true), "{searched:?}");
    let error = searched.structured_content.unwrap();
    assert_eq!(error["error"]["kind"], "qdrant-unavailable", "{error}");
    assert_eq!(error["error"]["recommended_action"], "start-qdrant");
    let message = error["error"]["message"].as_str().unwrap();
    for part in [
        "Could not reach Qdrant at http://127.0.0.1:9/",
        "Your 2 indexed chats are saved on this machine",
        "MCP `list_chats` and `view`",
        "Then search again.",
        "Guide: https://getmash.io/installation/#start-the-local-search-service",
    ] {
        assert!(message.contains(part), "{message}");
    }
    assert_eq!(message.matches("Then ").count(), 1, "{message}");
    let listed = call(&client, "list_chats", json!({"local": true})).await;
    assert_ne!(listed.is_error, Some(true), "{listed:?}");
    let viewed = call(&client, "view", json!({"id": "first-message"})).await;
    assert_ne!(viewed.is_error, Some(true), "{viewed:?}");
}

/// P1 S-002: configured but not indexed yet, `list_chats` and search point to
/// `mash index` and the Qdrant it needs, not back to `mash init`.
#[tokio::test]
async fn sdk_reads_before_the_first_index_name_index_and_qdrant() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        "version = 1\nproviders = {}\n[qdrant]\nurl = 'http://127.0.0.1:9'\ngrpc_url = 'http://127.0.0.1:9'\n",
    )
    .unwrap();
    let server = Mash {
        runtime: None,
        paths: Arc::new(Paths {
            config: Some(config),
            db: Some(directory.path().join("index.sqlite3")),
            models_dir: Some(directory.path().join("models")),
        }),
        gate: Arc::default(),
        model_work: Arc::new(tokio::sync::Semaphore::new(1)),
    };
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move { server.serve(server_io).await.unwrap().waiting().await });
    let client = ().serve(client_io).await.unwrap();
    for (tool, arguments) in [
        ("list_chats", json!({"local": true})),
        (
            "search",
            json!({"query": "rotation", "intent": "exact-mention"}),
        ),
    ] {
        let result = call(&client, tool, arguments).await;
        assert_eq!(result.is_error, Some(true), "{tool}: {result:?}");
        let error = &result.structured_content.unwrap()["error"];
        assert_eq!(error["kind"], "database-missing", "{tool}: {error}");
        assert_eq!(error["recommended_action"], "index", "{tool}: {error}");
        let message = error["message"].as_str().unwrap();
        for part in [
            "Mash is configured, but nothing is indexed yet",
            "Run `mash index`",
            "needs Qdrant at http://127.0.0.1:9",
        ] {
            assert!(message.contains(part), "{tool}: {message}");
        }
        assert!(!message.contains("mash init"), "{tool}: {message}");
    }
}
