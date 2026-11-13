use super::*;
use crate::provider::Provider;
use crate::search_results::ChatMetadata;
use axum::{Json, Router, routing::post};

mod failures;

#[tokio::test]
async fn dashboard_catalog_preserves_copies_and_partial_failure() {
    let app = Router::new().route(
        "/v1/catalog",
        axum::routing::get(|| async {
            Json(vec![Chat {
                chat_id: "thread-105".into(),
                first_message_id: None,
                provider: Provider::Codex,
                node_id: "peer".into(),
                metadata: ChatMetadata {
                    title: Some(format!("\n{}\nHidden continuation", "界".repeat(300))),
                    updated_at: Some(106),
                    ..Default::default()
                },
            }])
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path(), &url);
    let database = database(directory.path());
    let response = dashboard(&config, &database).await.unwrap();
    assert_eq!(response.local_node, "laptop");
    assert_eq!(response.chats.len(), 2);
    assert_eq!(response.chats[0].node_id, "peer");
    assert_eq!(
        response.chats[0].metadata.title.as_deref(),
        Some(format!("{}…", "界".repeat(240)).as_str())
    );
    assert!(
        response
            .chats
            .iter()
            .all(|chat| chat.metadata.enrichment.is_none())
    );
    assert_eq!(response.chats[1].node_id, "laptop");
    assert!(!response.truncated);
    assert!(response.shared_error.is_none());
    server.abort();
    let _ = server.await;
    let response = dashboard(&config, &database).await.unwrap();
    assert_eq!(response.chats.len(), 1);
    assert!(response.shared_error.is_some());
}

#[test]
fn dashboard_redaction_reuses_values_without_crossing_policy_or_source_changes() {
    let source = "password=\"sensitive-value\"";
    let make = |value: &str| Chat {
        chat_id: value.into(),
        first_message_id: None,
        provider: Provider::Codex,
        node_id: "local".into(),
        metadata: ChatMetadata {
            title: Some(value.into()),
            project: Some(value.into()),
            repository: Some(value.into()),
            ..Default::default()
        },
    };
    let response = finish_dashboard(
        vec![make(source), make(source)],
        "local".into(),
        None,
        crate::redaction::Policy::ON,
    );
    for chat in response.chats {
        assert_eq!(
            chat.metadata.title.as_deref(),
            Some("password=\"[REDACTED_SECRET]\"")
        );
        assert_eq!(chat.metadata.project, chat.metadata.title);
        assert_eq!(chat.metadata.repository, chat.metadata.title);
    }
    let raw = finish_dashboard(
        vec![make(source)],
        "local".into(),
        None,
        crate::redaction::Policy::new(false),
    );
    assert_eq!(raw.chats[0].metadata.title.as_deref(), Some(source));
    let changed = finish_dashboard(
        vec![make("benign")],
        "local".into(),
        None,
        crate::redaction::Policy::ON,
    );
    assert_eq!(changed.chats[0].metadata.title.as_deref(), Some("benign"));
    // Only the first line is shown, so a later line is never masked or sent.
    let prompt = finish_dashboard(
        vec![make("Rotate the key\npassword=\"sensitive-value\"")],
        "local".into(),
        None,
        crate::redaction::Policy::ON,
    );
    assert_eq!(
        prompt.chats[0].metadata.title.as_deref(),
        Some("Rotate the key")
    );
}

fn config(directory: &Path, url: &str) -> ResolvedConfig {
    let path = directory.join("config.toml");
    std::fs::write(&path, format!("version = 1\nproviders = {{}}\n[shared]\ngroup_id = 'group'\nnode_id = 'laptop'\napi_url = '{url}'\ntoken_env = 'HOME'\n")).unwrap();
    crate::config::load(Some(&path)).unwrap()
}

fn database(directory: &Path) -> std::path::PathBuf {
    let path = directory.join("index.sqlite3");
    let mut storage = Storage::open_writer(&path).unwrap();
    storage
        .replace_chat(&crate::silver::Chat {
            id: "thread-105".into(),
            provider: Provider::Codex,
            title_origin: None,
            timestamp_origins: None,
            claude_lineage: None,
            source: crate::silver::SourceIdentity::file("local"),
            title: Some("local".into()),
            project: None,
            repository: None,
            created_at: Some(1),
            updated_at: Some(105),
            messages: Vec::new().into(),
        })
        .unwrap();
    storage.commit_writer().unwrap();
    path
}

fn assert_combined_metadata_diagnostics(result: &Response) {
    let diagnostics = result.diagnostics.as_ref().unwrap();
    assert_eq!(diagnostics.selected_artifacts, [Artifact::Metadata]);
    assert_eq!(diagnostics.requested_scope, Scope::Combined);
    assert_eq!(
        diagnostics.completed_sources,
        [Source::Local, Source::Shared]
    );
    assert_eq!(diagnostics.returned_results, 5);
    assert!(!diagnostics.shared_fallback);
    assert_eq!(
        diagnostics
            .stages
            .iter()
            .map(|m| m.stage)
            .collect::<Vec<_>>(),
        [
            Stage::LocalMetadata,
            Stage::SharedMetadata,
            Stage::MetadataMerge
        ]
    );
}

#[tokio::test]
async fn combined_catalog_merges_before_paging_and_reports_remote_failure() {
    let app = Router::new().route(
        "/v1/chats",
        post(|Json(input): Json<Request>| async move {
            assert!(input.exclude_self);
            let mut collector = Collector::new(input).unwrap();
            for number in 0..110 {
                collector.push(Chat {
                    chat_id: format!("thread-{number:03}"),
                    first_message_id: Some(format!("message-{number:03}")),
                    provider: Provider::Codex,
                    node_id: "peer".into(),
                    metadata: ChatMetadata {
                        enrichment: None,
                        updated_at: Some(number),
                        ..Default::default()
                    },
                });
            }
            Json(collector.finish())
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path(), &url);
    let database = database(directory.path());
    let input = Request {
        offset: 100,
        limit: 5,
        ..Default::default()
    };
    let result = run(&config, &database, input, false, false, true)
        .await
        .unwrap();
    assert_combined_metadata_diagnostics(&result);
    assert!(result.shared);
    assert_eq!(result.next_offset, Some(105));
    // A peer's row names its publishing node, so view reads it from shared.
    let lookup = result.chats[0].lookup.as_ref().unwrap();
    assert_eq!(lookup.view.id, "message-009");
    assert_eq!(lookup.view.shared.as_deref(), Some("peer"));
    assert!(result.chats[0].chat.first_message_id.is_none());
    assert_eq!(
        result
            .chats
            .iter()
            .map(|row| row.chat.chat_id.as_str())
            .collect::<Vec<_>>(),
        [
            "thread-009",
            "thread-008",
            "thread-007",
            "thread-006",
            "thread-005"
        ]
    );
    let result = run(&config, &database, Request::default(), false, false, true)
        .await
        .unwrap();
    assert_eq!(
        result
            .chats
            .iter()
            .filter(|row| row.chat.chat_id == "thread-105")
            .count(),
        1
    );
    assert_eq!(
        result
            .chats
            .iter()
            .find(|row| row.chat.chat_id == "thread-105")
            .unwrap()
            .chat
            .node_id,
        "laptop"
    );
    server.abort();
    let result = run(&config, &database, Request::default(), false, false, true)
        .await
        .unwrap();
    assert!(!result.shared);
    assert!(result.shared_error.is_some());
    let diagnostics = result.diagnostics.as_ref().unwrap();
    assert!(diagnostics.shared_fallback);
    assert_eq!(diagnostics.completed_sources, [Source::Local]);
    let failed = diagnostics
        .stages
        .iter()
        .find(|m| m.stage == Stage::SharedMetadata)
        .unwrap();
    assert_eq!(failed.calls, 1);
    assert_eq!(failed.completed_calls, 0);
    assert_eq!(result.chats.len(), 1);
    assert!(
        run(&config, &database, Request::default(), false, true, false)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn invalid_catalog_inputs_and_local_scope_never_contact_remote_or_create_database() {
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path(), "http://127.0.0.1:1");
    let missing = directory.path().join("missing.sqlite3");
    for (input, local, shared) in [
        (
            Request {
                limit: 0,
                ..Default::default()
            },
            true,
            false,
        ),
        (Request::default(), true, true),
    ] {
        let error = run(&config, &missing, input, local, shared, false)
            .await
            .unwrap_err();
        assert!(matches!(error.error.kind, crate::ErrorKind::Usage));
    }
    assert!(!missing.exists());
    let path = database(directory.path());
    let result = run(&config, &path, Request::default(), true, false, false)
        .await
        .unwrap();
    assert!(result.diagnostics.is_none());
    assert_eq!(result.chats.len(), 1);
    assert!(!result.shared);
    assert!(result.shared_error.is_none());
    assert!(!directory.path().join("models").exists());
}
