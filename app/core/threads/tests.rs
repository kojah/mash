use super::*;
use crate::provider::Provider;
use crate::search_results::ThreadMetadata;
use axum::{Json, Router, routing::post};

fn config(directory: &Path, url: &str) -> ResolvedConfig {
    let path = directory.join("config.toml");
    std::fs::write(&path, format!("version = 1\nproviders = {{}}\n[shared]\ngroup_id = 'group'\nnode_id = 'laptop'\napi_url = '{url}'\ntoken_env = 'HOME'\n")).unwrap();
    crate::config::load(Some(&path)).unwrap()
}

fn database(directory: &Path) -> std::path::PathBuf {
    let path = directory.join("index.sqlite3");
    let mut storage = Storage::open_writer(&path).unwrap();
    storage
        .replace_conversation(&crate::ingestion::Conversation {
            id: "thread-105".into(),
            provider: Provider::Codex,
            claude_lineage: None,
            source: crate::ingestion::SourceIdentity::file("local"),
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

#[tokio::test]
async fn combined_catalog_merges_before_paging_and_reports_remote_failure() {
    let app = Router::new().route(
        "/v1/threads",
        post(|Json(input): Json<Request>| async move {
            assert!(input.exclude_self);
            let mut collector = Collector::new(input).unwrap();
            for number in 0..110 {
                collector.push(Thread {
                    conversation_id: format!("thread-{number:03}"),
                    provider: Provider::Codex,
                    node_id: "peer".into(),
                    metadata: ThreadMetadata {
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
    let result = run(&config, &database, input, false, false).await.unwrap();
    assert!(result.shared);
    assert_eq!(result.page.next_offset, Some(105));
    assert_eq!(
        result
            .page
            .threads
            .iter()
            .map(|row| row.conversation_id.as_str())
            .collect::<Vec<_>>(),
        [
            "thread-009",
            "thread-008",
            "thread-007",
            "thread-006",
            "thread-005"
        ]
    );
    let result = run(&config, &database, Request::default(), false, false)
        .await
        .unwrap();
    assert_eq!(
        result
            .page
            .threads
            .iter()
            .filter(|row| row.conversation_id == "thread-105")
            .count(),
        1
    );
    assert_eq!(
        result
            .page
            .threads
            .iter()
            .find(|row| row.conversation_id == "thread-105")
            .unwrap()
            .node_id,
        "laptop"
    );
    server.abort();
    let result = run(&config, &database, Request::default(), false, false)
        .await
        .unwrap();
    assert!(!result.shared);
    assert!(result.shared_error.is_some());
    assert_eq!(result.page.threads.len(), 1);
    assert!(
        run(&config, &database, Request::default(), false, true)
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
        let error = run(&config, &missing, input, local, shared)
            .await
            .unwrap_err();
        assert!(matches!(error.error.kind, crate::ErrorKind::Usage));
    }
    assert!(!missing.exists());
    let path = database(directory.path());
    let result = run(&config, &path, Request::default(), true, false)
        .await
        .unwrap();
    assert_eq!(result.page.threads.len(), 1);
    assert!(!result.shared);
    assert!(result.shared_error.is_none());
    assert!(!directory.path().join("models").exists());
}
