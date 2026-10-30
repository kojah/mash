use super::*;
use crate::progress::Phase;
use serde::Deserialize;

#[derive(Deserialize)]
struct Summary {
    malformed_records: u64,
    record_diagnostics: crate::silver::diagnostics::Diagnostics,
}

fn assert_diagnostics(
    diagnostics: &crate::silver::diagnostics::Diagnostics,
    source: &std::path::Path,
    malformed: u64,
) {
    assert_eq!(u64::try_from(diagnostics.samples.len()).unwrap(), malformed);
    if malformed > 0 {
        let issue = &diagnostics.samples[0];
        assert_eq!(issue.source, source);
        assert_eq!(issue.line, 2);
        assert_eq!(issue.provider, crate::provider::Provider::ClaudeCode);
        assert_eq!(issue.category, crate::silver::diagnostics::Category::Data);
    }
}

#[tokio::test]
async fn unavailable_shared_publication_is_a_retryable_mcp_result_with_pending_progress() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(
        &config,
        concat!(
            "version = 1\nproviders = {}\n[index]\ndestination = 'both'\n",
            "[shared]\ngroup_id = 'group'\nnode_id = 'node'\n",
            "api_url = 'http://127.0.0.1:9'\ntoken_env = 'HOME'\n",
        ),
    )
    .unwrap();
    let models = dir.path().join("models");
    install_test_marker(&models);
    let server = Mash {
        runtime: None,
        paths: Arc::new(Paths {
            config: Some(config),
            db: Some(dir.path().join("index.sqlite3")),
            models_dir: Some(models),
        }),
        gate: Arc::new(tokio::sync::Mutex::new(())),
        model_work: Arc::new(tokio::sync::Semaphore::new(1)),
    };
    let (sender, mut events) = tokio::sync::mpsc::channel(64);
    let result = crate::progress::scope(
        sender,
        server.perform(Operation::Index(core::IndexRequest::default())),
    )
    .await;
    assert_eq!(result.is_error, Some(true));
    let text = &result.content[0].as_text().unwrap().text;
    assert!(text.contains("retry required: true"));
    assert!(text.contains("Shared publication: \"pending\""));
    let summary: Summary = serde_json::from_value(result.structured_content.unwrap()).unwrap();
    assert_eq!(summary.malformed_records, 0);
    let mut last = None;
    while let Ok(event) = events.try_recv() {
        assert_ne!(event.phase, Phase::Complete);
        last = Some(event);
    }
    let last = last.unwrap();
    assert_eq!(last.phase, Phase::SharedPending);
    assert_eq!(last.eta_seconds, None);
    assert_eq!(last.malformed_records, Some(0));
    assert!(crate::progress::display(&last).contains("retry required"));
}

#[tokio::test]
async fn partial_indexing_streams_retry_status_before_mcp_error_and_recovers() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("broken.jsonl");
    std::fs::write(
        &source,
        "\r\n{\"sessionId\":[\"PRIVATE-MCP-RECORD-CANARY\"]}\r\n",
    )
    .unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(
        &config,
        format!(
            "version = 1\n[providers.claude-code]\nroots = [{:?}]\n",
            dir.path()
        ),
    )
    .unwrap();
    let models = dir.path().join("models");
    install_test_marker(&models);
    let server = Mash {
        runtime: None,
        paths: Arc::new(Paths {
            config: Some(config),
            db: Some(dir.path().join("index.sqlite3")),
            models_dir: Some(models),
        }),
        gate: Arc::new(tokio::sync::Mutex::new(())),
        model_work: Arc::new(tokio::sync::Semaphore::new(1)),
    };
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let serving = tokio::spawn(async move { server.serve(server_io).await.unwrap() });
    let (read, mut write) = tokio::io::split(client_io);
    let mut read = tokio::io::BufReader::new(read);
    wire_send(&mut write, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"partial-test","version":"1"}}})).await;
    assert_eq!(wire_next(&mut read).await["id"], 1);
    wire_send(
        &mut write,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )
    .await;
    let server = serving.await.unwrap();
    for (id, phase, malformed) in [(2, Phase::Incomplete, 1), (3, Phase::Complete, 0)] {
        wire_send(&mut write, json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"index","arguments":{},"_meta":{"progressToken":id}}})).await;
        let mut terminal = None;
        loop {
            let message = wire_next(&mut read).await;
            assert!(!message.to_string().contains("PRIVATE-MCP-RECORD-CANARY"));
            if message.get("id").is_some() {
                assert_eq!(message["id"], id);
                let result: CallToolResult =
                    serde_json::from_value(message["result"].clone()).unwrap();
                assert_eq!(result.is_error.unwrap_or(false), malformed > 0);
                let summary: Summary =
                    serde_json::from_value(result.structured_content.unwrap()).unwrap();
                assert_eq!(summary.malformed_records, malformed);
                assert_diagnostics(&summary.record_diagnostics, &source, malformed);
                break;
            }
            let notification: rmcp::model::ProgressNotificationParam =
                serde_json::from_value(message["params"].clone()).unwrap();
            assert_eq!(
                notification.progress_token,
                rmcp::model::ProgressToken(rmcp::model::NumberOrString::Number(id))
            );
            let event: crate::progress::Event = serde_json::from_value(
                notification.meta.unwrap()["dev.jamesqo.mash/index-progress"].clone(),
            )
            .unwrap();
            if matches!(
                event.phase,
                Phase::Complete | Phase::Incomplete | Phase::SharedPending
            ) {
                assert!(terminal.replace(event.phase).is_none());
                assert_eq!(event.malformed_records, Some(malformed));
                assert_eq!(event.eta_seconds, (malformed == 0).then_some(0));
                assert_diagnostics(
                    event.record_diagnostics.as_ref().unwrap(),
                    &source,
                    malformed,
                );
                if malformed > 0 {
                    let message = notification.message.unwrap();
                    assert!(message.contains("retry required"));
                    assert!(message.contains("1 malformed records"));
                    assert!(message.contains("broken.jsonl"));
                    assert!(!message.contains("ETA"));
                }
            }
        }
        assert_eq!(terminal, Some(phase));
        if malformed > 0 {
            std::fs::remove_file(&source).unwrap();
        }
    }
    drop(write);
    drop(read);
    server.waiting().await.unwrap();
}
