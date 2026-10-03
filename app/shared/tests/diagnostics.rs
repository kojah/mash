use super::*;
use crate::core::{self, Operation, Paths, Request, Response};
use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::post};

struct Fixture {
    _root: tempfile::TempDir,
    request: Request,
    database: std::path::PathBuf,
    server: tokio::task::JoinHandle<()>,
}

#[tokio::test]
async fn shared_lexical_capabilities_reject_hybrid_before_model_admission() {
    let root = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = Router::new().route(
        "/v1/capabilities",
        axum::routing::get(|| async {
            Json(crate::gold::config::Build {
                representations: crate::gold::config::Representations::LexicalOnly,
                ..Default::default()
            })
        }),
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let config = root.path().join("node.toml");
    let database = root.path().join("absent.sqlite3");
    let models = root.path().join("absent-models");
    std::fs::write(&config, format!("version=1\nproviders={{}}\n[shared]\ngroup_id='group'\nnode_id='node'\napi_url='http://{address}'\ntoken_env='HOME'\n")).unwrap();
    let request = Request {
        paths: Paths {
            config: Some(config),
            db: Some(database.clone()),
            models_dir: Some(models.clone()),
        },
        operation: Operation::Search {
            shared_vector_search: None,
            query: "needle".into(),
            intent: None,
            limit: None,
            unit: None,
            fusion: crate::gold::config::FusionOverrides::default(),
            ranking: None,
            diagnostics: false,
            filters: Box::default(),
            shared: true,
            local: false,
        },
    };
    let capacity = tokio::sync::Semaphore::new(0);
    let error = tokio::time::timeout(
        Duration::from_secs(1),
        core::execute_with_model_limit(request, &capacity),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert_eq!(error.error.kind, crate::ErrorKind::Usage);
    assert!(!database.exists() && !models.exists());
    server.abort();
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
            "/v1/capabilities",
            axum::routing::get(|| async { Json(crate::gold::config::Build::default()) }),
        )
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
            shared_vector_search: None,
            query: "hello".into(),
            intent: None,
            limit: Some(10),
            unit: Some(SearchUnit::Messages),
            fusion: crate::gold::config::FusionOverrides::default(),
            ranking: None,
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

fn assert_context_outcome(response: &core::SearchResponse, shared_only: bool) {
    assert!(response.shared_error.is_none());
    assert!(response.shared_context_error.is_some());
    assert!(response.shared);
    assert!(matches!(
        (&response.realized_mode, shared_only),
        (core::SearchMode::MergedHybrid, false) | (core::SearchMode::Hybrid, true)
    ));
    let crate::search_results::Results::Messages(hits) = &response.results else {
        panic!("messages")
    };
    assert!(
        hits.iter()
            .any(|hit| hit.node_id.as_deref() == Some("peer"))
    );
}

fn assert_measured_sources(diagnostics: &crate::gold::diagnostics::Diagnostics, shared_only: bool) {
    use crate::gold::diagnostics::Source;
    let expected = if shared_only {
        vec![Source::Shared]
    } else {
        vec![Source::Local, Source::Shared]
    };
    assert_eq!(diagnostics.completed_sources, expected);
    assert!(!diagnostics.shared_fallback);
    assert!(diagnostics.shared_context_incomplete);
    let context = diagnostics
        .stages
        .iter()
        .find(|s| s.stage == crate::observability::retrieval::Stage::Context)
        .unwrap();
    assert_eq!((context.calls, context.completed_calls), (1, 0));
    let remote = diagnostics
        .stages
        .iter()
        .find(|s| s.stage == crate::observability::retrieval::Stage::SharedCandidates)
        .unwrap();
    assert_eq!(remote.completed_calls, 1);
    assert_eq!(remote.items_out, Some(1));
}

#[tokio::test]
#[ignore = "requires explicitly installed MASH_TEST_MODELS_DIR"]
async fn real_models_shared_context_failure_preserves_measured_retrieval_sources() {
    let fixture = fixture().await;
    let before = std::fs::read(&fixture.database).unwrap();
    for shared_only in [false, true] {
        let mut baseline = None;
        for diagnostics in [false, true] {
            let mut request = fixture.request.clone();
            let Operation::Search {
                shared,
                diagnostics: enabled,
                ..
            } = &mut request.operation
            else {
                panic!("search")
            };
            *shared = shared_only;
            *enabled = diagnostics;
            let Response::Search(response) = core::execute(request).await.unwrap() else {
                panic!("search")
            };
            assert_context_outcome(&response, shared_only);
            if diagnostics {
                assert_measured_sources(response.diagnostics.as_ref().unwrap(), shared_only);
            } else {
                assert!(response.diagnostics.is_none());
            }
            let hits = serde_json::to_value(response.results).unwrap();
            if let Some(baseline) = &baseline {
                assert_eq!(&hits, baseline);
            } else {
                baseline = Some(hits);
            }
            assert_eq!(std::fs::read(&fixture.database).unwrap(), before);
        }
    }
    fixture.server.abort();
}
