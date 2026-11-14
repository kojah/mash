//! Scripted agent workflows: discovery, scoped search, canonical follow-ups.
use super::*;

pub(super) fn decision_history(root: &std::path::Path) -> Paths {
    let database = root.join("silver.sqlite3");
    let config = root.join("config.toml");
    std::fs::write(
        &config,
        "version=1\nproviders={}\n[views.build]\nrepresentations='lexical-only'\n[views.query]\nintent='exact-mention'\n",
    )
    .unwrap();
    let mut writer = crate::storage::Storage::open_writer(&database).unwrap();
    writer.begin_silver_scan(Provider::Codex).unwrap();
    let messages = [
        (
            crate::role::Role::User,
            "Which shared backend should we choose?",
        ),
        (
            crate::role::Role::Assistant,
            "We could use Qdrant for the shared index.",
        ),
        (crate::role::Role::User, "Yes, let's use Qdrant."),
        (
            crate::role::Role::Assistant,
            "I will switch the shared backend.",
        ),
    ]
    .into_iter()
    .enumerate()
    .map(
        |(ordinal, (role, content))| crate::silver::NormalizedMessage {
            id: format!("message-{ordinal}"),
            ordinal: i64::try_from(ordinal).unwrap(),
            context: crate::message_context::MessageContext::Linear,
            role,
            content: content.into(),
            dialogue_text: None,
            created_at: Some(1000),
            attachments: vec![],
            tools: crate::tools::Activity::default(),
        },
    )
    .collect::<Vec<_>>();
    writer
        .replace_chat(&crate::silver::Chat {
            id: "decision-thread".into(),
            provider: Provider::Codex,
            source: crate::silver::SourceIdentity::file("fixture"),
            claude_lineage: None,
            title_origin: None,
            timestamp_origins: None,
            project: None,
            repository: None,
            title: Some("Shared backend choice".into()),
            created_at: Some(1000),
            updated_at: Some(1000),
            messages: messages.into(),
        })
        .unwrap();
    writer.finish_silver_scan(Provider::Codex, true).unwrap();
    writer.finish_silver_ingestion().unwrap();
    writer.commit_writer().unwrap();
    Paths {
        config: Some(config),
        db: Some(database),
        models_dir: Some(root.join("missing-models")),
    }
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL and MASH_TEST_QDRANT_GRPC_URL"]
async fn sdk_decision_workflow_discovers_scopes_and_reads_user_approval_in_four_calls() {
    let root = tempfile::tempdir().unwrap();
    let paths = decision_history(root.path());
    let database = paths.db.clone().unwrap();
    let (gold, url) =
        crate::views::local::test_support::lexical(&database, paths.config.as_deref().unwrap())
            .await;
    let before = crate::storage::Storage::open_silver(&database)
        .unwrap()
        .silver_certificate()
        .unwrap();
    let server = Mash {
        paths: Arc::new(paths),
        gate: Arc::default(),
        runtime: None,
        model_work: Arc::new(tokio::sync::Semaphore::new(0)),
    };
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let serving = tokio::spawn(async move { server.serve(server_io).await.unwrap() });
    let client = ().serve(client_io).await.unwrap();
    let service = serving.await.unwrap();
    let call = |name: &'static str, args: serde_json::Value| {
        client.call_tool(
            CallToolRequestParams::new(name).with_arguments(args.as_object().unwrap().clone()),
        )
    };
    let mut responses = Vec::new();
    let threads = call(
        "list_chats",
        json!({"title_contains":"backend choice", "local":true, "limit":5}),
    )
    .await
    .unwrap();
    assert_ne!(threads.is_error, Some(true), "{threads:?}");
    let threads = threads.structured_content.unwrap();
    let thread = threads["chats"][0]["chat_id"].clone();
    responses.push(threads);
    let found = call(
        "search",
        json!({"query":"Qdrant", "intent":"exact-mention", "match_mode":"strict",
        "unit":"messages", "chat":[thread], "role":["user"], "local":true, "limit":5}),
    )
    .await
    .unwrap();
    assert_ne!(found.is_error, Some(true));
    let found = found.structured_content.unwrap();
    assert_eq!(found["results"].as_array().unwrap().len(), 1);
    let lookup = found["results"][0]["lookup"].clone();
    responses.push(found);
    let mut args = lookup["view"].clone();
    args["context"] = json!(1);
    let evidence = call("view", args).await.unwrap();
    assert_ne!(evidence.is_error, Some(true));
    let evidence = evidence.structured_content.unwrap();
    let messages = evidence["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0]["role"], "assistant");
    assert!(
        messages[0]["content"]
            .as_str()
            .unwrap()
            .starts_with("We could")
    );
    assert_eq!(messages[1]["role"], "user");
    assert_eq!(messages[1]["content"], "Yes, let's use Qdrant.");
    responses.push(evidence);
    let attachments = call("list_attachments", lookup["list_attachments"].clone())
        .await
        .unwrap();
    assert_ne!(attachments.is_error, Some(true));
    responses.push(attachments.structured_content.unwrap());
    assert_eq!(responses.len(), 4);
    let bytes: usize = responses
        .iter()
        .map(|value| serde_json::to_vec(value).unwrap().len())
        .sum();
    assert!(
        bytes < 14_000,
        "bounded discovery and evidence: {bytes} bytes"
    );
    let after = crate::storage::Storage::open_silver(&database)
        .unwrap()
        .silver_certificate()
        .unwrap();
    assert_eq!(before, after, "read workflows must not ingest");
    client.cancel().await.unwrap();
    service.cancel().await.unwrap();
    crate::views::local::test_support::cleanup(&gold, &url).await;
}
