//! Exercise the real application facade, HTTP client, authorization and both stores.
use super::*;

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn qdrant_core_thread_catalog_dedup_replacement_delete_and_forget() {
    let token = std::env::var("PATH").unwrap();
    contract(
        Service::with_parameters(true, 384, 64 * 1024 * 1024, &token).await,
        &token,
    )
    .await;
}
use crate::core::{self, Operation, Paths};
use crate::provider::Provider;
use crate::storage::Storage;
use std::path::Path;

fn configuration(directory: &Path, service: &Service) -> Paths {
    let path = directory.join("config.toml");
    std::fs::write(&path, format!("version = 1\nproviders = {{}}\n[shared]\ngroup_id = '{}'\nnode_id = 'first'\napi_url = '{}'\ntoken_env = 'PATH'\n", service.group, service.url)).unwrap();
    Paths {
        config: Some(path),
        db: Some(directory.join("index.sqlite3")),
        models_dir: Some(directory.join("uninstalled-models")),
    }
}

fn local_copy(path: &Path) {
    let mut storage = Storage::open_writer(path).unwrap();
    storage
        .replace_conversation(&crate::silver::Conversation {
            id: "a".into(),
            provider: Provider::Codex,
            title_origin: None,
            timestamp_origins: None,
            claude_lineage: None,
            source: crate::silver::SourceIdentity::file("local"),
            title: Some("Local Alpha".into()),
            project: None,
            repository: None,
            created_at: Some(1000),
            updated_at: Some(4000),
            messages: Vec::new().into(),
        })
        .unwrap();
    storage.commit_writer().unwrap();
}

async fn discover(paths: &Paths, input: Request, shared: bool) -> Page {
    let response = core::execute(core::Request {
        paths: paths.clone(),
        operation: Operation::ListThreads {
            input: Box::new(input),
            local: false,
            shared,
            diagnostics: false,
        },
    })
    .await
    .unwrap();
    let core::Response::ListThreads(response) = response else {
        panic!("thread response")
    };
    assert!(response.shared, "{:?}", response.shared_error);
    response.page
}

mod messages;

async fn contract(service: Service, token: &str) {
    let directory = tempfile::tempdir().unwrap();
    let paths = configuration(directory.path(), &service);
    local_copy(paths.db.as_deref().unwrap());
    for owner in [token, SECOND] {
        service
            .upload(owner, &catalog_snapshot(1))
            .await
            .error_for_status()
            .unwrap();
    }
    let initial = discover(&paths, Request::default(), false).await;
    assert_eq!(initial.threads.len(), 5);
    assert_eq!(initial.threads[0].conversation_id, "a");
    assert_eq!(initial.threads[0].node_id, "first");
    assert_eq!(
        initial.threads[0].metadata.title.as_deref(),
        Some("Local Alpha")
    );
    let own = discover(
        &paths,
        Request {
            node: vec!["first".into()],
            ..Default::default()
        },
        false,
    )
    .await;
    assert_eq!(
        own.threads.len(),
        1,
        "combined reads exclude this node's published copy"
    );
    let peer = discover(
        &paths,
        Request {
            node: vec!["second".into()],
            ..Default::default()
        },
        false,
    )
    .await;
    assert_eq!(peer.threads.len(), 5);
    assert!(peer.threads.iter().all(|row| row.node_id == "second"));
    replace_and_forget(&service, &paths, token).await;
    verify_client_authorization(&paths).await;
    assert!(!paths.models_dir.as_ref().unwrap().exists());
    service.cleanup().await;
}

async fn verify_client_authorization(paths: &Paths) {
    let config = paths.config.as_deref().unwrap();
    let text = std::fs::read_to_string(config).unwrap();
    std::fs::write(
        config,
        text.replace("node_id = 'first'", "node_id = 'second'"),
    )
    .unwrap();
    let operation = |shared| Operation::ListThreads {
        input: Box::default(),
        local: false,
        shared,
        diagnostics: false,
    };
    let fallback = core::execute(core::Request {
        paths: paths.clone(),
        operation: operation(false),
    })
    .await
    .unwrap();
    let core::Response::ListThreads(fallback) = fallback else {
        panic!("thread response")
    };
    assert!(!fallback.shared);
    assert!(fallback.shared_error.is_some());
    assert_eq!(fallback.page.threads.len(), 1);
    let error = core::execute(core::Request {
        paths: paths.clone(),
        operation: operation(true),
    })
    .await
    .unwrap_err();
    assert!(matches!(error.error.kind, crate::ErrorKind::SharedIndex));
    assert!(!error.error.retryable);
}

async fn replace_and_forget(service: &Service, paths: &Paths, token: &str) {
    let mut replacement = catalog_snapshot(2);
    replacement[1]["records"]
        .as_array_mut()
        .unwrap()
        .retain(|row| row["id"] != "c");
    replacement[1]["records"][0]["title"] = json!("New Alpha");
    replacement[1]["records"][0]["updated_at"] = json!(5000);
    service
        .upload(SECOND, &replacement)
        .await
        .error_for_status()
        .unwrap();
    let replaced = discover(paths, Request::default(), false).await;
    assert_eq!(replaced.threads.len(), 4);
    assert!(
        replaced
            .threads
            .iter()
            .all(|row| row.conversation_id != "c")
    );
    assert_eq!(replaced.threads[0].node_id, "second");
    assert_eq!(
        replaced.threads[0].metadata.title.as_deref(),
        Some("New Alpha")
    );
    service
        .upload(
            SECOND,
            &[
                json!({"type":"start","instance":"snapshot-instance","revision":3,"base_revision":2,"snapshot":false,"forgets":["a"]}),
                json!({"type":"finish"}),
            ],
        )
        .await
        .error_for_status()
        .unwrap();
    let combined = discover(paths, Request::default(), false).await;
    assert_eq!(
        combined.threads[0].node_id, "first",
        "forget on the peer preserves the local copy"
    );
    service
        .upload(
            token,
            &[
                json!({"type":"start","instance":"snapshot-instance","revision":2,"base_revision":1,"snapshot":false,"forgets":["a"]}),
                json!({"type":"finish"}),
            ],
        )
        .await
        .error_for_status()
        .unwrap();
    let shared = discover(paths, Request::default(), true).await;
    assert_eq!(shared.threads.len(), 4);
    assert!(shared.threads.iter().all(|row| row.conversation_id != "a"));
    let combined = discover(paths, Request::default(), false).await;
    assert!(
        combined
            .threads
            .iter()
            .any(|row| row.conversation_id == "a" && row.node_id == "first")
    );
    assert_eq!(
        service.upload(SECOND, &replacement).await.status(),
        StatusCode::CONFLICT
    );
    replacement[0]["revision"] = json!(4);
    service
        .upload(SECOND, &replacement)
        .await
        .error_for_status()
        .unwrap();
    assert!(
        discover(paths, Request::default(), true)
            .await
            .threads
            .iter()
            .all(|row| row.conversation_id != "a"),
        "a delayed publication cannot resurrect a tombstoned catalog entry"
    );
}
