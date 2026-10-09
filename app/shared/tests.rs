use super::client::{Endpoint, get, post, response};
use super::publication::{target, upload};
use super::retrieval::{CONTEXT_DEADLINE, context_documents, prepare_search, search_prepared};
use super::*;
use crate::config::SharedConfig;
use crate::gold::readiness::{Artifacts, State};
use crate::search_results::SearchUnit;
use crate::semantic::{self, Models};
use crate::silver::{Conversation, NormalizedMessage};
use crate::storage::{EmbeddingWrite, Message, SearchHit, Storage};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

mod context_batch;
mod correlation;
mod decoding;
mod diagnostics;
mod publication_recovery;
mod selections;

#[test]
fn node_freshness_preserves_unknown_artifact_states_from_older_responses() {
    let response = r#"{
        "node_id":"node", "instance":"node-instance", "revision":1, "last_synced_at":null,
        "last_updated_at":null, "conversations":0, "messages":0
    }"#;
    let node: NodeFreshness = serde_json::from_str(response).unwrap();
    assert!(node.artifacts.is_none());
    assert!(node.source_silver.is_none());
    let encoded = serde_json::to_string(&node).unwrap();
    assert!(!encoded.contains("\"artifacts\""));
    assert!(!encoded.contains("\"source_silver\""));
    let reported = r#"{
        "node_id":"node", "instance":"node-instance", "revision":1, "last_synced_at":null,
        "last_updated_at":null, "conversations":0, "messages":0,
        "artifacts":{"metadata":"ready","lexical":"stale","dense":"disabled"}
    }"#;
    let node: NodeFreshness = serde_json::from_str(reported).unwrap();
    assert_eq!(
        node.artifacts,
        Some(Artifacts {
            metadata: State::Ready,
            lexical: State::Stale,
            dense: State::Disabled,
        })
    );
    let encoded = serde_json::to_string(&node).unwrap();
    let decoded: NodeFreshness = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.artifacts, node.artifacts);
    assert!(
        serde_json::from_str::<NodeFreshness>(&reported.replace("disabled", "invented")).is_err()
    );
}

#[test]
fn freshness_source_certificate_is_typed_and_distinct_from_shared_identity() {
    let response = r#"{
        "node_id":"node", "instance":"node-instance", "revision":100, "last_synced_at":null,
        "last_updated_at":null, "conversations":0, "messages":0,
        "source_silver":{"revision":7,"interpretation_version":1,
            "completeness":"incomplete","rejected_sources":2}
    }"#;
    let node: NodeFreshness = serde_json::from_str(response).unwrap();
    assert_eq!(
        node.source_silver,
        Some(crate::silver::certificate::Certificate {
            revision: 7,
            interpretation_version: 1,
            completeness: crate::silver::certificate::Completeness::Incomplete,
            rejected_sources: 2,
        })
    );
    assert!(node.silver_revision.is_none());
    assert_eq!(node.revision, 100);
    let decoded: NodeFreshness =
        serde_json::from_str(&serde_json::to_string(&node).unwrap()).unwrap();
    assert_eq!(decoded.source_silver, node.source_silver);
    assert!(
        serde_json::from_str::<NodeFreshness>(&response.replace("incomplete", "invented")).is_err()
    );
}

#[tokio::test]
async fn context_deadline_preserves_completed_evidence_without_waiting_for_every_wave() {
    use axum::Router;
    use context_batch::view;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::Instant;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = SharedConfig {
        group_id: "test".into(),
        node_id: "local".into(),
        api_url: format!("http://{}", listener.local_addr().unwrap()),
        token_env: "HOME".into(),
    };
    let dropped = Arc::new(AtomicBool::new(false));
    let server_dropped = Arc::clone(&dropped);
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/contexts", axum::routing::post(view))
                .layer(
                    tower_http::compression::CompressionLayer::new()
                        .quality(tower_http::CompressionLevel::Fastest),
                )
                .with_state(server_dropped),
        )
        .await
        .unwrap();
    });
    let hits = (0..24)
        .map(|index| SearchHit {
            id: if index == 0 {
                "fast".into()
            } else {
                format!("slow-{index}")
            },
            conversation_id: "thread".into(),
            source: crate::source::Source::Provider(crate::provider::Provider::Codex),
            kind: crate::source::Kind::Message,
            role: Some(crate::role::Role::User),
            author_kind: None,
            content: "cache the model".into(),
            copies: Vec::new(),
            created_at: None,
            metadata: crate::search_results::ChatMetadata::default(),
            lexical_score: Some(1.0),
            lookup: None,
            lexical_match: None,
            semantic_score: None,
            fusion_score: 1.0,
            rerank_score: None,
            node_id: Some("remote".into()),
        })
        .collect::<Vec<_>>();
    let started = Instant::now();
    let (documents, error) = context_documents(&config, None, "test".into(), "cache", &hits)
        .await
        .unwrap();
    assert!(started.elapsed() < CONTEXT_DEADLINE + Duration::from_secs(1));
    assert!(error.unwrap().contains("deadline"));
    assert_eq!(documents.len(), 1);
    assert!(documents[&("remote".into(), "fast".into())].contains("keep the worker warm"));
    tokio::time::timeout(Duration::from_secs(1), async {
        while !dropped.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("HTTP cancellation must drain the pending server stream");
    server.abort();
}

#[tokio::test]
async fn interactive_request_timeout_does_not_inherit_the_upload_budget() {
    use axum::{Json, Router};
    async fn stalled() -> Json<protocol::Candidates> {
        std::future::pending().await
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = SharedConfig {
        group_id: "test".into(),
        node_id: "local".into(),
        api_url: format!("http://{}", listener.local_addr().unwrap()),
        token_env: "HOME".into(),
    };
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route("/v1/search", axum::routing::post(stalled)),
        )
        .await
        .unwrap();
    });
    let started = std::time::Instant::now();
    let query = protocol::Search {
        intent: crate::gold::exact::Intent::RelatedDiscussion,
        match_mode: crate::gold::matching::Mode::Strict,
        vector_policy: None,
        unit: SearchUnit::Chats,
        query: "cache".into(),
        generation: "test".into(),
        vector: Vec::new(),
        provider: None,
        days: None,
        exclude_self: true,
        filter: None,
    };
    let error = post::<protocol::Candidates>(&config, Endpoint::Search, &query)
        .await
        .unwrap_err();
    server.abort();
    assert!(started.elapsed() < Endpoint::Search.timeout() + Duration::from_secs(1));
    assert!(error.error.message.contains("timed out"));
}

#[tokio::test]
async fn buffered_publication_sends_a_complete_snapshot_before_acknowledging() {
    use axum::extract::State;
    use axum::{Json, Router};
    use tokio::sync::mpsc;

    async fn freshness() -> Json<serde_json::Value> {
        Json(serde_json::json!({
            "group_id": "test",
            "group_gold": {
                "version": "group",
                "build": crate::gold::config::Build::default(),
                "generation": semantic::SHARED_EMBEDDING_GENERATION,
                "dimensions": 2,
            },
            "nodes": [],
        }))
    }

    async fn capture(
        State(sender): State<mpsc::Sender<Vec<serde_json::Value>>>,
        body: axum::body::Bytes,
    ) -> Json<serde_json::Value> {
        let events = std::str::from_utf8(&body)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        sender.send(events).await.unwrap();
        Json(serde_json::json!({
            "counts": {"conversations": 1, "messages": 2, "searchable_messages": 1, "embeddings": 1},
            "revision": 1, "removed_messages": 0, "purged_conversations": 0
        }))
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("local.sqlite3");
    drop(seed(&path));
    let (sender, mut received) = mpsc::channel(1);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/freshness", axum::routing::get(freshness))
                .fallback(capture)
                .with_state(sender),
        )
        .await
        .unwrap();
    });
    let config = SharedConfig {
        group_id: "test".into(),
        node_id: "local-node".into(),
        api_url: format!("http://{address}"),
        token_env: "HOME".into(),
    };
    assert_eq!(
        publish(&path, &config, None).await.state,
        PublicationState::Published
    );
    let events = received.recv().await.unwrap();
    assert_eq!(events.first().unwrap()["type"], "start");
    assert_eq!(events.last().unwrap()["type"], "finish");
    let messages = events
        .iter()
        .find(|event| event["kind"] == "messages")
        .unwrap();
    assert_eq!(messages["records"].as_array().unwrap().len(), 2);
    assert!(!Storage::publication_is_pending(&path, &target(&config)).unwrap());
    server.abort();
}

#[tokio::test]
#[ignore = "requires MASH_TEST_MODELS_DIR containing an explicit model installation"]
async fn merged_search_requests_shared_candidates_while_local_sqlite_is_blocked() {
    use axum::Router;
    use tokio::sync::mpsc;

    let models_dir = std::env::var_os("MASH_TEST_MODELS_DIR")
        .map(std::path::PathBuf::from)
        .expect("explicit model directory");
    let models = Models::new(models_dir.clone());
    let generation = models.generation();
    // Warm inference before applying the database lock: the test measures
    // retrieval ordering, independently of model startup time.
    drop(crate::blocking(move || models.load()).await.unwrap());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("local.sqlite3");
    seed_ready(&path, &generation);
    // Readiness is checked before inference/HTTP; hold the lock only for the
    // subsequent candidate retrieval stage whose concurrency is under test.
    let prepared = prepare_search(
        Some((&path, &crate::gold::config::Build::default())),
        &models_dir,
        None,
        "hello",
        crate::gold::config::Ranking::default(),
    )
    .await
    .unwrap();
    let lock = rusqlite::Connection::open(&path).unwrap();
    lock.execute_batch("PRAGMA journal_mode=DELETE; BEGIN EXCLUSIVE")
        .unwrap();

    let (sender, mut received) = mpsc::channel(1);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .fallback(capture_search_request)
                .with_state(sender),
        )
        .await
        .unwrap();
    });
    let mut config = SharedConfig {
        group_id: "test".into(),
        node_id: "local-node".into(),
        api_url: format!("http://{address}"),
        token_env: "HOME".into(),
    };
    let filters = crate::search_filter::Selection::default()
        .resolve(&config.node_id)
        .unwrap();
    let unlock = async {
        let request = tokio::time::timeout(Duration::from_secs(3), received.recv()).await;
        // Always release the lock, including on failure, so a regression cannot
        // leave a blocking worker hung after the assertion.
        lock.execute_batch("ROLLBACK").unwrap();
        request.expect("shared request must start before SQLite is unlocked")
    };
    let mut options = crate::gold::config::Options::new(10, SearchUnit::Messages);
    options.fusion.lexical_weight = 4;
    options.fusion.semantic_weight = 4;
    let (result, request) = tokio::join!(
        search_prepared(&config, Some(&path), "hello", options, &filters, prepared,),
        unlock,
    );
    let request = request.unwrap();
    assert!(request.exclude_self);
    let result = result.unwrap();
    assert!(result.retrieval.into_error().is_none());
    assert!(result.context_error.is_none());
    let hits = result.hits;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "searchable");
    assert!(hits[0].fusion_score >= 4.0 / 61.0);
    assert!(hits[0].rerank_score.is_some());

    // Failure remains a local fallback for merged searches, and an error for
    // explicitly shared searches.
    config.token_env = "MASH_TEST_UNCONFIGURED_TOKEN_71B9".into();
    let fallback = search(
        &config,
        Some((&path, &crate::gold::config::Build::default())),
        &models_dir,
        None,
        "hello",
        options,
        &filters,
    )
    .await
    .unwrap();
    assert!(fallback.retrieval.into_error().is_some());
    assert_eq!(
        serde_json::to_value(fallback.hits).unwrap(),
        serde_json::to_value(hits).unwrap()
    );
    assert!(
        search(&config, None, &models_dir, None, "hello", options, &filters)
            .await
            .is_err()
    );
    server.abort();
}

async fn capture_search_request(
    axum::extract::State(sender): axum::extract::State<tokio::sync::mpsc::Sender<protocol::Search>>,
    axum::Json(request): axum::Json<protocol::Search>,
) -> axum::Json<protocol::Candidates> {
    sender.send(request).await.unwrap();
    axum::Json(protocol::Candidates {
        lexical: Vec::new(),
        semantic: Vec::new(),
        documents: Vec::new(),
    })
}

fn seed_ready(path: &Path, generation: &str) {
    drop(seed(path));
    let mut storage = Storage::open_writer(path).unwrap();
    storage.rebuild_derived_search_state().unwrap();
    storage
        .replace_embeddings(
            generation,
            &[EmbeddingWrite {
                message_id: "searchable",
                vector: &[127; 384],
                norm: 127.0 * 384_f32.sqrt(),
            }],
        )
        .unwrap();
    storage.mark_semantic_index_ready(generation).unwrap();
    storage
        .begin_silver_scan(crate::provider::Provider::Codex)
        .unwrap();
    storage
        .finish_silver_scan(crate::provider::Provider::Codex, true)
        .unwrap();
    storage.finish_silver_ingestion().unwrap();
    storage.mark_semantic_index_ready(generation).unwrap();
    storage.publish_lexical_manifest().unwrap();
    storage
        .publish_gold_manifest(&crate::gold::manifest::Manifest::new(
            crate::gold::config::Build::default(),
            generation.to_owned(),
            storage.silver_certificate().unwrap(),
        ))
        .unwrap();
    storage.commit_writer().unwrap();
}

#[tokio::test]
async fn http_pool_reuses_connections_without_caching_node_credentials() {
    use axum::extract::{ConnectInfo, State};
    use axum::http::{HeaderMap, Method};
    use axum::{Json, Router};
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct Received {
        peer: SocketAddr,
        method: Method,
        headers: HeaderMap,
    }
    async fn capture(
        State(received): State<Arc<Mutex<Vec<Received>>>>,
        ConnectInfo(peer): ConnectInfo<SocketAddr>,
        method: Method,
        headers: HeaderMap,
        _body: axum::body::Bytes,
    ) -> Json<serde_json::Value> {
        received.lock().unwrap().push(Received {
            peer,
            method,
            headers,
        });
        Json(serde_json::json!({
            "ok": true,
            "counts": {"conversations": 0, "messages": 0, "searchable_messages": 0, "embeddings": 0},
            "revision": 1, "removed_messages": 0, "purged_conversations": 0
        }))
    }
    let received = Arc::new(Mutex::new(Vec::<Received>::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = Router::new().fallback(capture).with_state(received.clone());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    // Existing environment values avoid process-global mutation in parallel tests.
    let mut config = SharedConfig {
        group_id: "first-group".into(),
        node_id: "first-node".into(),
        api_url: format!("http://{address}/"),
        token_env: "HOME".into(),
    };
    let first_token = std::env::var(&config.token_env).unwrap();
    assert_eq!(
        get::<serde_json::Value>(&config, Endpoint::Freshness)
            .await
            .unwrap()["ok"],
        true
    );
    assert_eq!(
        post::<serde_json::Value>(&config, Endpoint::Search, &serde_json::json!({}))
            .await
            .unwrap()["ok"],
        true
    );
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(b"{}\n").unwrap();
    assert_eq!(upload(&config, &file).await.unwrap().revision, 1);
    // Change identities on the same pooled transport, including which token to read.
    config.group_id = "second-group".into();
    config.node_id = "second-node".into();
    config.token_env = "USER".into();
    let second_token = std::env::var(&config.token_env).unwrap();
    assert_eq!(
        get::<serde_json::Value>(&config, Endpoint::Freshness)
            .await
            .unwrap()["ok"],
        true
    );
    let requests = received.lock().unwrap();
    assert_eq!(requests.len(), 4);
    assert!(requests.iter().all(|r| r.peer == requests[0].peer));
    assert_eq!(requests[0].method, Method::GET);
    assert_eq!(requests[1].method, Method::POST);
    assert_eq!(requests[2].method, Method::POST);
    assert_eq!(requests[2].headers["content-type"], "application/x-ndjson");
    for request in &requests[..3] {
        assert_eq!(
            request.headers["authorization"],
            format!("Bearer {first_token}")
        );
        assert_eq!(request.headers["x-mash-group"], "first-group");
        assert_eq!(request.headers["x-mash-node"], "first-node");
    }
    assert_eq!(
        requests[3].headers["authorization"],
        format!("Bearer {second_token}")
    );
    assert_eq!(requests[3].headers["x-mash-group"], "second-group");
    assert_eq!(requests[3].headers["x-mash-node"], "second-node");
    drop(requests);
    server.abort();
}

fn seed(path: &Path) -> Storage {
    let mut storage = Storage::open_writer(path).expect("writer");
    storage
        .replace_conversation(&Conversation {
            id: "conversation".to_owned(),
            provider: crate::provider::Provider::Codex,
            title_origin: None,
            timestamp_origins: None,
            claude_lineage: None,
            source: crate::silver::SourceIdentity::file("/tmp/shared-fixture.jsonl"),
            project: None,
            repository: None,
            title: Some("fixture".to_owned()),
            created_at: Some(1),
            updated_at: Some(2),
            messages: vec![
                NormalizedMessage {
                    attachments: Vec::new(),
                    id: "searchable".to_owned(),
                    ordinal: 0,
                    context: crate::message_context::MessageContext::Linear,
                    role: crate::role::Role::User,
                    content: "hello shared history".to_owned(),
                    dialogue_text: None,
                    created_at: Some(1),
                },
                NormalizedMessage {
                    attachments: Vec::new(),
                    id: "context".to_owned(),
                    ordinal: 1,
                    context: crate::message_context::MessageContext::Linear,
                    role: crate::role::Role::Tool,
                    content: "hello raw tool output".to_owned(),
                    dialogue_text: Some(String::new()),
                    created_at: Some(2),
                },
            ]
            .into(),
        })
        .expect("conversation");
    storage
        .replace_embeddings(
            semantic::embedding_generation(),
            &[EmbeddingWrite {
                message_id: "searchable",
                vector: &[127, 0],
                norm: 127.0,
            }],
        )
        .expect("embedding");
    storage.commit_writer().expect("commit");
    storage
}

#[test]
fn local_candidates_use_the_shared_query_vector_and_keep_context_out_of_search() {
    let dir = tempfile::tempdir().unwrap();
    let storage = seed(&dir.path().join("local.sqlite3"));
    let (hits, documents) = semantic::local_candidates(
        &storage,
        semantic::embedding_generation(),
        "hello",
        &[127.0, 0.0],
        (None, None, SearchUnit::Messages),
        None,
        crate::gold::config::Fusion::default(),
    )
    .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "searchable");
    assert_eq!(hits[0].node_id, None);
    assert!(hits[0].semantic_score.unwrap() > 0.99);
    assert_eq!(documents, ["hello shared history"]);
    assert!(
        semantic::local_candidates(
            &storage,
            semantic::embedding_generation(),
            "hello",
            &[127.0, 0.0],
            (
                Some(crate::provider::Provider::ClaudeCode),
                None,
                SearchUnit::Messages
            ),
            None,
            crate::gold::config::Fusion::default(),
        )
        .unwrap()
        .0
        .is_empty()
    );
}

#[test]
fn local_rich_filters_apply_before_lexical_and_semantic_candidate_limits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("filters.sqlite3");
    let mut storage = Storage::open_writer(&path).unwrap();
    let mut conversation = Conversation {
        id: "conversation".into(),
        provider: crate::provider::Provider::Codex,
        title_origin: None,
        timestamp_origins: None,
        claude_lineage: None,
        source: crate::silver::SourceIdentity::file("/tmp/filters.jsonl"),
        project: Some("/work/o'reilly".into()),
        repository: Some("repo://alpha".into()),
        title: None,
        created_at: None,
        updated_at: None,
        messages: Vec::new().into(),
    };
    for i in 0..61 {
        conversation.messages.inline_mut().push(NormalizedMessage {
            attachments: Vec::new(),
            id: format!("message-{i:02}"),
            ordinal: i,
            context: crate::message_context::MessageContext::Linear,
            role: if i == 60 { "assistant" } else { "user" }.into(),
            content: "hello filter".into(),
            dialogue_text: None,
            created_at: Some(2000),
        });
    }
    storage.replace_conversation(&conversation).unwrap();
    let embeddings = conversation
        .messages
        .inline()
        .iter()
        .map(|m| EmbeddingWrite {
            message_id: &m.id,
            vector: &[127, 0],
            norm: 127.0,
        })
        .collect::<Vec<_>>();
    storage
        .replace_embeddings(semantic::embedding_generation(), &embeddings)
        .unwrap();
    storage.commit_writer().unwrap();
    let filter = crate::search_filter::Filter::All(vec![
        crate::search_filter::Filter::Node(vec!["local-node".into()]),
        crate::search_filter::Filter::Role(vec!["assistant".into()]),
        crate::search_filter::Filter::Project(vec!["/work/o'reilly".into()]),
        crate::search_filter::Filter::Repository(vec!["repo://alpha".into()]),
        crate::search_filter::Filter::After(2),
        crate::search_filter::Filter::Before(3),
    ]);
    filter.validate().unwrap();
    let scope = crate::search_filter::Scope {
        match_mode: crate::gold::matching::Mode::Strict,
        filter,
        node: "local-node".into(),
    };
    let (hits, _) = semantic::local_candidates(
        &storage,
        semantic::embedding_generation(),
        "hello",
        &[127.0, 0.0],
        (None, None, SearchUnit::Messages),
        Some(&scope),
        crate::gold::config::Fusion::default(),
    )
    .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "message-60");
    assert_eq!(hits[0].metadata.project.as_deref(), Some("/work/o'reilly"));
    assert_eq!(hits[0].metadata.repository.as_deref(), Some("repo://alpha"));
    assert_eq!(hits[0].created_at, Some(2000));
    assert!(hits[0].lexical_score.is_some() && hits[0].semantic_score.is_some());
    assert_eq!(
        storage
            .search_filtered("hello", 1, None, None, Some(&scope))
            .unwrap()[0]
            .id,
        "message-60"
    );
}

#[tokio::test]
async fn publication_state_survives_failed_delivery_and_forget() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("local.sqlite3");
    let mut storage = seed(&path);
    let unavailable = SharedConfig {
        group_id: "test".to_owned(),
        node_id: "test".to_owned(),
        api_url: "http://127.0.0.1:9".to_owned(),
        token_env: "MASH_TEST_UNCONFIGURED_TOKEN_71B9".to_owned(),
    };
    let delivery = publish(&path, &unavailable, None).await;
    assert_eq!(delivery.state, PublicationState::Pending);
    assert!(delivery.error.is_some());
    assert!(Storage::publication_is_pending(&path, "target").expect("pending"));
    let revision = storage
        .with_publication_snapshot(|snapshot| {
            let mut messages = Vec::new();
            snapshot.visit(protocol::BatchKind::Messages, |batch| {
                let protocol::Batch::Messages { records } = batch else {
                    panic!("message batch")
                };
                messages.extend(records);
                Ok(())
            })?;
            assert_eq!(messages.len(), 2);
            let tool = messages.iter().find(|m| m.id == "context").expect("tool");
            assert_eq!(tool.projection, "");
            assert_eq!(tool.content, "hello raw tool output");
            Ok(())
        })
        .expect("snapshot");
    storage
        .prepare_publication_target("target")
        .expect("prepare destination");
    storage
        .acknowledge_publication("target", revision)
        .expect("ack");
    assert!(!Storage::publication_is_pending(&path, "target").expect("acked"));
    assert!(
        Storage::publication_is_pending(&path, "different destination").expect("target change")
    );
    storage.forget("conversation").expect("forget");
    assert!(Storage::publication_is_pending(&path, "target").expect("pending delete"));
    storage
        .acknowledge_publication("target", revision)
        .expect("stale ack ignored");
    assert!(Storage::publication_is_pending(&path, "target").expect("still pending"));
    storage
        .with_publication_snapshot(|snapshot| {
            snapshot.visit(protocol::BatchKind::Tombstones, |batch| {
                let protocol::Batch::Tombstones { records } = batch else {
                    panic!("tombstone batch")
                };
                assert_eq!(records[0].conversation_id, "conversation");
                Ok(())
            })
        })
        .expect("durable deletion");
}
