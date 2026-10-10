use super::*;
use crate::test_support::create_valid_model_marker as install_test_marker;
use rmcp::model::CallToolRequestParams;
use serde_json::json;

mod attachments;
mod chats;
mod coverage;
mod export;
mod fusion;
mod incomplete;
mod output;
mod rebuild;
mod redaction;
mod selections;
mod skill;
mod watching;
mod workflows;

#[tokio::test]
async fn exact_mcp_search_bypasses_model_permits_and_preserves_completed_silver() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("index.sqlite3");
    let mut writer = crate::storage::Storage::open_writer(&database).unwrap();
    writer.finish_silver_ingestion().unwrap();
    writer.rebuild_derived_search_state().unwrap();
    writer.publish_lexical_manifest().unwrap();
    let certificate = writer.silver_certificate().unwrap();
    writer.commit_writer().unwrap();
    drop(writer);
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        "version=1\nproviders={}\n[views.query]\nintent='exact-mention'\n",
    )
    .unwrap();
    let server = Mash {
        watcher: Arc::default(),
        paths: Arc::new(Paths {
            config: Some(config),
            db: Some(database.clone()),
            models_dir: Some(directory.path().join("missing-models")),
        }),
        gate: Arc::new(tokio::sync::Mutex::new(())),
        model_work: Arc::new(tokio::sync::Semaphore::new(0)),
    };
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        server.search(Parameters(Search {
            detail: Detail::Extended,
            query: "for".into(),
            intent: None,
            limit: Some(1),
            fusion: None,
            ranking: None,
            shared_vector: None,
            diagnostics: true,
            unit: Some(crate::search_results::SearchUnit::Messages),
            filters: crate::search_filter::Selection {
                provider: vec![crate::provider::Provider::Codex],
                role: vec![crate::role::Role::User],
                filter: Some(crate::search_filter::Filter::Not(Box::new(
                    crate::search_filter::Filter::Project(vec!["private-filter-canary".into()]),
                ))),
                ..Default::default()
            },
            shared: false,
            local: true,
        })),
    )
    .await
    .unwrap();
    assert_ne!(result.is_error, Some(true));
    let data = result.structured_content.unwrap();
    assert_eq!(data["intent"], "exact-mention");
    assert_eq!(data["realized_mode"], "lexical");
    assert_eq!(data["results"], json!([]));
    assert_eq!(data["diagnostics"]["intent"], "exact-mention");
    assert_eq!(
        data["diagnostics"]["selected_artifacts"],
        json!(["metadata", "lexical"])
    );
    assert!(data["diagnostics"].get("fusion").is_none());
    assert_eq!(
        data["diagnostics"]["filter_fields"],
        json!(["provider", "role", "project"])
    );
    assert!(
        !serde_json::to_string(&data["diagnostics"])
            .unwrap()
            .contains("private-filter-canary")
    );
    let stages = data["diagnostics"]["stages"].as_array().unwrap();
    assert_eq!(stages.len(), 2);
    assert_eq!(stages[0]["stage"], "local-exact");
    assert_eq!(stages[0]["items_out"], 0);
    assert_eq!(stages[0]["filters_before_limit"], true);
    assert_eq!(stages[1]["stage"], "fusion");
    assert_eq!(stages[1]["items_out"], 0);
    assert_eq!(stages[1]["filters_before_limit"], false);
    assert_eq!(
        crate::storage::Storage::open_catalog(&database)
            .unwrap()
            .silver_certificate()
            .unwrap(),
        certificate
    );
}

#[tokio::test]
async fn typed_search_requests_validate_before_touching_models_or_storage() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(&config, "version = 1\nproviders = {}\n").unwrap();
    let database = dir.path().join("missing.sqlite3");
    let models = dir.path().join("missing-models");
    let server = Mash {
        watcher: Arc::default(),
        paths: Arc::new(Paths {
            config: Some(config),
            db: Some(database.clone()),
            models_dir: Some(models.clone()),
        }),
        gate: Arc::new(tokio::sync::Mutex::new(())),
        model_work: Arc::new(tokio::sync::Semaphore::new(1)),
    };
    let invalid_filter = crate::search_filter::Filter::Project(vec![String::new()]);
    for (local, shared, filter) in [(true, true, None), (true, false, Some(invalid_filter))] {
        let result = server
            .search(Parameters(Search {
                detail: Detail::Extended,
                query: "--models-dir=untrusted".into(),
                intent: None,
                limit: None,
                fusion: None,
                ranking: None,
                shared_vector: None,
                diagnostics: false,
                unit: Some(crate::search_results::SearchUnit::Chats),
                filters: crate::search_filter::Selection {
                    provider: vec![Provider::Codex],
                    filter,
                    ..Default::default()
                },
                local,
                shared,
            }))
            .await;
        assert_eq!(result.is_error, Some(true));
        assert_eq!(result.structured_content.unwrap()["error"]["kind"], "usage");
    }
    let result = server
        .search(Parameters(Search {
            detail: Detail::Extended,
            query: "wie konfiguriere ich den gemeinsamen Suchindex".into(),
            intent: None,
            limit: None,
            fusion: None,
            ranking: None,
            shared_vector: None,
            diagnostics: false,
            unit: Some(crate::search_results::SearchUnit::Chats),
            filters: crate::search_filter::Selection::default(),
            local: true,
            shared: false,
        }))
        .await;
    assert_eq!(result.is_error, Some(true));
    let error = result.structured_content.unwrap();
    assert_eq!(error["error"]["kind"], "usage");
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("German")
    );
    assert!(!database.exists());
    assert!(!models.exists());
}

async fn wire_send(
    writer: &mut tokio::io::WriteHalf<tokio::io::DuplexStream>,
    value: serde_json::Value,
) {
    use tokio::io::AsyncWriteExt;
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    writer.write_all(&bytes).await.unwrap();
}

async fn wire_next(
    reader: &mut tokio::io::BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
) -> serde_json::Value {
    use tokio::io::AsyncBufReadExt;
    let mut line = String::new();
    tokio::time::timeout(
        // Full-suite SQLite/inference contention can delay a wire response;
        // this deadline detects a stalled session, not product latency.
        std::time::Duration::from_secs(15),
        reader.read_line(&mut line),
    )
    .await
    .unwrap()
    .unwrap();
    serde_json::from_str(&line).unwrap()
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "wire-level handshake, live progress, token omission, and failure form one session"
)]
async fn indexing_streams_correlated_progress_before_the_tool_result() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let database = dir.path().join("index.sqlite3");
    let models = dir.path().join("models");
    std::fs::write(&config, "version = 1\nproviders = {}\n").unwrap();
    install_test_marker(&models);
    let server = Mash {
        watcher: Arc::default(),
        paths: Arc::new(Paths {
            config: Some(config),
            db: Some(database.clone()),
            models_dir: Some(models.clone()),
        }),
        gate: Arc::new(tokio::sync::Mutex::new(())),
        model_work: Arc::new(tokio::sync::Semaphore::new(1)),
    };
    let gate = server.gate.clone();
    let held = gate.lock().await;
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let serving = tokio::spawn(async move { server.serve(server_io).await.unwrap() });
    let (read, mut write) = tokio::io::split(client_io);
    let mut read = tokio::io::BufReader::new(read);
    // Use the wire directly: rmcp's client replaces caller-supplied tokens
    // and adds one even when none was requested.
    wire_send(&mut write, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"progress-test","version":"1"}}})).await;
    assert_eq!(wire_next(&mut read).await["id"], 1);
    wire_send(
        &mut write,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )
    .await;
    let server = serving.await.unwrap();
    wire_send(&mut write, json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"index","arguments":{},"_meta":{"progressToken":"index-one"}}})).await;
    let waiting = wire_next(&mut read).await;
    assert_eq!(waiting["method"], "notifications/progress");
    assert_eq!(waiting["params"]["progressToken"], "index-one");
    assert_eq!(
        waiting["params"]["_meta"]["dev.jamesqo.mash/index-progress"]["phase"],
        "waiting-for-index"
    );
    assert!(
        !database.exists(),
        "progress arrives while indexing is still queued"
    );
    drop(held);
    let mut previous = waiting["params"]["progress"].as_f64().unwrap();
    let mut complete = false;
    loop {
        let message = wire_next(&mut read).await;
        if message.get("id").is_some() {
            assert_eq!(message["id"], 2);
            assert_ne!(message["result"]["isError"], true);
            assert_eq!(
                message["result"]["structuredContent"]["indexed_messages"],
                0
            );
            break;
        }
        assert_eq!(message["method"], "notifications/progress");
        let event: rmcp::model::ProgressNotificationParam =
            serde_json::from_value(message["params"].clone()).unwrap();
        assert_eq!(
            event.progress_token,
            rmcp::model::ProgressToken(rmcp::model::NumberOrString::String("index-one".into()))
        );
        assert!(event.progress > previous);
        assert!(
            event.total.is_none(),
            "phase totals are not an overall total"
        );
        previous = event.progress;
        let metadata: crate::progress::Event =
            serde_json::from_value(event.meta.unwrap()["dev.jamesqo.mash/index-progress"].clone())
                .unwrap();
        if metadata.phase == crate::progress::Phase::Complete {
            complete = true;
            assert_eq!(metadata.eta_seconds, Some(0));
            assert!(event.message.unwrap().contains("ETA 0m 00s"));
        }
    }
    assert!(complete, "completion progress precedes the tool result");
    wire_send(&mut write, json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"index","arguments":{}}})).await;
    let result = wire_next(&mut read).await;
    assert_eq!(result["id"], 3, "no token means no unsolicited progress");
    assert_ne!(result["result"]["isError"], true);
    std::fs::remove_file(models.join("dummy-model")).unwrap();
    wire_send(&mut write, json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"index","arguments":{},"_meta":{"progressToken":42}}})).await;
    let event = wire_next(&mut read).await;
    assert_eq!(event["params"]["progressToken"], 42);
    assert_eq!(event["params"]["progress"], 1.0);
    let result = wire_next(&mut read).await;
    assert_eq!(result["id"], 4);
    assert_eq!(result["result"]["isError"], true);
    assert_eq!(
        result["result"]["structuredContent"]["error"]["recommended_action"],
        "models install"
    );
    drop(write);
    drop(read);
    server.waiting().await.unwrap();
}

fn assert_filter_schema(tools: &[rmcp::model::Tool]) {
    let search = tools.iter().find(|tool| tool.name == "search").unwrap();
    assert!(search.input_schema["properties"].get("filter").is_some());
    for field in ["provider", "chat", "node", "role", "project", "repository"] {
        assert_eq!(search.input_schema["properties"][field]["type"], "array");
    }
    // Defaults come from the current node configuration, not a frozen SDK value.
    let unit = &search.input_schema["properties"]["unit"];
    assert!(unit.get("default").is_none());
    assert!(
        !search.input_schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field == "unit"
                || field == "limit"
                || field == "fusion"
                || field == "diagnostics")
    );
    let schema = serde_json::to_string(&search.input_schema).unwrap();
    assert!(schema.contains("\"chats\"") && schema.contains("\"messages\""));
    assert!(
        schema.contains("rank_constant")
            && schema.contains("lexical_weight")
            && schema.contains("semantic_weight")
    );
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "exercise a complete SDK session including explicit index readiness"
)]
async fn sdk_handshake_discovery_calls_and_errors_share_cli_contract() {
    let dir = tempfile::tempdir().expect("temp");
    let config = dir.path().join("config.toml");
    std::fs::write(
        &config,
        r#""version" = 1
"providers" = {}
"#,
    )
    .expect("config");
    let server = Mash {
        watcher: Arc::default(),
        paths: Arc::new(Paths {
            config: Some(config),
            db: Some(dir.path().join("missing.sqlite3")),
            models_dir: Some(dir.path().join("models")),
        }),
        gate: Arc::new(tokio::sync::Mutex::new(())),
        model_work: Arc::new(tokio::sync::Semaphore::new(1)),
    };
    let write_gate = server.gate.clone();
    let model_work = server.model_work.clone();
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let server = tokio::spawn(async move { server.serve(server_io).await.expect("server") });
    let client = ().serve(client_io).await.expect("SDK client initialization");
    let server = server.await.expect("server task");
    let tools = client.list_all_tools().await.expect("tools/list");
    assert_filter_schema(&tools);
    let threads = tools.iter().find(|tool| tool.name == "list_chats").unwrap();
    let properties = &threads.input_schema["properties"];
    assert_eq!(properties["limit"]["minimum"], 1);
    assert_eq!(properties["limit"]["maximum"], 100);
    assert_eq!(properties["offset"]["maximum"], 10_000);
    let mut names = tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<Vec<_>>();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "export_silver",
            "forget",
            "get_attachment",
            "import_silver",
            "index",
            "list_attachments",
            "list_chats",
            "models_install",
            "search",
            "status",
            "view",
            "watch_start",
            "watch_status",
            "watch_stop"
        ]
    );
    let write_guard = write_gate.lock().await;
    let model_permit = model_work.acquire().await.expect("model permit");
    let status = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client.call_tool(
            CallToolRequestParams::new("status")
                .with_arguments(json!({"detail":"extended"}).as_object().unwrap().clone()),
        ),
    )
    .await
    .expect("status remains responsive during a mutation and model work")
    .expect("status");
    drop(model_permit);
    drop(write_guard);
    assert_ne!(status.is_error, Some(true));
    let value = serde_json::to_value(&status).expect("serialize");
    assert!(
        value["content"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("Mash status")
    );
    let body = status.structured_content.as_ref().unwrap().clone();
    assert_eq!(body["ready"], false);
    assert_eq!(
        body["local_artifacts"],
        json!({"metadata":"missing","lexical":"missing","dense":"missing"})
    );
    assert_eq!(status.structured_content.as_ref(), Some(&body));
    assert_eq!(body["configuration"]["loaded"], true);
    let args = json!({
        "query":"--models-dir=untrusted", "shared":false,
        "unit":"messages", "provider":["codex", "claude-code"],
        "chat":["thread-a", "thread-b"], "after":2, "before":3
    })
    .as_object()
    .expect("object")
    .clone();
    let result = client
        .call_tool(CallToolRequestParams::new("search").with_arguments(args))
        .await
        .expect("tools/call");
    assert_eq!(result.is_error, Some(true));
    let value = serde_json::to_value(result).expect("serialize");
    assert!(
        value["content"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("Mash error")
    );
    let body = &value["structuredContent"];
    assert_eq!(
        body["error"]["kind"], "model",
        "query cannot inject CLI flags"
    );
    // Valid installation metadata with unusable inference assets proves
    // search checks the missing index before trying to load a model.
    let models = dir.path().join("models");
    install_test_marker(&models);
    let missing = client
        .call_tool(
            CallToolRequestParams::new("search")
                .with_arguments(json!({"query":"needle"}).as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    assert_eq!(missing.is_error, Some(true));
    assert_eq!(
        missing.structured_content.as_ref().unwrap()["error"]["recommended_action"],
        "index"
    );
    assert!(!dir.path().join("missing.sqlite3").exists());
    let args = json!({"query":"q", "unexpected":true})
        .as_object()
        .expect("object")
        .clone();
    let invalid = client
        .call_tool(CallToolRequestParams::new("search").with_arguments(args))
        .await
        .expect("invalid argument result");
    assert_eq!(invalid.is_error, Some(true));
    assert!(
        client
            .call_tool(CallToolRequestParams::new("unknown"))
            .await
            .is_err()
    );
    client.cancel().await.expect("client shutdown");
    server.waiting().await.expect("server shutdown");
    assert!(!dir.path().join("missing.sqlite3").exists());
}
