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
    let config = directory.join("config.toml");
    std::fs::write(&config, "version = 1\nproviders = {}\n").unwrap();
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
    assert_eq!(
        viewed.structured_content.unwrap()["messages"][0]["id"],
        "first-message"
    );
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
