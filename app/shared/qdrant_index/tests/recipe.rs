use super::*;

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_dimension_switch_preserves_silver_lexical_and_restart_recovery() {
    for memory in [MemoryTier::Cached, MemoryTier::Cold] {
        dimension_switch(memory).await;
    }
}

async fn dimension_switch(memory: MemoryTier) {
    let mut fixture = Fixture::with_memory(VectorIndex::default(), memory).await;
    fixture
        .store
        .index(&fixture.identity, certified_source_upload(&fixture))
        .await
        .unwrap();
    let original = fixture.store.load(&fixture.identity).await.unwrap();
    let old_store = fixture.store.clone();
    let old_identity = fixture.identity.clone();
    let old_query = fixture.query();
    if memory == MemoryTier::Cold {
        // Simulate termination between named-field creation and placement setup.
        reqwest::Client::new()
            .put(format!(
                "{}/collections/{}/vectors/dense_8?wait=true",
                fixture.url,
                collection(&fixture.identity.group_id, false)
            ))
            .json(&serde_json::json!({"dense":{"size":8,"distance":"Cosine"}}))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
    }
    let connect = || {
        Store::connect(
            &fixture.url,
            None,
            std::slice::from_ref(&fixture.identity.group_id),
            8,
            64 * 1024 * 1024,
            VectorIndex::default(),
            memory,
        )
    };
    let prepared = connect().await.unwrap();
    let mut next = fixture.identity.clone();
    next.dimensions = 8;
    next.generation = crate::semantic::model_space("alternate-test", 8);
    assert!(prepared.select_gold(&next, false).await.is_err());
    assert!(
        !old_store
            .search(&old_identity, fixture.query())
            .await
            .unwrap()
            .semantic
            .is_empty()
    );
    // Restart after preparation. It must reuse the field without activating it.
    fixture.store = connect().await.unwrap();
    fixture.identity = next;
    fixture
        .store
        .select_gold(&fixture.identity, true)
        .await
        .unwrap();
    let freshness = fixture.store.freshness(&fixture.identity).await.unwrap();
    assert!(freshness.nodes[0].lexical_current);
    assert!(!freshness.nodes[0].gold_current);
    assert!(old_store.search(&old_identity, old_query).await.is_err());
    assert!(
        old_store
            .index(&old_identity, certified_source_upload(&fixture))
            .await
            .is_err()
    );
    let mut exact = fixture.query();
    exact.intent = crate::gold::exact::Intent::ExactMention;
    exact.vector.clear();
    exact.generation.clear();
    assert_eq!(
        fixture
            .store
            .search(&fixture.identity, exact)
            .await
            .unwrap()
            .lexical
            .len(),
        2
    );
    assert!(
        fixture
            .store
            .search(&fixture.identity, fixture.query())
            .await
            .is_err()
    );
    dimension_roundtrip(&mut fixture, old_store, old_identity, &original).await;
    fixture.cleanup().await;
}

async fn dimension_roundtrip(
    fixture: &mut Fixture,
    old_store: Store,
    old_identity: Identity,
    original: &Loaded,
) {
    use crate::shared::gold_rebuild::Embedding;
    let silver = serde_json::to_vec(&original.data.silver()).unwrap();
    let fingerprint = original.data.canonical_fingerprint().unwrap();
    for dimensions in [8, 4] {
        if dimensions == 4 {
            fixture.store = old_store.clone();
            fixture.identity = old_identity.clone();
        }
        let vectors = original
            .data
            .messages
            .values()
            .map(|m| {
                let mut vector = m.vector.clone().unwrap();
                vector.resize(dimensions, 0.0);
                Embedding {
                    message_id: m.id.clone(),
                    vector,
                }
            })
            .collect::<Vec<_>>();
        assert_artifact_switch(
            fixture,
            original,
            &vectors,
            &fingerprint,
            crate::gold::config::Build::default(),
        )
        .await;
        let after = fixture.store.load(&fixture.identity).await.unwrap();
        assert_eq!(serde_json::to_vec(&after.data.silver()).unwrap(), silver);
        let before = original.head.as_ref().unwrap();
        let after = after.head.unwrap();
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.instance, before.instance);
        assert_eq!(after.silver_revision, before.silver_revision);
        assert_eq!(after.source_silver, before.source_silver);
        assert_eq!(after.last_synced_at, before.last_synced_at);
        assert_eq!(after.last_updated_at, before.last_updated_at);
    }
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_model_generation_switch_preserves_silver_and_requires_rebuilding_gold() {
    use crate::shared::gold_rebuild::Embedding;
    let mut fixture = Fixture::new(VectorIndex::default()).await;
    fixture
        .store
        .index(&fixture.identity, certified_source_upload(&fixture))
        .await
        .unwrap();
    let original = fixture.store.load(&fixture.identity).await.unwrap();
    let silver = serde_json::to_vec(&original.data.silver()).unwrap();
    let fingerprint = original.data.canonical_fingerprint().unwrap();
    let original_head = original.head.as_ref().unwrap();
    let old_identity = fixture.identity.clone();
    let old_query = fixture.query();
    let old_version = fixture.store.gold_version(&old_identity).await.unwrap();
    // Frozen vectors isolate publication/model identity from inference quality.
    // This contract changes model generation while keeping dimensionality fixed.
    let vectors = original
        .data
        .messages
        .values()
        .map(|m| Embedding {
            message_id: m.id.clone(),
            vector: m.vector.clone().unwrap(),
        })
        .collect::<Vec<_>>();
    fixture.identity.generation = crate::semantic::model_space("alternate-test", 4);
    assert!(
        fixture
            .store
            .select_gold(&fixture.identity, false)
            .await
            .is_err()
    );
    assert_eq!(
        fixture.store.gold_version(&old_identity).await.unwrap(),
        old_version
    );
    fixture
        .store
        .select_gold(&fixture.identity, true)
        .await
        .unwrap();
    let stale = fixture.store.freshness(&fixture.identity).await.unwrap();
    assert!(stale.nodes[0].lexical_current);
    assert!(!stale.nodes[0].gold_current);
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
            .search(&old_identity, old_query)
            .await
            .is_err()
    );
    let mut exact = fixture.query();
    exact.intent = crate::gold::exact::Intent::ExactMention;
    exact.vector.clear();
    exact.generation.clear();
    assert_eq!(
        fixture
            .store
            .search(&fixture.identity, exact)
            .await
            .unwrap()
            .lexical
            .len(),
        2
    );

    for identity in [fixture.identity.clone(), old_identity] {
        fixture.identity = identity;
        assert_artifact_switch(
            &fixture,
            &original,
            &vectors,
            &fingerprint,
            crate::gold::config::Build::default(),
        )
        .await;
        let after = fixture.store.load(&fixture.identity).await.unwrap();
        assert_eq!(serde_json::to_vec(&after.data.silver()).unwrap(), silver);
        let head = after.head.unwrap();
        assert_eq!(head.revision, original_head.revision);
        assert_eq!(head.instance, original_head.instance);
        assert_eq!(head.silver_revision, original_head.silver_revision);
        assert_eq!(head.source_silver, original_head.source_silver);
        assert_eq!(head.last_synced_at, original_head.last_synced_at);
        assert_eq!(head.last_updated_at, original_head.last_updated_at);
        assert!(
            after
                .data
                .messages
                .values()
                .all(|m| m.generation.as_deref() == Some(fixture.identity.generation.as_str()))
        );
    }
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_artifact_switch_replaces_vectors_without_changing_silver() {
    use crate::gold::config::{Build, Representations};
    use crate::shared::gold_rebuild::Embedding;
    let fixture = Fixture::new(VectorIndex::default()).await;
    let source = certified_source_upload(&fixture);
    let mut events = serde_json::Deserializer::from_reader(source.reopen().unwrap())
        .into_iter::<Event>()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    for event in &mut events {
        if let Event::Batch(Batch::Messages { records }) = event {
            records[1].role = crate::role::Role::Assistant;
        }
    }
    let mut source = tempfile::NamedTempFile::new().unwrap();
    for event in events {
        crate::shared::write_event(source.as_file_mut(), &event).unwrap();
    }
    fixture
        .store
        .index(&fixture.identity, source)
        .await
        .unwrap();
    let original = fixture.store.load(&fixture.identity).await.unwrap();
    let fingerprint = original.data.canonical_fingerprint().unwrap();
    let vectors = original
        .data
        .messages
        .values()
        .filter(|m| !m.projection.is_empty())
        .map(|m| Embedding {
            message_id: m.id.clone(),
            vector: m.vector.clone().unwrap(),
        })
        .collect::<Vec<_>>();
    for recipe in [
        Build {
            representations: Representations::LexicalOnly,
            ..Default::default()
        },
        Build::default(),
        Build {
            dense_content: crate::gold::config::DenseContent::AllVisible,
            text_preparation: crate::gold::config::TextPreparation::CollapseWhitespace,
            ..Default::default()
        },
        Build {
            dense_content: crate::gold::config::DenseContent::UserMessages,
            ..Default::default()
        },
        Build {
            retrieval_unit: crate::gold::config::RetrievalUnit::ParentAndMessage,
            ..Default::default()
        },
        Build::default(),
    ] {
        assert_artifact_switch(&fixture, &original, &vectors, &fingerprint, recipe).await;
    }
    fixture.cleanup().await;
}

async fn assert_artifact_switch(
    fixture: &Fixture,
    original: &Loaded,
    vectors: &[crate::shared::gold_rebuild::Embedding],
    fingerprint: &str,
    recipe: crate::gold::config::Build,
) {
    use crate::shared::gold_rebuild::{Embedding, Event as GoldEvent};
    let store = fixture.store.clone().with_build(recipe.clone());
    store.select_gold(&fixture.identity, true).await.unwrap();
    let loaded = store.load(&fixture.identity).await.unwrap();
    let head = loaded.head.as_ref().unwrap();
    let mut file = tempfile::NamedTempFile::new().unwrap();
    serde_json::to_writer(
        file.as_file_mut(),
        &GoldEvent::Start {
            publication_revision: head.revision,
            silver: head.silver_revision.clone().unwrap(),
            generation: fixture.identity.generation.clone(),
            build: recipe.clone(),
        },
    )
    .unwrap();
    writeln!(file).unwrap();
    if recipe.users_only() {
        assert_excluded_vectors_rejected(&store, fixture, head, &file, vectors, fingerprint).await;
    }
    if recipe.dense_enabled() {
        let records = vectors
            .iter()
            .filter(|v| {
                let message = &original.data.messages[&v.message_id];
                recipe.includes_dense(&message.role, &message.projection)
            })
            .map(|v| Embedding {
                message_id: v.message_id.clone(),
                vector: v.vector.clone(),
            })
            .collect();
        serde_json::to_writer(file.as_file_mut(), &GoldEvent::Embeddings { records }).unwrap();
        writeln!(file).unwrap();
    }
    serde_json::to_writer(file.as_file_mut(), &GoldEvent::Finish).unwrap();
    writeln!(file).unwrap();
    let indexed = store.rebuild_gold(&fixture.identity, file).await.unwrap();
    assert_eq!(
        indexed.counts.embeddings,
        indexed.counts.expected_dense(&recipe).unwrap()
    );
    let loaded = store.load(&fixture.identity).await.unwrap();
    assert_eq!(loaded.data.canonical_fingerprint().unwrap(), fingerprint);
    if recipe.users_only() {
        assert_eq!(indexed.counts.embeddings, 1);
        assert_eq!(indexed.counts.searchable_user_messages, Some(1));
        assert!(loaded.data.messages["message-1"].vector.is_none());
        assert!(loaded.data.messages["message-0"].vector.is_some());
        assert!(
            store
                .state(&fixture.identity)
                .await
                .unwrap()
                .sources
                .iter()
                .all(|s| s.ready)
        );
    }
    assert_eq!(store.capabilities(&fixture.identity).await.unwrap(), recipe);
    assert!(store.freshness(&fixture.identity).await.unwrap().nodes[0].gold_current);
    let mut exact = fixture.query();
    exact.intent = crate::gold::exact::Intent::ExactMention;
    exact.vector.clear();
    assert!(
        !store
            .search(&fixture.identity, exact)
            .await
            .unwrap()
            .lexical
            .is_empty()
    );
    let searched = store.search(&fixture.identity, fixture.query()).await;
    assert_eq!(
        searched.is_ok(),
        recipe.dense_enabled(),
        "recipe {recipe:?}: {searched:?}"
    );
}

async fn assert_excluded_vectors_rejected(
    store: &Store,
    fixture: &Fixture,
    head: &Head,
    file: &tempfile::NamedTempFile,
    vectors: &[crate::shared::gold_rebuild::Embedding],
    fingerprint: &str,
) {
    use crate::shared::gold_rebuild::{Embedding, Event as GoldEvent};
    let mut rejected = tempfile::NamedTempFile::new().unwrap();
    std::io::copy(&mut file.reopen().unwrap(), rejected.as_file_mut()).unwrap();
    serde_json::to_writer(
        rejected.as_file_mut(),
        &GoldEvent::Embeddings {
            records: vectors
                .iter()
                .map(|v| Embedding {
                    message_id: v.message_id.clone(),
                    vector: v.vector.clone(),
                })
                .collect(),
        },
    )
    .unwrap();
    writeln!(rejected).unwrap();
    serde_json::to_writer(rejected.as_file_mut(), &GoldEvent::Finish).unwrap();
    writeln!(rejected).unwrap();
    assert!(
        store
            .rebuild_gold(&fixture.identity, rejected)
            .await
            .is_err()
    );
    let unchanged = store.load(&fixture.identity).await.unwrap();
    assert_eq!(unchanged.head.as_ref().unwrap().revision, head.revision);
    assert_eq!(unchanged.data.canonical_fingerprint().unwrap(), fingerprint);
}

fn certified_source_upload(fixture: &Fixture) -> tempfile::NamedTempFile {
    let source = source_upload(
        fixture,
        1,
        Some(crate::gold::config::Build::default()),
        true,
        false,
        false,
    );
    let mut events = serde_json::Deserializer::from_reader(source.reopen().unwrap())
        .into_iter::<Event>()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let Event::Start(start) = &mut events[0] else {
        panic!("start");
    };
    start.source_silver = Some(crate::silver::certificate::Certificate {
        revision: 1,
        interpretation_version: crate::silver::certificate::INTERPRETATION_VERSION,
        completeness: crate::silver::certificate::Completeness::Complete,
        rejected_sources: 0,
    });
    let mut source = tempfile::NamedTempFile::new().unwrap();
    for event in events {
        crate::shared::write_event(source.as_file_mut(), &event).unwrap();
    }
    source
}

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

fn reuse_upload(
    fixture: &Fixture,
    target: &crate::shared::reuse::Target,
    defect: Option<usize>,
) -> tempfile::NamedTempFile {
    let source = source_upload(
        fixture,
        target.publication_revision + 1,
        Some(target.build.clone()),
        true,
        false,
        false,
    );
    let mut events = serde_json::Deserializer::from_reader(source.reopen().unwrap())
        .into_iter::<Event>()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let Event::Start(start) = &mut events[0] else {
        panic!("start")
    };
    start.expected_version = Some(target.publication_version.clone());
    start.reuse_gold = true;
    for event in &mut events {
        if let Event::Batch(Batch::Messages { records }) = event {
            for message in records {
                message.vector = None;
                message.generation = None;
            }
        }
    }
    let reuse = Event::ReuseGold {
        conversation_id: "thread".into(),
        fingerprint: target.threads["thread"].clone(),
    };
    events.insert(events.len() - 1, reuse);
    match defect {
        Some(0) => {
            if let Event::Start(start) = &mut events[0] {
                start.expected_version = None;
            }
        }
        Some(1) => {
            if let Event::Start(start) = &mut events[0] {
                start.reuse_gold = false;
            }
        }
        Some(2) => {
            if let Event::Start(start) = &mut events[0] {
                start.expected_version = Some("stale".into());
            }
        }
        Some(3) => {
            if let Event::Batch(Batch::Conversations { records }) = &mut events[1] {
                records[0].title = Some("new title".into());
            }
        }
        Some(4) => {
            if let Event::Batch(Batch::Messages { records }) = &mut events[2] {
                records[0].content.push('!');
            }
        }
        Some(5) => {
            if let Event::Batch(Batch::Messages { records }) = &mut events[2] {
                records[0].vector = Some(vec![127.0, 0.0, 0.0, 0.0]);
            }
        }
        Some(6) => {
            events.insert(
                events.len() - 1,
                Event::ReuseGold {
                    conversation_id: "thread".into(),
                    fingerprint: target.threads["thread"].clone(),
                },
            );
        }
        Some(7) => {
            events.pop();
        }
        Some(8) => {
            if let Event::ReuseGold { fingerprint, .. } = &mut events[3] {
                *fingerprint = "f".repeat(64);
            }
        }
        Some(9) => {
            if let Event::ReuseGold {
                conversation_id, ..
            } = &mut events[3]
            {
                *conversation_id = "missing".into();
            }
        }
        _ => {}
    }
    let mut file = tempfile::NamedTempFile::new().unwrap();
    for event in events {
        crate::shared::write_event(file.as_file_mut(), &event).unwrap();
    }
    file
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_snapshot_reuse_validates_dependencies_pins_and_complete_streams() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    let build = crate::gold::config::Build {
        max_embedding_chars: None,
        representations: crate::gold::config::Representations::LexicalAndDense,
        dense_content: crate::gold::config::DenseContent::AllVisible,
        retrieval_unit: crate::gold::config::RetrievalUnit::Message,
        text_preparation: crate::gold::config::TextPreparation::Preserve,
        embedding_text: crate::gold::config::EmbeddingText::TitleAndDialogue,
    };
    let store = fixture.store.clone().with_build(build.clone());
    store.select_gold(&fixture.identity, true).await.unwrap();
    store
        .index(
            &fixture.identity,
            source_upload(&fixture, 1, Some(build), true, false, false),
        )
        .await
        .unwrap();
    let loaded = store.load(&fixture.identity).await.unwrap();
    let before = store.state(&fixture.identity).await.unwrap();
    let target =
        crate::shared::reuse::Target::new(&loaded, &fixture.identity, store.build.clone()).unwrap();
    for defect in 0..10 {
        assert!(
            store
                .index(
                    &fixture.identity,
                    reuse_upload(&fixture, &target, Some(defect))
                )
                .await
                .is_err(),
            "defect {defect}"
        );
        assert_eq!(store.state(&fixture.identity).await.unwrap(), before);
    }
    store
        .index(&fixture.identity, reuse_upload(&fixture, &target, None))
        .await
        .unwrap();
    let after = store.load(&fixture.identity).await.unwrap();
    assert_eq!(
        after.data.canonical_fingerprint().unwrap(),
        loaded.data.canonical_fingerprint().unwrap()
    );
    assert_eq!(
        after.head.as_ref().unwrap().silver_revision,
        loaded.head.as_ref().unwrap().silver_revision
    );
    for (id, message) in loaded.data.messages {
        assert_eq!(after.data.messages[&id].vector, message.vector);
    }
    store
        .search(&fixture.identity, fixture.query())
        .await
        .unwrap();
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_pinned_recipe_refresh_at_same_source_revision_requires_unchanged_silver() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    fixture
        .store
        .index(
            &fixture.identity,
            source_upload(&fixture, 1, None, true, false, false),
        )
        .await
        .unwrap();
    let before = fixture.store.state(&fixture.identity).await.unwrap();
    let build = crate::gold::config::Build {
        max_embedding_chars: Some(256_u32.try_into().unwrap()),
        representations: crate::gold::config::Representations::LexicalAndDense,
        dense_content: crate::gold::config::DenseContent::AllVisible,
        retrieval_unit: crate::gold::config::RetrievalUnit::Message,
        text_preparation: crate::gold::config::TextPreparation::Preserve,
        embedding_text: crate::gold::config::EmbeddingText::TitleAndDialogue,
    };
    let store = fixture.store.clone().with_build(build.clone());
    store.select_gold(&fixture.identity, true).await.unwrap();
    for changed in [true, false] {
        let file = source_upload(&fixture, 1, Some(build.clone()), true, false, false);
        let mut events = serde_json::Deserializer::from_reader(file.reopen().unwrap())
            .into_iter::<Event>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let Event::Start(start) = &mut events[0] else {
            panic!("start")
        };
        start.expected_version = Some(before.publication_version.clone());
        if changed && let Event::Batch(Batch::Conversations { records }) = &mut events[1] {
            records[0].title = Some("canonical change".into());
        }
        let mut outgoing = tempfile::NamedTempFile::new().unwrap();
        for event in events {
            crate::shared::write_event(outgoing.as_file_mut(), &event).unwrap();
        }
        let result = store.index(&fixture.identity, outgoing).await;
        if changed {
            assert!(result.is_err());
        } else {
            result.unwrap();
        }
    }
    let after = store.state(&fixture.identity).await.unwrap();
    assert_eq!(after.revision, before.revision);
    assert_eq!(after.silver_revision, before.silver_revision);
    assert_eq!(after.gold_build, build);
    store
        .search(&fixture.identity, fixture.query())
        .await
        .unwrap();
    fixture.cleanup().await;
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
        max_embedding_chars: None,
        representations: crate::gold::config::Representations::LexicalAndDense,
        dense_content: crate::gold::config::DenseContent::AllVisible,
        retrieval_unit: crate::gold::config::RetrievalUnit::Message,
        text_preparation: crate::gold::config::TextPreparation::Preserve,
        embedding_text: crate::gold::config::EmbeddingText::TitleAndDialogue,
    };
    let store = fixture.store.clone().with_build(title.clone());
    store.select_gold(&fixture.identity, true).await.unwrap();
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
    for policy in [None, Some(VectorIndex::Hnsw { ef_runtime: 80 })] {
        let mut query = fixture.query();
        query.vector_policy = policy;
        assert!(store.search(&fixture.identity, query).await.is_err());
    }
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
        max_embedding_chars: None,
        representations: crate::gold::config::Representations::LexicalAndDense,
        dense_content: crate::gold::config::DenseContent::AllVisible,
        retrieval_unit: crate::gold::config::RetrievalUnit::Message,
        text_preparation: crate::gold::config::TextPreparation::Preserve,
        embedding_text: crate::gold::config::EmbeddingText::TitleAndDialogue,
    };
    let store = fixture.store.clone().with_build(title.clone());
    store.select_gold(&fixture.identity, true).await.unwrap();
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
