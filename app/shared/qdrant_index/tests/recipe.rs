use super::*;

fn source_upload(
    fixture: &Fixture,
    revision: i64,
    recipe: Option<crate::gold::config::Build>,
    snapshot: bool,
    include_other: bool,
    empty: bool,
) -> tempfile::NamedTempFile {
    let source = fixture.file(revision, "recipe");
    let mut events = serde_json::Deserializer::from_reader(source.reopen().unwrap())
        .into_iter::<Event>()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let Event::Start(start) = &mut events[0] else {
        panic!("start")
    };
    start.gold_build = recipe;
    start.snapshot = snapshot;
    start.base_revision = (!snapshot).then_some(revision - 1);
    if empty {
        events.retain(|event| matches!(event, Event::Start(_) | Event::Finish));
    } else if include_other {
        for event in &mut events {
            match event {
                Event::Batch(Batch::Conversations { records }) => {
                    let mut other = records[0].clone();
                    other.id = "other".into();
                    records.push(other);
                }
                Event::Batch(Batch::Messages { records }) => {
                    let extra = records
                        .iter()
                        .cloned()
                        .map(|mut message| {
                            message.id = format!("other-{}", message.id);
                            message.conversation_id = "other".into();
                            message
                        })
                        .collect::<Vec<_>>();
                    records.extend(extra);
                }
                _ => {}
            }
        }
    }
    let mut file = tempfile::NamedTempFile::new().unwrap();
    for event in events {
        crate::shared::write_event(file.as_file_mut(), &event).unwrap();
    }
    file
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_source_recipe_pinning_rejects_mismatches_and_mixed_retained_vectors() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    fixture
        .store
        .index(
            &fixture.identity,
            source_upload(&fixture, 1, None, true, true, false),
        )
        .await
        .unwrap();
    let title = crate::gold::config::Build {
        embedding_text: crate::gold::config::EmbeddingText::TitleAndDialogue,
    };
    let store = fixture.store.clone().with_build(title.clone());
    assert_eq!(
        store.state(&fixture.identity).await.unwrap().gold_build,
        title
    );
    let before = store.node(&fixture.identity).await.unwrap().version;
    let error = store
        .index(
            &fixture.identity,
            source_upload(
                &fixture,
                2,
                Some(crate::gold::config::Build::default()),
                false,
                false,
                false,
            ),
        )
        .await
        .err()
        .unwrap();
    assert!(error.error.message.contains("recipe changed"));
    assert_eq!(store.node(&fixture.identity).await.unwrap().version, before);
    // Only one of two retained threads was replaced: no mixed-recipe certificate.
    store
        .index(
            &fixture.identity,
            source_upload(&fixture, 2, Some(title.clone()), false, false, false),
        )
        .await
        .unwrap();
    assert!(
        store
            .node(&fixture.identity)
            .await
            .unwrap()
            .head
            .unwrap()
            .gold_manifest
            .is_none()
    );
    assert!(
        store
            .search(&fixture.identity, fixture.query())
            .await
            .is_err()
    );
    // A complete replacement recovers readiness; a later no-op preserves it.
    store
        .index(
            &fixture.identity,
            source_upload(&fixture, 3, Some(title.clone()), true, true, false),
        )
        .await
        .unwrap();
    store
        .search(&fixture.identity, fixture.query())
        .await
        .unwrap();
    let canonical = store
        .state(&fixture.identity)
        .await
        .unwrap()
        .silver_revision;
    store
        .index(
            &fixture.identity,
            source_upload(&fixture, 4, Some(title.clone()), false, false, true),
        )
        .await
        .unwrap();
    let current = store.node(&fixture.identity).await.unwrap().head.unwrap();
    assert_eq!(current.silver_revision, canonical);
    assert!(current.gold_manifest.unwrap().matches(
        &title,
        &fixture.identity.generation,
        canonical.as_ref()
    ));
    store
        .search(&fixture.identity, fixture.query())
        .await
        .unwrap();
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_publication_version_pins_bootstrap_and_fresh_source_retries() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    let initial = fixture.store.state(&fixture.identity).await.unwrap();
    assert_eq!(initial.publication_version, "");
    let pinned = |revision: i64, version: &str| {
        let source = fixture.file(revision, "pin");
        let mut events = serde_json::Deserializer::from_reader(source.reopen().unwrap())
            .into_iter::<Event>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let Event::Start(start) = &mut events[0] else {
            panic!("start")
        };
        start.expected_version = Some(version.into());
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for event in events {
            crate::shared::write_event(file.as_file_mut(), &event).unwrap();
        }
        file
    };
    fixture
        .store
        .index(&fixture.identity, pinned(1, ""))
        .await
        .unwrap();
    let accepted = fixture.store.state(&fixture.identity).await.unwrap();
    assert_ne!(accepted.publication_version, "");
    assert_eq!(
        fixture.store.node(&fixture.identity).await.unwrap().version,
        accepted.publication_version
    );
    let error = fixture
        .store
        .index(&fixture.identity, pinned(2, ""))
        .await
        .err()
        .unwrap();
    assert_eq!(error.error.kind, crate::ErrorKind::Conflict);
    assert_eq!(
        fixture.store.state(&fixture.identity).await.unwrap(),
        accepted
    );
    fixture
        .store
        .index(&fixture.identity, pinned(2, &accepted.publication_version))
        .await
        .unwrap();
    let retried = fixture.store.state(&fixture.identity).await.unwrap();
    assert_eq!(retried.revision, 2);
    assert_ne!(retried.publication_version, accepted.publication_version);
    assert_eq!(retried.silver_revision, accepted.silver_revision);
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_empty_title_recipe_bootstrap_and_duplicate_refresh_stay_current() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    let title = crate::gold::config::Build {
        embedding_text: crate::gold::config::EmbeddingText::TitleAndDialogue,
    };
    let store = fixture.store.clone().with_build(title.clone());
    for _ in 0..2 {
        store
            .index(
                &fixture.identity,
                source_upload(&fixture, 1, Some(title.clone()), true, false, true),
            )
            .await
            .unwrap();
        let head = store.node(&fixture.identity).await.unwrap().head.unwrap();
        assert!(head.gold_manifest.unwrap().matches(
            &title,
            &fixture.identity.generation,
            head.silver_revision.as_ref()
        ));
        let result = store
            .search(&fixture.identity, fixture.query())
            .await
            .unwrap();
        assert!(result.lexical.is_empty() && result.semantic.is_empty());
    }
    fixture.cleanup().await;
}
