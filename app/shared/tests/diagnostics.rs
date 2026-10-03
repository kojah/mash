use super::*;
use crate::core::{self, Operation, Paths, Request, Response};
use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::post};

struct Fixture {
    _root: tempfile::TempDir,
    request: Request,
    database: std::path::PathBuf,
    server: tokio::task::JoinHandle<()>,
}

async fn fixture() -> Fixture {
    let models_dir = std::path::PathBuf::from(
        std::env::var_os("MASH_TEST_MODELS_DIR").expect("installed models"),
    );
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("local.sqlite3");
    let generation = Models::new(models_dir.clone()).generation();
    seed_ready(&database, &generation);
    let mut remote = Storage::open_search(&database, &generation)
        .unwrap()
        .search_filtered("hello", 10, None, None, None)
        .unwrap()
        .remove(0);
    remote.id = "peer-message".into();
    remote.conversation_id = "peer-thread".into();
    remote.node_id = Some("peer".into());
    let document = protocol::Document {
        node_id: "peer".into(),
        id: remote.id.clone(),
        projection: remote.content.clone(),
    };
    let reply = std::sync::Arc::new(protocol::Candidates {
        lexical: vec![remote],
        semantic: vec![],
        documents: vec![document],
    });
    let router = Router::new()
        .route(
            "/v1/search",
            post(move || {
                let reply = reply.clone();
                async move { Json(reply.as_ref()).into_response() }
            }),
        )
        .route(
            "/v1/view",
            post(|| async { StatusCode::SERVICE_UNAVAILABLE }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let config = root.path().join("node.toml");
    std::fs::write(&config, format!("version=1\nproviders={{}}\n[index]\ndestination='both'\n[shared]\ngroup_id='group'\nnode_id='local-node'\napi_url={url:?}\ntoken_env='HOME'\n")).unwrap();
    let request = Request {
        paths: Paths {
            config: Some(config),
            db: Some(database.clone()),
            models_dir: Some(models_dir),
        },
        operation: Operation::Search {
            query: "hello".into(),
            intent: None,
            limit: Some(10),
            unit: Some(SearchUnit::Messages),
            fusion: crate::gold::config::FusionOverrides::default(),
            diagnostics: true,
            filters: Box::default(),
            shared: false,
            local: false,
        },
    };
    Fixture {
        _root: root,
        request,
        database,
        server,
    }
}

#[tokio::test]
#[ignore = "requires explicitly installed MASH_TEST_MODELS_DIR"]
async fn real_models_shared_context_failure_preserves_measured_retrieval_sources() {
    let fixture = fixture().await;
    let before = std::fs::read(&fixture.database).unwrap();
    let Response::Search(response) = core::execute(fixture.request).await.unwrap() else {
        panic!("search")
    };
    assert!(response.shared_error.is_some());
    let crate::search_results::Results::Messages(hits) = &response.results else {
        panic!("messages")
    };
    assert!(
        hits.iter()
            .any(|hit| hit.node_id.as_deref() == Some("peer"))
    );
    let diagnostics = response.diagnostics.unwrap();
    assert_eq!(
        diagnostics.completed_sources,
        vec![
            crate::gold::diagnostics::Source::Local,
            crate::gold::diagnostics::Source::Shared
        ]
    );
    assert!(!diagnostics.shared_fallback);
    assert!(diagnostics.shared_context_incomplete);
    let context = diagnostics
        .stages
        .iter()
        .find(|s| s.stage == crate::observability::retrieval::Stage::Context)
        .unwrap();
    assert_eq!(context.calls, 1);
    assert_eq!(context.completed_calls, 0);
    let remote = diagnostics
        .stages
        .iter()
        .find(|s| s.stage == crate::observability::retrieval::Stage::SharedCandidates)
        .unwrap();
    assert_eq!(remote.completed_calls, 1);
    assert_eq!(remote.items_out, Some(1));
    assert_eq!(std::fs::read(&fixture.database).unwrap(), before);
    fixture.server.abort();
}
