//! Durable authority across independent store handles and deliberate switches.
use super::*;

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_group_query_activation_preserves_gold_and_silver_across_processes() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    let other = Fixture::new(VectorIndex::default()).await;
    fixture
        .store
        .index(&fixture.identity, fixture.file(1, "retained"))
        .await
        .unwrap();
    let before = fixture
        .store
        .active_configuration(&fixture.identity)
        .await
        .unwrap();
    let data =
        serde_json::to_vec(&fixture.store.load(&fixture.identity).await.unwrap().data).unwrap();
    let query = VectorIndex::Hnsw { ef_runtime: 80 };
    let alternate = fixture.store.clone().with_query(query);
    let error = alternate
        .select_query(&fixture.identity, false)
        .await
        .unwrap_err();
    assert!(matches!(error.error.kind, crate::ErrorKind::Configuration));
    alternate
        .select_query(&fixture.identity, true)
        .await
        .unwrap();
    // Even an API handle configured for exact reads the new persisted policy.
    let after = fixture
        .store
        .active_configuration(&fixture.identity)
        .await
        .unwrap();
    assert_eq!(after.vector_search, query);
    assert_eq!(after.version, before.version);
    assert_eq!(after.build, before.build);
    assert_eq!(after.generation, before.generation);
    assert_eq!(
        serde_json::to_vec(&after.nodes).unwrap(),
        serde_json::to_vec(&before.nodes).unwrap()
    );
    assert_eq!(
        serde_json::to_vec(&fixture.store.load(&fixture.identity).await.unwrap().data).unwrap(),
        data
    );
    assert_eq!(
        fixture
            .store
            .gold_configuration(&fixture.identity)
            .await
            .unwrap()
            .vector_search,
        query
    );
    assert_eq!(fixture.search().await.semantic.len(), 2);
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
    assert!(
        reopened
            .select_query(&fixture.identity, false)
            .await
            .is_err()
    );
    assert_eq!(
        reopened
            .search(&fixture.identity, fixture.query())
            .await
            .unwrap()
            .semantic
            .len(),
        2
    );
    assert_eq!(
        other
            .store
            .active_configuration(&other.identity)
            .await
            .unwrap()
            .vector_search,
        VectorIndex::default()
    );

    fixture.cleanup().await;
    other.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_query_policy_survives_delayed_writers_publication_and_build_activation() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    fixture
        .store
        .index(&fixture.identity, fixture.file(1, "retained"))
        .await
        .unwrap();
    let before = fixture
        .store
        .active_configuration(&fixture.identity)
        .await
        .unwrap();
    let query = VectorIndex::Hnsw { ef_runtime: 80 };
    fixture
        .store
        .clone()
        .with_query(query)
        .select_query(&fixture.identity, true)
        .await
        .unwrap();
    let after = fixture
        .store
        .active_configuration(&fixture.identity)
        .await
        .unwrap();
    // A delayed catalogue write must not restore an old query policy.
    let mut delayed = before.clone();
    delayed.nodes.as_mut().unwrap().clear();
    delayed.publication = Some(super::super::publish::staging_version().unwrap());
    fixture
        .store
        .replace_configuration(&fixture.identity, delayed, Some(&before))
        .await
        .unwrap();
    let observed = fixture
        .store
        .active_configuration(&fixture.identity)
        .await
        .unwrap();
    assert_eq!(observed.publication, after.publication);
    assert_eq!(observed.vector_search, query);

    // Later publications and build activations preserve independent query defaults.
    fixture
        .store
        .index(&fixture.identity, fixture.file(2, "updated"))
        .await
        .unwrap();
    assert_eq!(
        fixture
            .store
            .active_configuration(&fixture.identity)
            .await
            .unwrap()
            .vector_search,
        query
    );
    let title = title_store(&fixture);
    title.select_gold(&fixture.identity, true).await.unwrap();
    assert_eq!(
        title
            .active_configuration(&fixture.identity)
            .await
            .unwrap()
            .vector_search,
        query
    );
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_invalid_query_activation_leaves_authority_unchanged() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    let before = fixture
        .store
        .active_configuration(&fixture.identity)
        .await
        .unwrap();
    let invalid = fixture
        .store
        .clone()
        .with_query(VectorIndex::Hnsw { ef_runtime: 0 });
    assert!(invalid.select_query(&fixture.identity, true).await.is_err());
    let after = fixture
        .store
        .active_configuration(&fixture.identity)
        .await
        .unwrap();
    assert_eq!(after.publication, before.publication);
    assert_eq!(after.vector_search, before.vector_search);
    fixture.cleanup().await;
}

fn title_store(fixture: &Fixture) -> Store {
    fixture
        .store
        .clone()
        .with_build(crate::gold::config::Build {
            max_embedding_chars: None,
            representations: crate::gold::config::Representations::LexicalAndDense,
            text_preparation: crate::gold::config::TextPreparation::Preserve,
            embedding_text: crate::gold::config::EmbeddingText::TitleAndDialogue,
        })
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_delayed_catalogue_publication_cannot_overwrite_activation() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    fixture
        .store
        .index(&fixture.identity, fixture.file(1, "retained"))
        .await
        .unwrap();
    let previous = fixture
        .store
        .active_configuration(&fixture.identity)
        .await
        .unwrap();
    let mut delayed = previous.clone();
    delayed.nodes.as_mut().unwrap().clear();
    delayed.publication = Some(super::super::publish::staging_version().unwrap());
    let title = title_store(&fixture);
    title.select_gold(&fixture.identity, true).await.unwrap();
    let selected = title.active_configuration(&fixture.identity).await.unwrap();
    let before = title.load(&fixture.identity).await.unwrap();
    fixture
        .store
        .replace_configuration(&fixture.identity, delayed, Some(&previous))
        .await
        .unwrap();
    let observed = title.active_configuration(&fixture.identity).await.unwrap();
    assert_eq!(observed.version, selected.version);
    assert_eq!(observed.publication, selected.publication);
    assert_eq!(observed.nodes.unwrap().len(), 1);
    let after = title.load(&fixture.identity).await.unwrap();
    assert_eq!(before.expected, after.expected);
    assert_eq!(
        before.data.canonical_fingerprint().unwrap(),
        after.data.canonical_fingerprint().unwrap()
    );
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_concurrent_nodes_publish_without_losing_each_others_references() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    let mut peer = fixture.identity.clone();
    peer.node_id = "peer".into();
    let (a, b) = tokio::join!(
        fixture
            .store
            .index(&fixture.identity, fixture.file(1, "first")),
        fixture.store.index(&peer, fixture.file(1, "second"))
    );
    a.unwrap();
    b.unwrap();
    let selected = fixture
        .store
        .active_configuration(&fixture.identity)
        .await
        .unwrap();
    assert_eq!(selected.nodes.unwrap().len(), 2);
    assert_ne!(
        fixture
            .store
            .load(&fixture.identity)
            .await
            .unwrap()
            .expected,
        ""
    );
    assert_ne!(fixture.store.load(&peer).await.unwrap().expected, "");
    assert_eq!(fixture.search().await.semantic.len(), 4);
    fixture.cleanup().await;
}

async fn install_legacy_catalogue(fixture: &Fixture) {
    let mut legacy = fixture
        .store
        .active_configuration(&fixture.identity)
        .await
        .unwrap();
    let node = legacy
        .nodes
        .as_mut()
        .unwrap()
        .remove(&fixture.identity.node_id)
        .unwrap();
    let name = collection(&fixture.identity.group_id, true);
    fixture
        .store
        .api
        .upsert(
            &name,
            &api::Upsert {
                points: vec![api::Point {
                    id: point_id(&["head", &node.node_id]),
                    vector: api::Vectors::default(),
                    payload: node,
                }],
                update_filter: None,
                update_mode: None,
            },
        )
        .await
        .unwrap();
    legacy.schema_version = 1;
    legacy.publication = None;
    legacy.phase = None;
    legacy.nodes = None;
    fixture
        .store
        .api
        .upsert(
            &name,
            &api::Upsert {
                points: vec![api::Point {
                    id: point_id(&["gold-configuration"]),
                    vector: api::Vectors::default(),
                    payload: legacy,
                }],
                update_filter: None,
                update_mode: None,
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_explicit_catalogue_migration_and_interrupted_resume_preserve_silver() {
    use super::super::configuration::Phase;
    let fixture = Fixture::new(VectorIndex::default()).await;
    fixture
        .store
        .index(&fixture.identity, fixture.file(1, "legacy"))
        .await
        .unwrap();
    let before = fixture.store.load(&fixture.identity).await.unwrap();
    install_legacy_catalogue(&fixture).await;
    assert!(
        fixture
            .store
            .select_gold(&fixture.identity, false)
            .await
            .is_err()
    );
    assert!(fixture.store.load(&fixture.identity).await.is_err());
    fixture
        .store
        .select_gold(&fixture.identity, true)
        .await
        .unwrap();
    let migrated = fixture.store.load(&fixture.identity).await.unwrap();
    assert_eq!(before.expected, migrated.expected);
    assert_eq!(
        before.data.canonical_fingerprint().unwrap(),
        migrated.data.canonical_fingerprint().unwrap()
    );
    assert_eq!(fixture.search().await.semantic.len(), 2);
    // Reproduce a process exit after sealing, before adopting legacy references.
    let previous = fixture
        .store
        .active_configuration(&fixture.identity)
        .await
        .unwrap();
    let mut sealed = previous.clone();
    sealed.phase = Some(Phase::Migrating);
    sealed.nodes.as_mut().unwrap().clear();
    sealed.publication = Some(super::super::publish::staging_version().unwrap());
    fixture
        .store
        .replace_configuration(&fixture.identity, sealed, Some(&previous))
        .await
        .unwrap();
    assert!(
        fixture
            .store
            .select_gold(&fixture.identity, false)
            .await
            .is_err()
    );
    fixture
        .store
        .select_gold(&fixture.identity, true)
        .await
        .unwrap();
    let resumed = fixture.store.load(&fixture.identity).await.unwrap();
    assert_eq!(before.expected, resumed.expected);
    assert_eq!(
        before.data.canonical_fingerprint().unwrap(),
        resumed.data.canonical_fingerprint().unwrap()
    );
    assert_eq!(fixture.search().await.semantic.len(), 2);
    fixture.cleanup().await;
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
    for defect in 0..10 {
        let mut payload = original.clone();
        match defect {
            0 => payload["schema_version"] = 99.into(),
            1 => payload["version"] = "invalid".into(),
            2 => payload["kind"] = "head".into(),
            3 => payload["dimensions"] = 0.into(),
            4 => payload["generation"] = "".into(),
            5 => payload["unknown"] = true.into(),
            6 => payload["publication"] = "invalid".into(),
            7 => payload["phase"] = "unknown".into(),
            8 => payload["nodes"] = serde_json::Value::Null,
            _ => {
                payload["nodes"]["bad"] = serde_json::json!({
                    "kind": "head", "node_id": "bad", "version": "missing",
                    "head": null, "manifest": null
                });
            }
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
