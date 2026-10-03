use super::*;
use crate::shared::protocol::{self, Batch, ConversationRecord, Event, MessageRecord, Start};
use std::io::Write;
mod exact;
mod memory;
mod publication;
mod recipe;

struct Fixture {
    store: Store,
    identity: Identity,
    url: String,
}
impl Fixture {
    async fn new(index: VectorIndex) -> Self {
        Self::with_memory(index, MemoryTier::Cached).await
    }

    async fn with_memory(index: VectorIndex, memory: MemoryTier) -> Self {
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).unwrap();
        let group = format!("qdrant-test-{}", hex::encode(random));
        let url = std::env::var("MASH_TEST_QDRANT_URL").unwrap();
        let identity = Identity {
            group_id: group.clone(),
            node_id: "node".into(),
            generation: crate::semantic::model_space("test", 4),
            dimensions: 4,
        };
        let store = Store::connect(&url, None, &[group], 4, 64 * 1024 * 1024, index, memory)
            .await
            .unwrap();
        Self {
            store,
            identity,
            url,
        }
    }
    async fn cleanup(&self) {
        for meta in [false, true] {
            reqwest::Client::new()
                .delete(format!(
                    "{}/collections/{}",
                    self.url,
                    collection(&self.identity.group_id, meta)
                ))
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap();
        }
    }
    fn file(&self, revision: i64, marker: &str) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let events = vec![
            Event::Start(Start {
                expected_version: None,
                gold_build: None,
                source_silver: None,
                instance: "instance".into(),
                revision: Some(revision),
                base_revision: None,
                snapshot: true,
                forgets: vec![],
            }),
            Event::Batch(Batch::Conversations {
                records: vec![ConversationRecord {
                    enrichment: None,
                    id: "thread".into(),
                    provider: crate::provider::Provider::Codex,
                    title: Some("Needle".into()),
                    project: Some("project".into()),
                    repository: Some("repo".into()),
                    created_at: Some(1000),
                    updated_at: Some(2000),
                }],
            }),
            Event::Batch(Batch::Messages {
                records: (0..2)
                    .map(|i| MessageRecord {
                        id: format!("message-{i}"),
                        conversation_id: "thread".into(),
                        ordinal: i,
                        context: crate::message_context::MessageContext::Linear,
                        role: crate::role::Role::User,
                        content: format!("needle canonical {marker}"),
                        projection: format!("needle {marker}"),
                        created_at: Some(1000 + i * 1000),
                        generation: Some(self.identity.generation.clone()),
                        vector: Some(vec![127.0, 0.0, 0.0, 0.0]),
                    })
                    .collect(),
            }),
            Event::Finish,
        ];
        for event in events {
            writeln!(file, "{}", serde_json::to_string(&event).unwrap()).unwrap();
        }
        file
    }
    fn query(&self) -> protocol::Search {
        protocol::Search {
            intent: crate::gold::exact::Intent::RelatedDiscussion,
            unit: crate::search_results::SearchUnit::Messages,
            query: "needle".into(),
            generation: self.identity.generation.clone(),
            vector: vec![127.0, 0.0, 0.0, 0.0],
            provider: None,
            days: None,
            exclude_self: false,
            filter: None,
        }
    }

    async fn search(&self) -> protocol::Candidates {
        self.store
            .search(&self.identity, self.query())
            .await
            .unwrap()
    }
}

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn qdrant_mixed_snapshot_migrates_without_losing_embeddings_or_revision() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    fixture
        .store
        .index(&fixture.identity, fixture.file(1, "retained"))
        .await
        .unwrap();
    let data = fixture.store.load(&fixture.identity).await.unwrap().data;
    let expected = serde_json::to_value(&data).unwrap();
    let mut old = fixture.store.node(&fixture.identity).await.unwrap();
    let encoded = crate::shared::snapshot::Encoded::new(&data, 64 * 1024 * 1024).unwrap();
    old.manifest = Some(
        fixture
            .store
            .stage(&fixture.identity, &data, &encoded, &old.version)
            .await
            .unwrap(),
    );
    old.head.as_mut().unwrap().snapshot_schema = crate::shared::snapshot::layers::Schema::Mixed;
    old.head.as_mut().unwrap().silver_revision = None;
    let old_version = old.version.clone();
    fixture
        .store
        .api
        .upsert(
            &collection(&fixture.identity.group_id, true),
            &api::Upsert {
                points: vec![api::Point {
                    id: point_id(&["head", &fixture.identity.node_id]),
                    vector: api::Vectors::default(),
                    payload: old,
                }],
                update_filter: None,
                update_mode: None,
            },
        )
        .await
        .unwrap();
    fixture
        .store
        .index(&fixture.identity, fixture.file(1, "retained"))
        .await
        .unwrap();
    let current = fixture.store.node(&fixture.identity).await.unwrap();
    assert_ne!(current.version, old_version);
    let head = current.head.unwrap();
    assert_eq!(head.revision, 1);
    assert_eq!(head.silver_revision.as_ref().unwrap().revision, 1);
    assert!(matches!(
        head.snapshot_schema,
        crate::shared::snapshot::layers::Schema::Layered
    ));
    let loaded = fixture.store.load(&fixture.identity).await.unwrap();
    assert_eq!(serde_json::to_value(loaded.data).unwrap(), expected);
    let stored: serde_json::Value = fixture
        .store
        .pending_snapshot(&fixture.identity)
        .await
        .unwrap()
        .payload
        .unwrap()
        .decode()
        .unwrap();
    assert!(
        stored["silver"]["messages"]["message-0"]
            .get("vector")
            .is_none()
    );
    assert_eq!(
        stored["gold"]["embeddings"]["message-0"]["vector"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn qdrant_spooled_snapshot_round_trips_unicode_chunks_and_rejects_over_budget() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    let mut plan = crate::shared::snapshot::plan(
        &fixture.file(1, "large"),
        fixture.store.load(&fixture.identity).await.unwrap(),
        &fixture.identity,
        &crate::gold::config::Build::default(),
    )
    .unwrap();
    let content = format!("{}🦀é漢字", "a".repeat(4 * 1024 * 1024));
    plan.loaded
        .data
        .messages
        .get_mut("message-0")
        .unwrap()
        .content = content.clone();
    fixture
        .store
        .commit(&fixture.identity, plan, "instance".into(), true)
        .await
        .unwrap();
    let node = fixture.store.node(&fixture.identity).await.unwrap();
    assert!(node.manifest.as_ref().unwrap().chunks >= 2);
    let loaded = fixture.store.load(&fixture.identity).await.unwrap();
    assert_eq!(loaded.data.messages["message-0"].content, content);
    assert_eq!(
        loaded.data.messages["message-1"].content,
        "needle canonical large"
    );
    let mut plan = crate::shared::snapshot::plan(
        &fixture.file(2, "replacement"),
        loaded,
        &fixture.identity,
        &crate::gold::config::Build::default(),
    )
    .unwrap();
    plan.loaded
        .data
        .messages
        .get_mut("message-0")
        .unwrap()
        .content = content;
    let encoded =
        crate::shared::snapshot::Encoded::new(&plan.loaded.data.layers(), 64 * 1024 * 1024)
            .unwrap();
    let mut limited = fixture.store.clone();
    limited.max_node_bytes = encoded.bytes - 1;
    let error = limited
        .commit(&fixture.identity, plan, "instance".into(), true)
        .await
        .unwrap_err();
    assert_eq!(error.error.kind, crate::ErrorKind::Usage);
    let current = fixture.store.node(&fixture.identity).await.unwrap();
    assert_eq!(current.version, node.version);
    assert_eq!(current.head.unwrap().revision, 1);
    assert!(
        fixture
            .search()
            .await
            .documents
            .iter()
            .all(|d| !d.projection.contains("replacement"))
    );
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn qdrant_conditional_publication_handles_competing_first_and_replacement_uploads() {
    let fixture = Fixture::new(VectorIndex::Hnsw { ef_runtime: 200 }).await;
    for revision in [1, 2] {
        // Both writers prepare against the same head before either can publish.
        let a = crate::shared::snapshot::plan(
            &fixture.file(revision, "alpha"),
            fixture.store.load(&fixture.identity).await.unwrap(),
            &fixture.identity,
            &crate::gold::config::Build::default(),
        )
        .unwrap();
        let b = crate::shared::snapshot::plan(
            &fixture.file(revision, "beta"),
            fixture.store.load(&fixture.identity).await.unwrap(),
            &fixture.identity,
            &crate::gold::config::Build::default(),
        )
        .unwrap();
        let (a, b) = tokio::join!(
            fixture
                .store
                .commit(&fixture.identity, a, "instance".into(), true),
            fixture
                .store
                .commit(&fixture.identity, b, "instance".into(), true)
        );
        assert_ne!(
            a.is_ok(),
            b.is_ok(),
            "exactly one stale-head writer may commit"
        );
        let error = a.err().or_else(|| b.err()).unwrap();
        assert!(matches!(error.error.kind, crate::ErrorKind::Conflict));
        let result = fixture.search().await;
        assert_eq!(result.lexical.len(), 2);
        assert_eq!(result.semantic.len(), 2);
        assert!(
            result
                .documents
                .iter()
                .all(|d| d.projection == result.documents[0].projection)
        );
        assert_eq!(
            fixture
                .store
                .state(&fixture.identity)
                .await
                .unwrap()
                .revision,
            revision
        );
    }
    // A complete staged version has no search visibility until its pointer commits.
    let plan = crate::shared::snapshot::plan(
        &fixture.file(3, "hidden"),
        fixture.store.load(&fixture.identity).await.unwrap(),
        &fixture.identity,
        &crate::gold::config::Build::default(),
    )
    .unwrap();
    fixture
        .store
        .stage(
            &fixture.identity,
            &plan.loaded.data,
            &crate::shared::snapshot::Encoded::new(&plan.loaded.data.layers(), 64 * 1024 * 1024)
                .unwrap(),
            "unpublished-test-version",
        )
        .await
        .unwrap();
    assert!(
        fixture
            .search()
            .await
            .documents
            .iter()
            .all(|d| !d.projection.contains("hidden"))
    );
    for metadata in [false, true] {
        fixture
            .store
            .api
            .delete_version(
                &collection(&fixture.identity.group_id, metadata),
                "unpublished-test-version",
            )
            .await
            .unwrap();
    }
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn qdrant_reopen_schema_mismatch_and_failed_upload_preserve_published_data() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    fixture
        .store
        .index(&fixture.identity, fixture.file(1, "original"))
        .await
        .unwrap();
    let mut bad = fixture.file(2, "broken");
    writeln!(bad, "{{\"type\":\"finish\"}}").unwrap();
    assert!(fixture.store.index(&fixture.identity, bad).await.is_err());
    assert!(
        Store::connect(
            &fixture.url,
            None,
            std::slice::from_ref(&fixture.identity.group_id),
            8,
            64 * 1024 * 1024,
            VectorIndex::default(),
            MemoryTier::Cached,
        )
        .await
        .is_err()
    );
    let reopen = Store::connect(
        &fixture.url,
        None,
        std::slice::from_ref(&fixture.identity.group_id),
        4,
        64 * 1024 * 1024,
        VectorIndex::Hnsw { ef_runtime: 200 },
        MemoryTier::Cached,
    )
    .await
    .unwrap();
    assert_eq!(reopen.state(&fixture.identity).await.unwrap().revision, 1);
    assert_eq!(reopen.status(&fixture.identity).await.unwrap().messages, 2);
    assert!(
        fixture
            .search()
            .await
            .documents
            .iter()
            .all(|d| d.projection.contains("original"))
    );
    let forgotten = reopen
        .forget(
            &fixture.identity,
            protocol::Forget {
                instance: "instance".into(),
                revision: Some(2),
                ids: vec!["thread".into()],
            },
        )
        .await
        .unwrap();
    assert!(forgotten.forgotten);
    assert!(fixture.search().await.documents.is_empty());
    fixture
        .store
        .index(&fixture.identity, fixture.file(3, "delayed"))
        .await
        .unwrap();
    assert_eq!(
        fixture
            .store
            .status(&fixture.identity)
            .await
            .unwrap()
            .messages,
        0
    );
    fixture.cleanup().await;
}

#[test]
fn qdrant_configuration_and_sparse_vectors_are_typed_and_bounded() {
    assert!(
        serde_json::from_str::<VectorIndex>(r#"{"algorithm":"hnsw","ef_runtime":0}"#)
            .unwrap()
            .validate()
            .is_err()
    );
    assert!(
        serde_json::from_str::<VectorIndex>(r#"{"algorithm":"exact","ef_runtime":20}"#).is_err()
    );
    assert!(api::Client::new("https://secret:password@example.com/", None, 1024).is_err());
    assert!(api::Client::new("https://example.com/?secret=value", None, 1024).is_err());
    let a = sparse("cache worker cache", false);
    let b = sparse("worker cache", true);
    assert_eq!(a.indices, b.indices);
    assert!(a.values.iter().all(|v| v.is_finite() && *v > 0.0));
    assert_eq!(b.values, vec![1.0; 2]);
    assert_ne!(point_id(&["ab", "c"]), point_id(&["a", "bc"]));
}

#[tokio::test]
async fn qdrant_rejects_old_server_before_writes_and_sends_key_only_to_origin() {
    use axum::{Json, Router, http::HeaderMap, routing::get};
    #[derive(Serialize)]
    struct Version {
        version: &'static str,
    }
    async fn root(headers: HeaderMap) -> Json<Version> {
        assert_eq!(headers.get("api-key").unwrap(), "private-test-key");
        Json(Version { version: "1.18.9" })
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/", get(root)))
            .await
            .unwrap();
    });
    let result = Store::connect(
        &url,
        Some("private-test-key"),
        &["group".into()],
        4,
        1024,
        VectorIndex::default(),
        MemoryTier::Cached,
    )
    .await;
    let error = result
        .err()
        .expect("old server rejected before any collection route");
    assert!(matches!(error.error.kind, crate::ErrorKind::Configuration));
    assert!(
        !serde_json::to_string(&error)
            .unwrap()
            .contains("private-test-key")
    );
    task.abort();
}

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn qdrant_chunked_utf8_snapshots_and_size_limits_preserve_existing_head() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    // More than one 4 MiB canonical chunk, including a multibyte boundary.
    let marker = "é".repeat(600_000);
    fixture
        .store
        .index(&fixture.identity, fixture.file(1, &marker))
        .await
        .unwrap();
    let loaded = fixture.store.load(&fixture.identity).await.unwrap();
    assert!(
        loaded
            .data
            .messages
            .values()
            .all(|m| m.projection.ends_with(&marker))
    );
    let mut limited = fixture.store.clone();
    limited.max_node_bytes = 1024;
    assert!(
        limited
            .index(&fixture.identity, fixture.file(2, &marker))
            .await
            .is_err()
    );
    assert_eq!(
        fixture
            .store
            .state(&fixture.identity)
            .await
            .unwrap()
            .revision,
        1
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn qdrant_missing_old_chunk_after_publication_is_a_conflict() {
    use axum::{Json, Router, extract::State, routing::post};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    #[derive(Deserialize)]
    struct Ids {
        ids: Vec<String>,
    }
    #[derive(Serialize)]
    struct Record {
        id: String,
        payload: NodeHead,
    }
    #[derive(Serialize)]
    struct Reply {
        status: &'static str,
        result: Vec<Record>,
    }
    async fn retrieve(
        State(calls): State<Arc<AtomicUsize>>,
        Json(input): Json<Ids>,
    ) -> Json<Reply> {
        let result = if input.ids == vec![point_id(&["head", "node"])] {
            let revision = calls.fetch_add(1, Ordering::SeqCst) + 1;
            let version = format!("version-{revision}");
            vec![Record {
                id: input.ids[0].clone(),
                payload: NodeHead {
                    kind: PointKind::Head,
                    node_id: "node".into(),
                    version: version.clone(),
                    head: Some(Head {
                        lexical_manifest: None,
                        gold_manifest: None,
                        silver_coverage: Vec::new(),
                        silver_revision: None,
                        silver_scans: Vec::new(),
                        source_silver: None,
                        snapshot_schema: crate::shared::snapshot::layers::Schema::Mixed,
                        instance: "instance".into(),
                        generation: "test".into(),
                        revision: i64::try_from(revision).unwrap(),
                        version,
                        counts: crate::storage::Counts::default(),
                        last_synced_at: None,
                        last_updated_at: None,
                        metadata_ready: true,
                        catalog_ready: true,
                    }),
                    manifest: Some(Manifest {
                        bytes: 10,
                        chunks: 1,
                        digest: "unused".into(),
                    }),
                },
            }]
        } else {
            vec![]
        }; // Winner reclaimed the old chunk between the two head reads.
        Json(Reply {
            status: "ok",
            result,
        })
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let calls = Arc::new(AtomicUsize::new(0));
    let identity = Identity {
        group_id: "group".into(),
        node_id: "node".into(),
        generation: "test".into(),
        dimensions: 4,
    };
    let path = format!("/collections/{}/points", collection("group", true));
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route(&path, post(retrieve)).with_state(calls),
        )
        .await
        .unwrap();
    });
    let store = Store {
        build: crate::gold::config::Build::default(),
        api: api::Client::new(&url, None, 1024).unwrap(),
        max_node_bytes: 1024,
        vector_index: VectorIndex::default(),
        snapshot_budget: super::super::snapshot_budget::Budget::default(),
    };
    let error = store.load(&identity).await.err().expect("stale snapshot");
    assert!(matches!(error.error.kind, crate::ErrorKind::Conflict));
    task.abort();
}

#[tokio::test]
async fn qdrant_upserts_respect_upstream_body_limit_without_losing_records() {
    use axum::{
        Json, Router,
        extract::{DefaultBodyLimit, State},
        routing::put,
    };
    use std::sync::{Arc, Mutex};
    #[derive(Serialize)]
    struct Blob {
        body: String,
    }
    #[derive(Deserialize)]
    struct IncomingPoint {
        id: String,
    }
    #[derive(Deserialize)]
    struct Incoming {
        points: Vec<IncomingPoint>,
    }
    #[derive(Serialize)]
    struct Operation {
        status: &'static str,
    }
    #[derive(Serialize)]
    struct Reply {
        status: &'static str,
        result: Operation,
    }
    async fn upload(
        State(ids): State<Arc<Mutex<Vec<String>>>>,
        Json(input): Json<Incoming>,
    ) -> Json<Reply> {
        ids.lock()
            .unwrap()
            .extend(input.points.into_iter().map(|p| p.id));
        Json(Reply {
            status: "ok",
            result: Operation {
                status: "completed",
            },
        })
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let ids = Arc::new(Mutex::new(Vec::new()));
    let captured = ids.clone();
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/collections/test/points", put(upload))
                .with_state(captured)
                .layer(DefaultBodyLimit::max(32 * 1024 * 1024)),
        )
        .await
        .unwrap();
    });
    let points: Vec<_> = (0..20)
        .map(|i| api::Point {
            id: format!("point-{i}"),
            vector: api::Vectors::default(),
            payload: Blob {
                body: "x".repeat(2 * 1024 * 1024),
            },
        })
        .collect();
    api::Client::new(&url, None, 64 * 1024 * 1024)
        .unwrap()
        .upsert(
            "test",
            &api::Upsert {
                points,
                update_filter: None,
                update_mode: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        *ids.lock().unwrap(),
        (0..20).map(|i| format!("point-{i}")).collect::<Vec<_>>()
    );
    task.abort();
}
