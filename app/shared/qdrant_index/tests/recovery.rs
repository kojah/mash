//! Shared views are derived: losing it never loses history, and silver rebuilds it.
use super::*;

/// Delete this group's Qdrant collections, as if the views backend were lost.
async fn lose_views(fixture: &Fixture) {
    fixture.cleanup().await;
}

async fn reconnect(fixture: &Fixture) -> Server {
    let views = Store::connect(
        &fixture.url,
        None,
        std::slice::from_ref(&fixture.identity.group_id),
        fixture.identity.dimensions,
        64 * 1024 * 1024,
        VectorIndex::default(),
        MemoryTier::Cached.into(),
    )
    .await
    .unwrap();
    views.select_views(&fixture.identity, false).await.unwrap();
    fixture.serve(views)
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_lost_views_is_reported_and_a_resent_snapshot_rebuilds_it() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    fixture
        .server
        .index(&fixture.identity, fixture.file(1, "durable"))
        .await
        .unwrap();
    let silver = fixture.retained();
    let state = fixture.server.state(&fixture.identity).await.unwrap();
    lose_views(&fixture).await;
    let server = reconnect(&fixture).await;
    // Silver, revisions and metadata survive; views are reported missing.
    assert_eq!(fixture.retained(), silver);
    assert_eq!(server.state(&fixture.identity).await.unwrap().revision, 1);
    let freshness = server.freshness(&fixture.identity).await.unwrap();
    assert!(!freshness.nodes[0].lexical_current);
    assert!(!freshness.nodes[0].views_current);
    assert_eq!(freshness.nodes[0].silver_revision, state.silver_revision);
    let threads = server
        .list_chats(&fixture.identity, crate::chat_catalog::Request::default())
        .await
        .unwrap();
    assert_eq!(threads.chats.len(), 1);
    assert!(
        server
            .search(&fixture.identity, fixture.query())
            .await
            .unwrap()
            .documents
            .is_empty()
    );
    // A node resending the same snapshot (unacknowledged publication) repairs views.
    server
        .index(&fixture.identity, fixture.file(1, "durable"))
        .await
        .unwrap();
    assert_eq!(fixture.retained(), silver);
    assert!(server.freshness(&fixture.identity).await.unwrap().nodes[0].views_current);
    assert_eq!(
        server
            .search(&fixture.identity, fixture.query())
            .await
            .unwrap()
            .semantic
            .len(),
        2
    );
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_publication_commits_silver_before_views_and_stale_views_is_never_current() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    fixture
        .server
        .index(&fixture.identity, fixture.file(1, "first"))
        .await
        .unwrap();
    // View becomes unreachable after the configuration check: silver still commits.
    lose_views(&fixture).await;
    assert!(
        fixture
            .server
            .index(&fixture.identity, fixture.file(2, "second"))
            .await
            .is_err()
    );
    let server = reconnect(&fixture).await;
    let state = server.state(&fixture.identity).await.unwrap();
    assert!(
        state.revision == 1 || state.revision == 2,
        "the rejected upload either committed completely or not at all"
    );
    assert!(!server.freshness(&fixture.identity).await.unwrap().nodes[0].views_current);
    server
        .index(&fixture.identity, fixture.file(3, "third"))
        .await
        .unwrap();
    let freshness = server.freshness(&fixture.identity).await.unwrap();
    assert!(freshness.nodes[0].views_current);
    let results = server
        .search(&fixture.identity, fixture.query())
        .await
        .unwrap();
    assert!(
        results
            .documents
            .iter()
            .all(|d| d.projection.contains("third"))
    );
    fixture.cleanup().await;
}

mod embeddings {
    use axum::{Json, Router, routing::post};
    use serde::{Deserialize, Serialize};

    #[derive(Deserialize)]
    struct Request {
        input: Vec<String>,
        dimensions: Option<usize>,
    }
    #[derive(Serialize)]
    struct Output {
        data: Vec<Item>,
    }
    #[derive(Serialize)]
    struct Item {
        index: usize,
        embedding: Vec<f32>,
    }

    async fn embed(Json(input): Json<Request>) -> Json<Output> {
        let dimensions = input.dimensions.unwrap_or(4);
        Json(Output {
            data: input
                .input
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    let mut embedding = vec![0.0; dimensions];
                    embedding[0] = 1.0;
                    Item { index, embedding }
                })
                .collect(),
        })
    }

    /// A controlled OpenAI-compatible endpoint on its own runtime thread.
    pub(super) fn serve() -> String {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(async move {
                    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                    sender
                        .send(format!(
                            "http://{}/embeddings",
                            listener.local_addr().unwrap()
                        ))
                        .unwrap();
                    axum::serve(listener, Router::new().route("/embeddings", post(embed)))
                        .await
                        .unwrap();
                });
        });
        receiver
            .recv_timeout(crate::test_support::HANG_GUARD)
            .unwrap()
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_replay_interruption_never_leaves_current_metadata_and_retry_repairs() {
    use crate::shared::server::store::ReplayFault;

    let fixture = Fixture::new(VectorIndex::default()).await;
    let remote = crate::semantic::remote::Configuration {
        url: embeddings::serve(),
        model: "test".into(),
        dimensions: 4,
        api: crate::semantic::remote::Api::OpenaiCompatible,
        token_env: None,
        request_dimensions: true,
        redaction: crate::redaction::Policy::ON,
    };
    let models = tempfile::tempdir().unwrap();
    for fault in [ReplayFault::AfterDeletion, ReplayFault::AfterInvalidation] {
        fixture
            .server
            .index(&fixture.identity, fixture.file(1, "durable"))
            .await
            .unwrap();
        let silver = fixture.retained();
        let head = fixture.server.state(&fixture.identity).await.unwrap();
        assert!(
            fixture
                .server
                .freshness(&fixture.identity)
                .await
                .unwrap()
                .nodes[0]
                .views_current
        );
        let interrupted = fixture
            .serve(fixture.store.clone())
            .with_replay_fault(fault);
        let error = interrupted
            .replay(&fixture.identity, models.path(), Some(&remote))
            .await
            .unwrap_err();
        assert_eq!(error.error.message, "injected replay interruption");
        drop(interrupted);

        // A new service handle must observe the durable stale marker. This is
        // a boundary interruption, not a claim of process/power-loss testing.
        let reopened = reconnect(&fixture).await;
        let freshness = reopened.freshness(&fixture.identity).await.unwrap();
        assert!(!freshness.nodes[0].views_current);
        assert!(!freshness.nodes[0].lexical_current);
        assert_eq!(freshness.nodes[0].silver_revision, head.silver_revision);
        assert_eq!(fixture.retained(), silver);
        assert!(
            reopened
                .search(&fixture.identity, fixture.query())
                .await
                .is_err()
        );

        reopened
            .replay(&fixture.identity, models.path(), Some(&remote))
            .await
            .unwrap();
        assert!(reopened.freshness(&fixture.identity).await.unwrap().nodes[0].views_current);
        assert_eq!(fixture.retained(), silver);
        let results = reopened
            .search(&fixture.identity, fixture.query())
            .await
            .unwrap();
        assert_eq!(results.lexical.len(), 2);
        assert_eq!(results.semantic.len(), 2);
    }
    fixture.cleanup().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_replay_reconstructs_all_views_from_shared_silver() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    fixture
        .server
        .index(&fixture.identity, fixture.file(1, "replayed"))
        .await
        .unwrap();
    let silver = fixture.retained();
    lose_views(&fixture).await;
    let server = reconnect(&fixture).await;
    let remote = crate::semantic::remote::Configuration {
        url: embeddings::serve(),
        model: "test".into(),
        dimensions: 4,
        api: crate::semantic::remote::Api::OpenaiCompatible,
        token_env: None,
        request_dimensions: true,
        redaction: crate::redaction::Policy::ON,
    };
    let models = tempfile::tempdir().unwrap();
    let replayed = server
        .replay(&fixture.identity, models.path(), Some(&remote))
        .await
        .unwrap();
    assert_eq!(replayed.chats, 1);
    assert_eq!(replayed.messages, 2);
    assert_eq!(replayed.embeddings, 2);
    assert_eq!(fixture.retained(), silver);
    let freshness = server.freshness(&fixture.identity).await.unwrap();
    assert!(freshness.nodes[0].lexical_current);
    assert!(freshness.nodes[0].views_current);
    let results = server
        .search(&fixture.identity, fixture.query())
        .await
        .unwrap();
    assert_eq!(results.lexical.len(), 2);
    assert_eq!(results.semantic.len(), 2);
    let view = server
        .view(
            &fixture.identity,
            protocol::View {
                node_id: fixture.identity.node_id.clone(),
                id: "message-1".into(),
                context: 1,
                content: crate::storage::MessageContent::Canonical,
            },
        )
        .await
        .unwrap();
    assert_eq!(view.len(), 2);
    fixture.cleanup().await;
}
