//! Retry contracts across a committed silver write and unfinished views.
use super::*;

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_retry_rejects_changed_or_missing_canonical_messages() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    fixture
        .server
        .index(&fixture.identity, fixture.file(1, "original"))
        .await
        .unwrap();
    let retained = fixture.retained();
    // Retain silver but make views require reconstruction.
    fixture
        .store
        .put_node_views(
            &fixture.identity,
            &NodeViews::empty(&fixture.store, &fixture.identity),
        )
        .await
        .unwrap();
    for messages in [fixture.messages("changed"), Vec::new()] {
        let file = Fixture::upload(&[
            Event::Start(Fixture::start(1)),
            Event::Batch(Batch::Conversations {
                records: vec![Fixture::conversation()],
            }),
            Event::Batch(Batch::Messages { records: messages }),
            Event::Finish,
        ]);
        assert!(
            fixture.server.index(&fixture.identity, file).await.is_err(),
            "same revision must not certify different or missing silver data"
        );
        assert_eq!(fixture.retained(), retained);
        assert!(
            !fixture
                .server
                .freshness(&fixture.identity)
                .await
                .unwrap()
                .nodes[0]
                .views_current
        );
    }
    fixture
        .server
        .index(&fixture.identity, fixture.file(1, "original"))
        .await
        .unwrap();
    assert_eq!(fixture.search().await.semantic.len(), 2);
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_retry_removes_documents_deleted_by_an_interrupted_publication() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    fixture
        .server
        .index(&fixture.identity, fixture.file(1, "removed"))
        .await
        .unwrap();
    // A failed newer write can leave points absent from both silver and the
    // last recorded chat map. Reconciliation must remove these too.
    let mut chat = Fixture::conversation();
    chat.id = "untracked".into();
    let mut partial = Documents::new(&fixture.identity);
    for mut message in fixture.messages("partial") {
        message.id = format!("untracked-{}", message.id);
        message.conversation_id.clone_from(&chat.id);
        partial
            .push(&fixture.store, &fixture.identity, &chat, message)
            .await
            .unwrap();
    }
    partial.finish(&fixture.store).await.unwrap();
    // Complete only the real silver transaction, as when synchronization fails
    // before its first Qdrant write. Retry has no in-memory Applied.deleted set.
    crate::shared::silver_store::apply::write(
        &fixture.silver,
        &fixture.identity,
        &Fixture::forget(2, &["thread"]),
        fixture.store.build(),
        &crate::shared::activity::Progress::default(),
        None,
        false,
    )
    .unwrap();
    let retry = Fixture::upload(&[Event::Start(Fixture::start(2)), Event::Finish]);
    fixture
        .server
        .index(&fixture.identity, retry)
        .await
        .unwrap();
    assert!(
        fixture
            .server
            .freshness(&fixture.identity)
            .await
            .unwrap()
            .nodes[0]
            .views_current
    );
    let results = fixture.search().await;
    assert!(
        results.semantic.is_empty(),
        "forgotten silver must not survive as certified search hits"
    );
    assert!(results.lexical.is_empty());
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_concurrent_publications_keep_views_with_the_latest_accepted_silver() {
    for newer_first in [false, true] {
        let fixture = Fixture::new(VectorIndex::default()).await;
        fixture
            .server
            .index(&fixture.identity, fixture.file(1, "initial"))
            .await
            .unwrap();
        let old = fixture
            .server
            .index(&fixture.identity, fixture.file(2, "older"));
        let new = fixture
            .server
            .index(&fixture.identity, fixture.file(3, "newest"));
        let (older, newer) = if newer_first {
            let (newer, older) = tokio::join!(new, old);
            (older, newer)
        } else {
            tokio::join!(old, new)
        };
        newer.unwrap();
        if let Err(error) = older {
            assert!(error.error.message.contains("newer snapshot"));
        }
        assert_eq!(
            fixture
                .server
                .state(&fixture.identity)
                .await
                .unwrap()
                .revision,
            3
        );
        assert!(
            fixture
                .server
                .freshness(&fixture.identity)
                .await
                .unwrap()
                .nodes[0]
                .views_current
        );
        let results = fixture.search().await;
        assert_eq!(results.semantic.len(), 2);
        assert!(
            results
                .documents
                .iter()
                .all(|doc| doc.projection.contains("newest"))
        );
        fixture.cleanup().await;
    }
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_native_cleanup_preserves_other_nodes() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    let other = Scope {
        node_id: "other".into(),
        ..fixture.identity.clone()
    };
    let conversation = Fixture::conversation();
    for operation in 0..3 {
        for identity in [&fixture.identity, &other] {
            let mut documents = Documents::new(identity);
            let mut message = fixture.messages("native").remove(0);
            message.conversation_id.clone_from(&conversation.id);
            documents
                .push(&fixture.store, identity, &conversation, message)
                .await
                .unwrap();
            documents.finish(&fixture.store).await.unwrap();
        }
        match operation {
            0 => fixture
                .store
                .retain_node_conversations(&fixture.identity, Vec::new())
                .await
                .unwrap(),
            1 => fixture
                .store
                .retain_documents(&fixture.identity, &conversation.id, Vec::new())
                .await
                .unwrap(),
            _ => fixture
                .store
                .delete_node_documents(&fixture.identity)
                .await
                .unwrap(),
        }
        let name = collection(&fixture.identity.group_id, false);
        let native = fixture
            .store
            .api
            .retrieve::<Document>(
                &name,
                &[
                    document_id(&fixture.identity.node_id, "message-0"),
                    document_id(&other.node_id, "message-0"),
                ],
            )
            .await
            .unwrap();
        assert_eq!(native.len(), 1);
        assert_eq!(native[0].payload.node_id, other.node_id);
    }
    fixture.cleanup().await;
}
