use super::*;
use crate::gold::config::{Build, EmbeddingText, TextPreparation};

fn requested(fixture: &Fixture) -> Store {
    fixture.store.clone().with_build(Build {
        embedding_text: EmbeddingText::TitleAndDialogue,
        ..Default::default()
    })
}

async fn candidate(fixture: &Fixture) -> CandidateStatus {
    fixture
        .store
        .candidate_status(&fixture.identity)
        .await
        .unwrap()
        .unwrap()
}

async fn seeded() -> Fixture {
    let fixture = Fixture::new(VectorIndex::default()).await;
    fixture
        .store
        .index(&fixture.identity, fixture.file(1, "candidate baseline"))
        .await
        .unwrap();
    fixture
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_candidate_staging_preserves_active_search_and_survives_reopen_and_query_changes() {
    let fixture = seeded().await;
    let before = fixture
        .store
        .active_configuration(&fixture.identity)
        .await
        .unwrap();
    let data =
        serde_json::to_vec(&fixture.store.load(&fixture.identity).await.unwrap().data).unwrap();
    let results = serde_json::to_vec(&fixture.search().await).unwrap();
    assert_eq!(
        requested(&fixture)
            .stage_base(&fixture.identity)
            .await
            .unwrap(),
        Build::default()
    );
    assert!(
        fixture
            .store
            .candidate_status(&fixture.identity)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        requested(&fixture)
            .stage_gold(&fixture.identity)
            .await
            .unwrap(),
        Build::default()
    );
    let status = candidate(&fixture).await;
    assert_eq!(status.validity, CandidateValidity::Pinned);
    assert_eq!(
        status.candidate.build,
        Build {
            embedding_text: EmbeddingText::TitleAndDialogue,
            ..Default::default()
        }
    );
    let after = fixture
        .store
        .active_configuration(&fixture.identity)
        .await
        .unwrap();
    assert_eq!(after.version, before.version);
    assert_eq!(after.build, before.build);
    assert_ne!(after.publication, before.publication);
    assert_eq!(
        serde_json::to_vec(&after.nodes).unwrap(),
        serde_json::to_vec(&before.nodes).unwrap()
    );
    requested(&fixture)
        .stage_gold(&fixture.identity)
        .await
        .unwrap();
    assert_eq!(candidate(&fixture).await, status);
    assert_reopened_candidate(&fixture, &status).await;
    fixture
        .store
        .clone()
        .with_query(VectorIndex::Hnsw { ef_runtime: 80 })
        .select_query(&fixture.identity, true)
        .await
        .unwrap();
    assert_eq!(candidate(&fixture).await, status);
    assert_eq!(
        serde_json::to_vec(&fixture.search().await).unwrap(),
        results
    );
    assert_eq!(
        serde_json::to_vec(&fixture.store.load(&fixture.identity).await.unwrap().data).unwrap(),
        data
    );
    fixture
        .store
        .discard_candidate(&fixture.identity)
        .await
        .unwrap();
    fixture
        .store
        .discard_candidate(&fixture.identity)
        .await
        .unwrap();
    assert!(
        fixture
            .store
            .candidate_status(&fixture.identity)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        serde_json::to_vec(&fixture.store.load(&fixture.identity).await.unwrap().data).unwrap(),
        data
    );
    fixture.cleanup().await;
}

async fn assert_reopened_candidate(fixture: &Fixture, status: &CandidateStatus) {
    let reopened = Store::connect(
        &fixture.url,
        None,
        std::slice::from_ref(&fixture.identity.group_id),
        4,
        64 * 1024 * 1024,
        VectorIndex::default(),
        MemoryTier::Cached,
    )
    .await
    .unwrap();
    reopened
        .select_gold(&fixture.identity, false)
        .await
        .unwrap();
    assert_eq!(
        &reopened
            .candidate_status(&fixture.identity)
            .await
            .unwrap()
            .unwrap(),
        status
    );
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_candidate_edits_and_new_nodes_invalidate_pins_without_blocking_active_search() {
    for new_node in [false, true] {
        let fixture = seeded().await;
        requested(&fixture)
            .stage_gold(&fixture.identity)
            .await
            .unwrap();
        let id = candidate(&fixture).await.candidate.id;
        let publisher = Identity {
            node_id: if new_node {
                "new-node".into()
            } else {
                fixture.identity.node_id.clone()
            },
            ..fixture.identity.clone()
        };
        fixture
            .store
            .index(
                &publisher,
                fixture.file(if new_node { 1 } else { 2 }, "new canonical evidence"),
            )
            .await
            .unwrap();
        let status = candidate(&fixture).await;
        assert_eq!(status.candidate.id, id);
        assert_eq!(status.validity, CandidateValidity::Invalidated);
        assert!(!fixture.search().await.semantic.is_empty());
        let error = requested(&fixture)
            .stage_gold(&fixture.identity)
            .await
            .unwrap_err();
        assert_eq!(error.error.kind, crate::ErrorKind::Conflict);
        fixture
            .store
            .discard_candidate(&fixture.identity)
            .await
            .unwrap();
        requested(&fixture)
            .stage_gold(&fixture.identity)
            .await
            .unwrap();
        let fresh = candidate(&fixture).await;
        assert_ne!(fresh.candidate.id, id);
        assert_eq!(fresh.validity, CandidateValidity::Pinned);
        fixture.cleanup().await;
    }
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_competing_candidate_ownership_never_overwrites_the_winner() {
    let fixture = seeded().await;
    let first = requested(&fixture);
    let second = fixture.store.clone().with_build(Build {
        text_preparation: TextPreparation::CollapseWhitespace,
        ..Default::default()
    });
    let (a, b) = tokio::join!(
        first.stage_gold(&fixture.identity),
        second.stage_gold(&fixture.identity)
    );
    assert_ne!(a.is_ok(), b.is_ok());
    let winner = candidate(&fixture).await;
    let loser = if a.is_ok() { second } else { first };
    assert!(loser.stage_gold(&fixture.identity).await.is_err());
    assert_eq!(candidate(&fixture).await, winner);
    assert_eq!(
        fixture
            .store
            .active_configuration(&fixture.identity)
            .await
            .unwrap()
            .build,
        Build::default()
    );
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_candidate_rejects_noop_and_model_space_changes_without_metadata_writes() {
    let fixture = seeded().await;
    let before = serde_json::to_vec(
        &fixture
            .store
            .active_configuration(&fixture.identity)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(fixture.store.stage_gold(&fixture.identity).await.is_err());
    let incompatible = Identity {
        generation: "different-model".into(),
        ..fixture.identity.clone()
    };
    assert!(requested(&fixture).stage_gold(&incompatible).await.is_err());
    assert_eq!(
        serde_json::to_vec(
            &fixture
                .store
                .active_configuration(&fixture.identity)
                .await
                .unwrap()
        )
        .unwrap(),
        before
    );
    fixture.cleanup().await;
}
