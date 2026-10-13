use super::*;
use crate::core::{self, Operation, Paths, Request, Response, SearchMode};
use crate::views::diagnostics::{Artifact, Scope, Source};
use axum::{Json, Router, extract::State, routing::post};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn assert_diagnostics(response: &core::SearchResponse, scope: Scope, fallback: bool, count: usize) {
    let diagnostics = response.diagnostics.as_ref().unwrap();
    assert_eq!(diagnostics.requested_scope, scope);
    let completed = match (scope, fallback) {
        (Scope::Combined, false) => vec![Source::Local, Source::Shared],
        (Scope::Shared, false) => vec![Source::Shared],
        _ => vec![Source::Local],
    };
    assert_eq!(diagnostics.completed_sources, completed);
    assert_eq!(diagnostics.returned_results, count);
    assert_eq!(diagnostics.shared_fallback, fallback);
    let shared = diagnostics
        .stages
        .iter()
        .find(|s| s.stage == crate::observability::retrieval::Stage::SharedCandidates)
        .unwrap();
    assert_eq!(shared.calls, 1);
    assert_eq!(shared.completed_calls, u64::from(!fallback));
    assert!(shared.filters_before_limit);
    assert_eq!(
        diagnostics.filter_fields,
        [
            crate::search_filter::Field::Provider,
            crate::search_filter::Field::Role,
            crate::search_filter::Field::After
        ]
    );
    assert!(!diagnostics.stages.iter().any(|s| matches!(
        s.stage,
        crate::observability::retrieval::Stage::Embed
            | crate::observability::retrieval::Stage::Rerank
            | crate::observability::retrieval::Stage::LocalDense
    )));
    assert!(diagnostics.fusion.is_none());
    assert_eq!(
        diagnostics.selected_artifacts,
        vec![Artifact::Metadata, Artifact::Lexical]
    );
}

fn hit() -> SearchHit {
    SearchHit {
        id: "peer-message".into(),
        conversation_id: "peer-thread".into(),
        source: crate::source::Source::Provider(crate::provider::Provider::Codex),
        object: crate::source::ObjectType::native_message(crate::provider::Provider::Codex),
        ancestors: Vec::new(),
        role: Some(crate::role::Role::User),
        author_kind: None,
        content: "use voyage-code-4".into(),
        created_at: Some(1000),
        metadata: crate::search_results::ChatMetadata::default(),
        copies: vec![],
        node_id: Some("peer".into()),
        lexical_score: Some(0.0),
        lookup: None,
        lexical_match: None,
        semantic_score: None,
        rerank_score: None,
        fusion_score: 0.0,
    }
}

async fn assert_thread_projection(mut request: Request) {
    let Operation::Search { unit, .. } = &mut request.operation else {
        panic!("search")
    };
    *unit = Some(SearchUnit::Chats);
    let Response::Search(threads) = core::execute(request).await.unwrap() else {
        panic!("search")
    };
    let crate::search_results::Results::Chats(threads) = threads.results else {
        panic!("chats")
    };
    let anchor = &threads[0].matches[0];
    assert_eq!(anchor.node_id.as_deref(), Some("peer"));
    assert_eq!(anchor.message_id, "peer-message");
    let lookup = anchor.lookup.as_ref().unwrap();
    assert_eq!(lookup.view.id, "peer-message");
    assert_eq!(lookup.view.shared.as_deref(), Some("peer"));
    assert_eq!(anchor.projection_span.start_char, 0);
    assert_eq!(
        anchor.projection_span.end_char,
        hit().content.chars().count()
    );
}

async fn reply(
    State(mode): State<Arc<AtomicUsize>>,
    Json(input): Json<protocol::Search>,
) -> Json<protocol::Candidates> {
    assert_eq!(input.intent, exact::Intent::ExactMention);
    assert_eq!(input.query, "voyage-code-4");
    assert!(input.vector.is_empty() && input.generation.is_empty());
    assert!(input.filter.is_some());
    let mut candidates = protocol::Candidates {
        lexical: vec![hit()],
        semantic: vec![],
        documents: vec![],
    };
    match mode.load(Ordering::SeqCst) {
        1 => candidates.lexical[0].content = "voyage-code-40".into(),
        2 => candidates.lexical[0].node_id = Some("self".into()),
        3 => candidates.semantic.push(hit()),
        4 => candidates.lexical[0].node_id = None,
        _ => {}
    }
    Json(candidates)
}

#[tokio::test]
async fn exact_core_shared_and_fallback_paths_are_model_free_and_query_only() {
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("local.sqlite3");
    let mut writer = Storage::open_writer(&database).unwrap();
    writer.finish_silver_ingestion().unwrap();
    writer.rebuild_derived_search_state().unwrap();
    writer.publish_lexical_manifest().unwrap();
    writer.commit_writer().unwrap();
    drop(writer);
    let before = std::fs::read(&database).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mode = Arc::new(AtomicUsize::new(0));
    let url = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .route("/v1/search", post(reply))
        .with_state(mode.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let config = root.path().join("node.toml");
    std::fs::write(&config, format!("version=1\nproviders={{}}\n[index]\ndestination='both'\n[shared]\ngroup_id='group'\nnode_id='self'\napi_url={url:?}\ntoken_env='HOME'\n[views.query]\nintent='exact-mention'\n")).unwrap();
    let request = Request {
        paths: Paths {
            config: Some(config),
            db: Some(database.clone()),
            models_dir: Some(root.path().join("missing-models")),
        },
        operation: Operation::Search {
            shared_vector_search: None,
            fusion: crate::views::config::FusionOverrides::default(),
            ranking: None,
            diagnostics: true,
            query: "voyage-code-4".into(),
            intent: None,
            unit: Some(SearchUnit::Messages),
            limit: Some(2),
            offset: 0,
            filters: Box::new(crate::search_filter::Selection {
                provider: vec![crate::provider::Provider::Codex],
                role: vec![crate::role::Role::User],
                after: Some(1),
                ..Default::default()
            }),
            local: false,
            shared: false,
        },
    };
    let Response::Search(response) = core::execute(request.clone()).await.unwrap() else {
        panic!("search response")
    };
    assert!(matches!(response.realized_mode, SearchMode::MergedLexical));
    assert!(response.shared && response.shared_error.is_none());
    assert_diagnostics(&response, Scope::Combined, false, 1);
    let crate::search_results::Results::Messages(hits) = response.results else {
        panic!("messages")
    };
    assert_eq!(hits.len(), 1);
    for malformed in 1..5 {
        mode.store(malformed, Ordering::SeqCst);
        let Response::Search(response) = core::execute(request.clone()).await.unwrap() else {
            panic!("search response")
        };
        assert!(matches!(response.realized_mode, SearchMode::LocalLexical));
        assert!(!response.shared && response.shared_error.is_some());
        assert_diagnostics(&response, Scope::Combined, true, 0);
        let crate::search_results::Results::Messages(hits) = response.results else {
            panic!("messages")
        };
        assert!(hits.is_empty());
    }
    let mut shared = request;
    let Operation::Search { shared: only, .. } = &mut shared.operation else {
        panic!("search")
    };
    *only = true;
    mode.store(1, Ordering::SeqCst);
    assert!(
        core::execute(shared.clone()).await.is_err(),
        "shared-only cannot fall back"
    );
    mode.store(0, Ordering::SeqCst);
    let Response::Search(response) = core::execute(shared.clone()).await.unwrap() else {
        panic!("search response")
    };
    assert!(matches!(response.realized_mode, SearchMode::Lexical));
    assert!(response.shared);
    assert_diagnostics(&response, Scope::Shared, false, 1);
    assert_thread_projection(shared).await;
    server.abort();
    assert_eq!(std::fs::read(database).unwrap(), before);
    assert!(!root.path().join("missing-models").exists());
}
