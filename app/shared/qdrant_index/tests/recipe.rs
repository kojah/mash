//! Recipe, model-space and pinning contracts of shared publication.
use super::*;
use crate::gold::readiness::{Artifacts, State};

fn events(file: &tempfile::NamedTempFile) -> Vec<Event> {
    serde_json::Deserializer::from_reader(file.reopen().unwrap())
        .into_iter::<Event>()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn write(events: Vec<Event>) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    for event in events {
        crate::shared::write_event(file.as_file_mut(), &event).unwrap();
    }
    file
}

fn titled() -> crate::gold::config::Build {
    crate::gold::config::Build {
        max_embedding_chars: None,
        representations: crate::gold::config::Representations::LexicalAndDense,
        dense_content: crate::gold::config::DenseContent::AllVisible,
        retrieval_unit: crate::gold::config::RetrievalUnit::Message,
        text_preparation: crate::gold::config::TextPreparation::Preserve,
        embedding_text: crate::gold::config::EmbeddingText::TitleAndDialogue,
    }
}

fn certified_source_upload(fixture: &Fixture) -> tempfile::NamedTempFile {
    let mut events = events(&source_upload(
        fixture,
        1,
        Some(crate::gold::config::Build::default()),
        true,
        false,
        false,
    ));
    let Event::Start(start) = &mut events[0] else {
        panic!("start");
    };
    start.source_silver = Some(crate::silver::certificate::Certificate {
        revision: 1,
        interpretation_version: crate::silver::certificate::INTERPRETATION_VERSION,
        completeness: crate::silver::certificate::Completeness::Complete,
        rejected_sources: 0,
    });
    write(events)
}

fn source_upload(
    fixture: &Fixture,
    revision: i64,
    recipe: Option<crate::gold::config::Build>,
    snapshot: bool,
    include_other: bool,
    empty: bool,
) -> tempfile::NamedTempFile {
    let mut events = events(&fixture.file(revision, "recipe"));
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
    write(events)
}

struct Embedding {
    message_id: String,
    vector: Vec<f64>,
}

/// The node's same-revision snapshot, resent with vectors for `recipe`: the
/// server keeps silver and rewrites gold that is not current.
fn republish(
    mut events: Vec<Event>,
    generation: &str,
    recipe: &crate::gold::config::Build,
    vectors: Vec<Embedding>,
) -> tempfile::NamedTempFile {
    let vectors: std::collections::HashMap<_, _> = vectors
        .into_iter()
        .map(|embedding| (embedding.message_id, embedding.vector))
        .collect();
    for event in &mut events {
        match event {
            Event::Start(start) => start.gold_build = Some(recipe.clone()),
            Event::Batch(Batch::Messages { records }) => {
                for message in records {
                    message.vector = vectors.get(&message.id).cloned();
                    message.generation = message.vector.as_ref().map(|_| generation.to_owned());
                }
            }
            _ => {}
        }
    }
    write(events)
}

fn vectors(fixture: &Fixture, dimensions: usize, users_only: bool) -> Vec<Embedding> {
    fixture
        .messages("recipe")
        .into_iter()
        .filter(|m| !users_only || m.role == crate::role::Role::User)
        .map(|m| {
            let mut vector = m.vector.unwrap();
            vector.resize(dimensions, 0.0);
            Embedding {
                message_id: m.id,
                vector,
            }
        })
        .collect()
}

fn exact(fixture: &Fixture) -> protocol::Search {
    let mut exact = fixture.query();
    exact.intent = crate::gold::exact::Intent::ExactMention;
    exact.vector.clear();
    exact.generation.clear();
    exact
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_freshness_revokes_source_certificate_after_forget() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    fixture
        .server
        .index(&fixture.identity, certified_source_upload(&fixture))
        .await
        .unwrap();
    let before = fixture.server.freshness(&fixture.identity).await.unwrap();
    assert!(
        before.nodes[0]
            .source_silver
            .as_ref()
            .unwrap()
            .is_complete()
    );
    fixture
        .server
        .index(&fixture.identity, Fixture::forget(2, &["thread"]))
        .await
        .unwrap();
    let after = fixture.server.freshness(&fixture.identity).await.unwrap();
    assert!(after.nodes[0].source_silver.is_none());
    assert_ne!(
        before.nodes[0].silver_revision,
        after.nodes[0].silver_revision
    );
    assert_eq!(after.nodes[0].revision, 2);
    assert!(fixture.search().await.documents.is_empty());
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
#[allow(clippy::too_many_lines)]
async fn qdrant_dimension_switch_preserves_silver_lexical_and_requires_rebuilding_gold() {
    for memory in [MemoryTier::Cached, MemoryTier::Cold, MemoryTier::Durable] {
        let fixture = Fixture::with_memory(VectorIndex::default(), memory).await;
        fixture
            .server
            .index(&fixture.identity, certified_source_upload(&fixture))
            .await
            .unwrap();
        let silver = fixture.retained();
        let original = fixture.server.state(&fixture.identity).await.unwrap();
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
        let wide = Store::connect(
            &fixture.url,
            None,
            std::slice::from_ref(&fixture.identity.group_id),
            8,
            64 * 1024 * 1024,
            VectorIndex::default(),
            memory.into(),
        )
        .await
        .unwrap();
        let mut next = fixture.identity.clone();
        next.dimensions = 8;
        next.generation = crate::semantic::model_space("alternate-test", 8);
        assert!(wide.select_gold(&next, false).await.is_err());
        fixture.search().await;
        wide.select_gold(&next, true).await.unwrap();
        let switched = fixture.serve(wide);
        let freshness = switched.freshness(&next).await.unwrap();
        assert!(freshness.nodes[0].lexical_current);
        assert!(!freshness.nodes[0].gold_current);
        assert_eq!(freshness.nodes[0].source_silver, original.source_silver);
        assert_eq!(
            freshness.nodes[0].artifacts,
            Some(Artifacts {
                metadata: State::Ready,
                lexical: State::Ready,
                dense: State::Stale,
            })
        );
        assert!(
            fixture
                .server
                .search(&fixture.identity, fixture.query())
                .await
                .is_err()
        );
        assert!(
            fixture
                .server
                .index(&fixture.identity, certified_source_upload(&fixture))
                .await
                .is_err(),
            "a process with the replaced settings fails closed"
        );
        assert_eq!(
            switched
                .search(&next, exact(&fixture))
                .await
                .unwrap()
                .lexical
                .len(),
            2
        );
        let mut wide_query = fixture.query();
        wide_query.generation.clone_from(&next.generation);
        wide_query.vector.resize(8, 0.0);
        assert!(switched.search(&next, wide_query.clone()).await.is_err());
        switched
            .index(
                &next,
                republish(
                    events(&certified_source_upload(&fixture)),
                    &next.generation,
                    &crate::gold::config::Build::default(),
                    vectors(&fixture, 8, false),
                ),
            )
            .await
            .unwrap();
        assert_eq!(
            switched
                .search(&next, wide_query)
                .await
                .unwrap()
                .semantic
                .len(),
            2
        );
        let after = switched.state(&next).await.unwrap();
        assert_eq!(after.revision, original.revision);
        assert_eq!(after.silver_revision, original.silver_revision);
        assert_eq!(fixture.retained(), silver);
        // Switching back needs another explicit gold rebuild at the original width.
        fixture
            .store
            .select_gold(&fixture.identity, true)
            .await
            .unwrap();
        assert!(
            fixture
                .server
                .search(&fixture.identity, fixture.query())
                .await
                .is_err()
        );
        fixture
            .server
            .index(
                &fixture.identity,
                republish(
                    events(&certified_source_upload(&fixture)),
                    &fixture.identity.generation,
                    &crate::gold::config::Build::default(),
                    vectors(&fixture, 4, false),
                ),
            )
            .await
            .unwrap();
        assert_eq!(fixture.search().await.semantic.len(), 2);
        assert_eq!(fixture.retained(), silver);
        fixture.cleanup().await;
    }
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
#[allow(clippy::too_many_lines)]
async fn qdrant_artifact_switch_replaces_vectors_without_changing_silver() {
    use crate::gold::config::{Build, Representations};
    let fixture = Fixture::new(VectorIndex::default()).await;
    let mut initial = events(&certified_source_upload(&fixture));
    for event in &mut initial {
        if let Event::Batch(Batch::Messages { records }) = event {
            records[1].role = crate::role::Role::Assistant;
        }
    }
    let base = write(initial);
    fixture
        .server
        .index(&fixture.identity, write(events(&base)))
        .await
        .unwrap();
    let silver = fixture.retained();
    let original = fixture.server.state(&fixture.identity).await.unwrap();
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
        let gold = fixture.store.clone().with_build(recipe.clone());
        gold.select_gold(&fixture.identity, true).await.unwrap();
        let server = fixture.serve(gold);
        if recipe.users_only() {
            let rejected = server
                .index(
                    &fixture.identity,
                    republish(
                        events(&base),
                        &fixture.identity.generation,
                        &recipe,
                        vectors(&fixture, 4, false),
                    ),
                )
                .await;
            assert!(rejected.is_err(), "excluded vectors are rejected");
            assert_eq!(fixture.retained(), silver);
        }
        let supplied = if recipe.dense_enabled() {
            // The upload above made the second message an assistant reply.
            let mut supplied = vectors(&fixture, 4, false);
            if recipe.users_only() {
                supplied.remove(1);
            }
            supplied
        } else {
            Vec::new()
        };
        let indexed = server
            .index(
                &fixture.identity,
                republish(
                    events(&base),
                    &fixture.identity.generation,
                    &recipe,
                    supplied,
                ),
            )
            .await
            .unwrap();
        assert_eq!(fixture.retained(), silver);
        if recipe.users_only() {
            assert_eq!(indexed.counts.embeddings, 1);
            assert_eq!(indexed.counts.searchable_user_messages, Some(1));
        }
        assert_eq!(
            server
                .freshness(&fixture.identity)
                .await
                .unwrap()
                .group_gold
                .unwrap()
                .build,
            recipe
        );
        let freshness = server.freshness(&fixture.identity).await.unwrap();
        assert!(freshness.nodes[0].gold_current);
        assert_eq!(freshness.nodes[0].source_silver, original.source_silver);
        assert_eq!(
            freshness.nodes[0].artifacts,
            Some(Artifacts {
                metadata: State::Ready,
                lexical: State::Ready,
                dense: if recipe.dense_enabled() {
                    State::Ready
                } else {
                    State::Disabled
                },
            })
        );
        assert!(
            !server
                .search(&fixture.identity, exact(&fixture))
                .await
                .unwrap()
                .lexical
                .is_empty()
        );
        let searched = server.search(&fixture.identity, fixture.query()).await;
        assert_eq!(
            searched.is_ok(),
            recipe.dense_enabled(),
            "recipe {recipe:?}: {searched:?}"
        );
    }
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_source_recipe_pinning_rejects_mismatches_and_mixed_retained_vectors() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    fixture
        .server
        .index(
            &fixture.identity,
            source_upload(&fixture, 1, None, true, true, false),
        )
        .await
        .unwrap();
    let title = titled();
    let gold = fixture.store.clone().with_build(title.clone());
    gold.select_gold(&fixture.identity, true).await.unwrap();
    let server = fixture.serve(gold);
    assert_eq!(
        server.state(&fixture.identity).await.unwrap().gold_build,
        title
    );
    let before = server.state(&fixture.identity).await.unwrap();
    let error = server
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
    assert_eq!(server.state(&fixture.identity).await.unwrap(), before);
    // Only one of two retained threads was replaced: no mixed-recipe certificate.
    server
        .index(
            &fixture.identity,
            source_upload(&fixture, 2, Some(title.clone()), false, false, false),
        )
        .await
        .unwrap();
    assert!(!server.freshness(&fixture.identity).await.unwrap().nodes[0].gold_current);
    for policy in [None, Some(VectorIndex::Hnsw { ef_runtime: 80 })] {
        let mut query = fixture.query();
        query.vector_policy = policy;
        assert!(server.search(&fixture.identity, query).await.is_err());
    }
    // A complete replacement recovers readiness; a later no-op preserves it.
    server
        .index(
            &fixture.identity,
            source_upload(&fixture, 3, Some(title.clone()), true, true, false),
        )
        .await
        .unwrap();
    server
        .search(&fixture.identity, fixture.query())
        .await
        .unwrap();
    let canonical = server
        .state(&fixture.identity)
        .await
        .unwrap()
        .silver_revision;
    server
        .index(
            &fixture.identity,
            source_upload(&fixture, 4, Some(title.clone()), false, false, true),
        )
        .await
        .unwrap();
    let freshness = server.freshness(&fixture.identity).await.unwrap();
    assert_eq!(freshness.nodes[0].silver_revision, canonical);
    assert!(freshness.nodes[0].gold_current);
    server
        .search(&fixture.identity, fixture.query())
        .await
        .unwrap();
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_empty_title_recipe_bootstrap_and_duplicate_refresh_stay_current() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    let title = titled();
    let gold = fixture.store.clone().with_build(title.clone());
    gold.select_gold(&fixture.identity, true).await.unwrap();
    let server = fixture.serve(gold);
    for _ in 0..2 {
        server
            .index(
                &fixture.identity,
                source_upload(&fixture, 1, Some(title.clone()), true, false, true),
            )
            .await
            .unwrap();
        let freshness = server.freshness(&fixture.identity).await.unwrap();
        assert!(freshness.nodes[0].gold_current);
        assert_eq!(
            freshness.nodes[0].artifacts,
            Some(Artifacts {
                metadata: State::Ready,
                lexical: State::Ready,
                dense: State::Ready
            })
        );
        let result = server
            .search(&fixture.identity, fixture.query())
            .await
            .unwrap();
        assert!(result.lexical.is_empty() && result.semantic.is_empty());
    }
    fixture.cleanup().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_source_and_kind_indexes_are_added_to_existing_collections() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    let path = format!(
        "{}/collections/{}",
        fixture.url,
        collection(&fixture.identity.group_id, false)
    );
    let indexed = || async {
        let info: serde_json::Value = reqwest::get(&path).await.unwrap().json().await.unwrap();
        ["source_id", "object.kind"].map(|field| {
            info["result"]["payload_schema"][field]["data_type"]
                .as_str()
                .map(str::to_owned)
        })
    };
    let keyword = || Some("keyword".to_owned());
    assert_eq!(indexed().await, [keyword(), keyword()]);
    // A collection from an earlier release has neither; opening it adds them.
    for field in ["source_id", "object.kind"] {
        reqwest::Client::new()
            .delete(format!("{path}/index/{field}?wait=true"))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
    }
    assert_eq!(indexed().await, [None, None]);
    fixture
        .reopen(VectorIndex::default(), MemoryTier::Cached)
        .await
        .unwrap();
    assert_eq!(indexed().await, [keyword(), keyword()]);
    fixture.cleanup().await;
}
