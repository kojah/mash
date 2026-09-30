use super::*;
use crate::ingestion::{Conversation, NormalizedMessage};
use crate::storage::EmbeddingWrite;

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
        get::<serde_json::Value>(&config, "status").await.unwrap()["ok"],
        true
    );
    assert_eq!(
        post::<serde_json::Value>(&config, "search", &serde_json::json!({}))
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
        get::<serde_json::Value>(&config, "freshness")
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
            provider: "codex",
            source_path: "/tmp/shared-fixture.jsonl".into(),
            project: None,
            repository: None,
            title: Some("fixture".to_owned()),
            created_at: Some(1),
            updated_at: Some(2),
            messages: vec![
                NormalizedMessage {
                    id: "searchable".to_owned(),
                    ordinal: 0,
                    role: "user".to_owned(),
                    content: "hello shared history".to_owned(),
                    search_projection: None,
                    created_at: Some(1),
                },
                NormalizedMessage {
                    id: "context".to_owned(),
                    ordinal: 1,
                    role: "tool".to_owned(),
                    content: "hello raw tool output".to_owned(),
                    search_projection: Some(String::new()),
                    created_at: Some(2),
                },
            ],
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
        None,
        None,
        None,
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
            Some("claude-code"),
            None,
            None
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
        provider: "codex",
        source_path: "/tmp/filters.jsonl".into(),
        project: Some("/work/o'reilly".into()),
        repository: Some("repo://alpha".into()),
        title: None,
        created_at: None,
        updated_at: None,
        messages: Vec::new(),
    };
    for i in 0..61 {
        conversation.messages.push(NormalizedMessage {
            id: format!("message-{i:02}"),
            ordinal: i,
            role: if i == 60 { "assistant" } else { "user" }.into(),
            content: "hello filter".into(),
            search_projection: None,
            created_at: Some(2000),
        });
    }
    storage.replace_conversation(&conversation).unwrap();
    let embeddings = conversation
        .messages
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
        filter,
        node: "local-node".into(),
    };
    let (hits, _) = semantic::local_candidates(
        &storage,
        semantic::embedding_generation(),
        "hello",
        &[127.0, 0.0],
        None,
        None,
        Some(&scope),
    )
    .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "message-60");
    assert!(hits[0].lexical_score.is_some() && hits[0].semantic_score.is_some());
    assert_eq!(
        storage
            .search_filtered("hello", 1, None, None, Some(&scope))
            .unwrap()[0]
            .id,
        "message-60"
    );
}

#[test]
fn routed_forget_acknowledgements_preserve_newer_intents_and_pending_indexing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("local.sqlite3");
    let storage = seed(&path);
    storage.queue_shared_forget("group-node", "first").unwrap();
    let first = storage.direct_owner("group-node").unwrap();
    storage.queue_shared_forget("group-node", "second").unwrap();
    storage.acknowledge_direct("routed", &first, true).unwrap();
    let pending = storage.direct_owner("group-node").unwrap();
    assert_eq!(pending.forgets, ["first", "second"]);
    storage
        .acknowledge_direct("routed", &pending, false)
        .unwrap();
    assert!(
        Storage::publication_is_pending(&path, "routed").unwrap(),
        "deletion does not acknowledge other unpublished index changes"
    );
    let indexed = storage.direct_owner("group-node").unwrap();
    assert_eq!(indexed.forgets, Vec::<String>::new());
    storage
        .acknowledge_direct("routed", &indexed, true)
        .unwrap();
    assert!(!Storage::publication_is_pending(&path, "routed").unwrap());
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
    let delivery = publish(&path, &unavailable).await;
    assert_eq!(delivery.state, "pending");
    assert!(delivery.error.is_some());
    assert!(Storage::publication_is_pending(&path, "target").expect("pending"));
    let revision = storage
        .with_publication_snapshot(|snapshot| {
            let mut messages = Vec::new();
            snapshot.visit("messages", |batch| {
                messages.extend_from_slice(batch);
                Ok(())
            })?;
            assert_eq!(messages.len(), 2);
            let tool = messages
                .iter()
                .find(|m| m["id"] == "context")
                .expect("tool");
            assert_eq!(tool["projection"], "");
            assert_eq!(tool["content"], "hello raw tool output");
            Ok(())
        })
        .expect("snapshot");
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
            snapshot.visit("tombstones", |batch| {
                assert_eq!(batch[0]["conversation_id"], "conversation");
                Ok(())
            })
        })
        .expect("durable deletion");
}
