//! Durable authority across independent store handles and deliberate switches.
use super::*;

fn title_store(fixture: &Fixture) -> Store {
    fixture
        .store
        .clone()
        .with_build(crate::gold::config::Build {
            embedding_text: crate::gold::config::EmbeddingText::TitleAndDialogue,
        })
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_group_gold_authority_requires_explicit_activation_and_preserves_silver() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    fixture
        .store
        .index(&fixture.identity, fixture.file(1, "canonical"))
        .await
        .unwrap();
    let old_version = fixture.store.gold_version(&fixture.identity).await.unwrap();
    let before = fixture.store.load(&fixture.identity).await.unwrap();
    let title = title_store(&fixture);
    assert!(title.select_gold(&fixture.identity, false).await.is_err());
    assert!(title.state(&fixture.identity).await.is_err());
    assert_eq!(
        fixture.store.gold_version(&fixture.identity).await.unwrap(),
        old_version
    );
    let activated = title.select_gold(&fixture.identity, true).await.unwrap();
    assert_ne!(activated, old_version);
    assert_eq!(
        title.select_gold(&fixture.identity, false).await.unwrap(),
        activated
    );
    assert_eq!(
        title.select_gold(&fixture.identity, true).await.unwrap(),
        activated
    );
    assert!(
        fixture
            .store
            .search(&fixture.identity, fixture.query())
            .await
            .is_err()
    );
    assert!(
        fixture
            .store
            .index(&fixture.identity, fixture.file(2, "stale-writer"))
            .await
            .is_err()
    );
    assert!(
        fixture
            .store
            .unchanged_gold(&fixture.identity, &old_version)
            .await
            .is_err()
    );
    let after = title.load(&fixture.identity).await.unwrap();
    assert_eq!(after.expected, before.expected);
    assert_eq!(
        after.data.canonical_fingerprint().unwrap(),
        before.data.canonical_fingerprint().unwrap()
    );
    assert!(
        title
            .search(&fixture.identity, fixture.query())
            .await
            .is_err()
    );
    // Lexical retrieval remains available independently of stale dense gold.
    let mut exact = fixture.query();
    exact.intent = crate::gold::exact::Intent::ExactMention;
    exact.vector.clear();
    exact.generation.clear();
    assert_eq!(
        title
            .search(&fixture.identity, exact)
            .await
            .unwrap()
            .lexical
            .len(),
        2
    );
    // Returning to the existing configuration recovers its unchanged vectors.
    fixture
        .store
        .select_gold(&fixture.identity, true)
        .await
        .unwrap();
    assert_eq!(fixture.search().await.semantic.len(), 2);
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_group_gold_bootstrap_race_selects_one_configuration_and_reopens_it() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    // Race two first-start selections in another empty, owned group.
    let mut identity = fixture.identity.clone();
    identity.group_id.push_str("-race");
    let default = Store::connect(
        &fixture.url,
        None,
        std::slice::from_ref(&identity.group_id),
        4,
        64 * 1024 * 1024,
        VectorIndex::default(),
        MemoryTier::Cached,
    )
    .await
    .unwrap();
    let title = default.clone().with_build(title_store(&fixture).build);
    let (a, b) = tokio::join!(
        default.select_gold(&identity, false),
        title.select_gold(&identity, false)
    );
    assert_ne!(
        a.is_ok(),
        b.is_ok(),
        "exactly one conflicting bootstrap may win"
    );
    let winner = if a.is_ok() { default } else { title };
    let version = winner.gold_version(&identity).await.unwrap();
    let reopened = Store::connect(
        &fixture.url,
        None,
        std::slice::from_ref(&identity.group_id),
        4,
        64 * 1024 * 1024,
        VectorIndex::default(),
        MemoryTier::Cached,
    )
    .await
    .unwrap()
    .with_build(winner.build.clone());
    assert_eq!(
        reopened.select_gold(&identity, false).await.unwrap(),
        version
    );
    Fixture {
        store: winner,
        identity,
        url: fixture.url.clone(),
    }
    .cleanup()
    .await;
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_group_gold_corruption_fails_closed_without_overwriting_authority() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    let name = collection(&fixture.identity.group_id, true);
    let id = point_id(&["gold-configuration"]);
    let original = fixture
        .store
        .api
        .retrieve::<serde_json::Value>(&name, std::slice::from_ref(&id))
        .await
        .unwrap()
        .pop()
        .unwrap()
        .payload;
    for defect in 0..6 {
        let mut payload = original.clone();
        match defect {
            0 => payload["schema_version"] = 99.into(),
            1 => payload["version"] = "invalid".into(),
            2 => payload["kind"] = "head".into(),
            3 => payload["dimensions"] = 0.into(),
            4 => payload["generation"] = "".into(),
            _ => payload["unknown"] = true.into(),
        }
        fixture
            .store
            .api
            .upsert(
                &name,
                &api::Upsert {
                    points: vec![api::Point {
                        id: id.clone(),
                        vector: api::Vectors::default(),
                        payload: payload.clone(),
                    }],
                    update_filter: None,
                    update_mode: None,
                },
            )
            .await
            .unwrap();
        assert!(
            fixture
                .store
                .select_gold(&fixture.identity, true)
                .await
                .is_err(),
            "defect {defect}"
        );
        assert!(fixture.store.state(&fixture.identity).await.is_err());
        assert_eq!(
            fixture
                .store
                .api
                .retrieve::<serde_json::Value>(&name, std::slice::from_ref(&id))
                .await
                .unwrap()[0]
                .payload,
            payload
        );
    }
    fixture.cleanup().await;
}
