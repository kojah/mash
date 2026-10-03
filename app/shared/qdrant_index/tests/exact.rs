use super::*;

fn exact_query(fixture: &Fixture, text: &str) -> protocol::Search {
    let mut query = fixture.query();
    query.intent = crate::gold::exact::Intent::ExactMention;
    query.query = text.into();
    query.vector.clear();
    query.generation.clear();
    query
}

async fn head(fixture: &Fixture, head: NodeHead) {
    fixture
        .store
        .api
        .upsert(
            &collection(&fixture.identity.group_id, true),
            &api::Upsert {
                points: vec![api::Point {
                    id: point_id(&["head", &fixture.identity.node_id]),
                    vector: api::Vectors::default(),
                    payload: head,
                }],
                update_filter: None,
                update_mode: None,
            },
        )
        .await
        .unwrap();
}

async fn assert_recipe_independent_lexical(fixture: &Fixture) {
    let other_recipe = fixture
        .store
        .clone()
        .with_build(crate::gold::config::Build {
            embedding_text: crate::gold::config::EmbeddingText::TitleAndDialogue,
        });
    other_recipe
        .select_gold(&fixture.identity, true)
        .await
        .unwrap();
    assert_eq!(
        other_recipe
            .search(&fixture.identity, exact_query(fixture, "retained"))
            .await
            .unwrap()
            .lexical
            .len(),
        2
    );
    fixture
        .store
        .select_gold(&fixture.identity, true)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_exact_readiness_rejects_stale_artifacts_and_unpublished_staging() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    fixture
        .store
        .index(&fixture.identity, fixture.file(1, "retained"))
        .await
        .unwrap();
    let published = fixture.store.node(&fixture.identity).await.unwrap();
    let mut staging = fixture.store.load(&fixture.identity).await.unwrap().data;
    for message in staging.messages.values_mut() {
        message.projection = "needle unpublished".into();
    }
    let encoded =
        crate::shared::snapshot::Encoded::new(&staging.layers(), 64 * 1024 * 1024).unwrap();
    fixture
        .store
        .stage(&fixture.identity, &staging, &encoded, "unpublished-version")
        .await
        .unwrap();
    assert!(
        fixture
            .store
            .search(&fixture.identity, exact_query(&fixture, "unpublished"))
            .await
            .unwrap()
            .lexical
            .is_empty()
    );
    // Recipe changes affect dense retrieval, not a current lexical artifact.
    assert_recipe_independent_lexical(&fixture).await;
    for corrupt in 0..4 {
        let mut changed = published.clone();
        let evidence = changed.head.as_mut().unwrap();
        match corrupt {
            0 => evidence.lexical_manifest = None,
            1 => evidence.metadata_ready = false,
            2 => evidence.catalog_ready = false,
            _ => {
                let silver = crate::shared::silver_revision::State::next(
                    evidence.silver_revision.as_ref(),
                    "a".repeat(64),
                )
                .unwrap();
                evidence.lexical_manifest =
                    Some(crate::shared::lexical_manifest::Manifest::new(silver));
            }
        }
        head(&fixture, changed).await;
        let error = fixture
            .store
            .search(&fixture.identity, exact_query(&fixture, "retained"))
            .await
            .err()
            .unwrap();
        assert!(error.error.message.contains("lexical artifact"));
    }
    head(&fixture, published.clone()).await;
    fixture
        .store
        .index(&fixture.identity, fixture.file(2, "changed"))
        .await
        .unwrap();
    let result = fixture
        .store
        .search(&fixture.identity, exact_query(&fixture, "changed"))
        .await
        .unwrap();
    assert_eq!(result.lexical.len(), 2);
    let current = fixture.store.node(&fixture.identity).await.unwrap();
    let evidence = current.head.as_ref().unwrap();
    assert!(
        evidence
            .lexical_manifest
            .as_ref()
            .unwrap()
            .matches(evidence.silver_revision.as_ref())
    );
    assert!(
        fixture
            .store
            .search(&fixture.identity, exact_query(&fixture, "retained"))
            .await
            .unwrap()
            .lexical
            .is_empty()
    );
    fixture.cleanup().await;
}
