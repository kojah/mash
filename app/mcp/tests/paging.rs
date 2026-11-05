use super::*;

fn seed(database: &std::path::Path) {
    let mut writer = crate::storage::Storage::open_writer(database).unwrap();
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
            updated_at: Some(5),
            messages: (0..5_u8)
                .map(|ordinal| crate::silver::NormalizedMessage {
                    id: format!("message-{ordinal}"),
                    ordinal: ordinal.into(),
                    context: crate::message_context::MessageContext::default(),
                    role: crate::role::Role::User,
                    content: format!(
                        "needle {} message {ordinal}",
                        "hay ".repeat(usize::from(5 - ordinal))
                    ),
                    dialogue_text: None,
                    created_at: Some(i64::from(ordinal) + 1),
                    attachments: Vec::new(),
                    tools: crate::tools::Activity::default(),
                })
                .collect::<Vec<_>>()
                .into(),
        })
        .unwrap();
    writer.finish_silver_scan(Provider::Codex, true).unwrap();
    writer.finish_silver_ingestion().unwrap();
    writer.commit_writer().unwrap();
}

fn ids(page: &serde_json::Value) -> Vec<String> {
    page["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hit| hit["id"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL and MASH_TEST_QDRANT_GRPC_URL"]
async fn sdk_search_pages_continue_from_next_offset() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("silver.sqlite3");
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        "version=1\nproviders={}\n[views.build]\nrepresentations='lexical-only'\n[views.query]\nintent='exact-mention'\nunit='messages'\n",
    )
    .unwrap();
    seed(&database);
    let (gold, url) = crate::views::local::test_support::lexical(&database, &config).await;
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
    let search = |arguments: serde_json::Value| {
        let mut request = json!({"query":"needle","local":true,"diagnostics":true});
        request
            .as_object_mut()
            .unwrap()
            .extend(arguments.as_object().unwrap().clone());
        client.call_tool(
            CallToolRequestParams::new("search")
                .with_arguments(request.as_object().unwrap().clone()),
        )
    };
    let whole = search(json!({})).await.unwrap().structured_content.unwrap();
    assert_eq!(whole["next_offset"], serde_json::Value::Null);
    let expected = ids(&whole);
    assert_eq!(expected.len(), 5);
    let mut paged = Vec::new();
    let mut offset = 0;
    loop {
        let page = search(json!({"limit":2, "offset":offset})).await.unwrap();
        assert_ne!(page.is_error, Some(true), "{page:?}");
        let readable = page.content[0].as_text().unwrap().text.clone();
        let page = page.structured_content.unwrap();
        assert_eq!(page["diagnostics"]["offset"], offset);
        assert_eq!(page["diagnostics"]["limit"], 2);
        paged.extend(ids(&page));
        let Some(next) = page["next_offset"].as_u64() else {
            assert!(!readable.contains("Next offset"));
            break;
        };
        assert!(readable.contains(&format!("Next offset: {next}")));
        offset = next;
    }
    assert_eq!(offset, 4, "the last page starts at the fifth result");
    assert_eq!(paged, expected);
    for invalid in [
        json!({"offset":50}),
        json!({"limit":0}),
        json!({"limit":201}),
    ] {
        let rejected = search(invalid).await.unwrap();
        assert_eq!(rejected.is_error, Some(true), "{rejected:?}");
        assert!(
            rejected.content[0]
                .as_text()
                .unwrap()
                .text
                .contains("search pages require")
        );
    }
    client.cancel().await.unwrap();
    service.cancel().await.unwrap();
    crate::views::local::test_support::cleanup(&gold, &url).await;
}
