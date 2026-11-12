use super::*;
use crate::views::{documents::EmbeddingInput, materialize::Writer};

fn model() -> crate::semantic::test_embeddings::Fixture {
    crate::semantic::test_embeddings::Fixture::new(4)
}

fn search_messages(fixture: &Fixture) -> Vec<crate::views::documents::SearchMessage> {
    fixture
        .messages("native")
        .into_iter()
        .map(Into::into)
        .collect()
}

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_materializer_prefetch_preserves_inference_batches_and_retry() {
    let model = model();
    let calls = &model.state.calls;
    let fixture = Fixture::new(VectorIndex::default()).await;
    let template = search_messages(&fixture).remove(0);
    let mut messages = (0..601)
        .map(|i| {
            let mut message = template.clone();
            message.id = format!("row-{i}");
            message.ordinal = i;
            message.projection = if i % 5 == 0 {
                String::new()
            } else {
                format!("visible text {i}")
            };
            message
        })
        .collect::<Vec<_>>();
    let expected = (0..601)
        .collect::<Vec<_>>()
        .chunks(32)
        .map(|rows| rows.iter().filter(|i| *i % 5 != 0).count())
        .collect::<Vec<_>>();
    let visible = expected.iter().sum::<usize>() as u64;
    for step in 0..4 {
        if step == 2 {
            messages[289].projection = "changed after prefetch boundary".into();
            let mut unavailable = Writer::new(&fixture.store, &fixture.identity, None);
            assert!(
                unavailable
                    .write_chat(&Fixture::chat(), &messages)
                    .await
                    .is_err()
            );
            assert!(calls.lock().unwrap().is_empty());
            continue;
        }
        let mut writer = Writer::new(&fixture.store, &fixture.identity, Some(model.path()));
        let built = writer
            .write_chat(&Fixture::chat(), &messages)
            .await
            .unwrap();
        assert!(built.complete);
        assert_eq!(built.messages, 601);
        assert_eq!(built.embeddings, visible);
        let result = writer.finish().await.unwrap();
        assert_eq!(
            result.embeddings.model_inferences,
            match step {
                0 => visible,
                1 => 0,
                _ => 1,
            }
        );
        let actual = std::mem::take(&mut *calls.lock().unwrap());
        assert_eq!(
            actual,
            match step {
                0 => expected.clone(),
                1 => vec![],
                _ => vec![1],
            }
        );
    }
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_empty_projection_publication_preserves_silver_and_removes_search_point() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    for (step, visible) in [false, true, false, true].into_iter().enumerate() {
        let mut messages = fixture.messages("retained");
        if !visible {
            messages[0].projection.clear();
            messages[0].vector = None;
            messages[0].generation = None;
        }
        let file = Fixture::upload(&[
            Event::Start(Fixture::start(i64::try_from(step + 1).unwrap())),
            Event::Batch(Batch::Chats {
                records: vec![Fixture::chat()],
            }),
            Event::Batch(Batch::Messages { records: messages }),
            Event::Finish,
        ]);
        fixture.server.index(&fixture.identity, file).await.unwrap();
        let stored = fixture
            .store
            .api
            .retrieve::<Document>(
                &collection(&fixture.identity.group_id, false),
                &[document_id(&fixture.identity.node_id, "message-0")],
            )
            .await
            .unwrap();
        assert_eq!(stored.len(), usize::from(visible));
        assert_eq!(
            fixture.search().await.semantic.len(),
            1 + usize::from(visible)
        );
        let connection = fixture.silver.connect(&fixture.identity).unwrap();
        let canonical = crate::shared::silver_store::chat_messages(&connection, "thread").unwrap();
        assert_eq!(canonical.len(), 2);
        assert_eq!(canonical[0].content, "needle canonical retained");
    }
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_materializer_reuses_chat_vectors_after_metadata_edits_without_a_model() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    let model = model();
    let mut chat = Fixture::chat();
    let mut messages = search_messages(&fixture);
    let mut writer = Writer::new(&fixture.store, &fixture.identity, Some(model.path()));
    writer.write_chat(&chat, &messages).await.unwrap();
    assert_eq!(
        writer.finish().await.unwrap().embeddings.model_inferences,
        2
    );

    chat.title = Some("changed title, dialogue-only recipe".into());
    chat.updated_at = Some(200);
    let mut writer = Writer::new(&fixture.store, &fixture.identity, None);
    writer.write_chat(&chat, &messages).await.unwrap();
    let result = writer.finish().await.unwrap();
    assert_eq!(result.embeddings.model_inferences, 0);
    assert_eq!(result.embeddings.reused_vectors, 2);
    let stored = fixture
        .store
        .api
        .retrieve::<Document>(
            &collection(&fixture.identity.group_id, false),
            &[document_id(&fixture.identity.node_id, "message-0")],
        )
        .await
        .unwrap();
    assert_eq!(stored.len(), 1);
    assert!(stored.iter().any(|p| p.payload.unit.title == chat.title));
    assert!(
        stored
            .iter()
            .any(|p| p.payload.unit.chat_updated_at == Some(200))
    );

    messages[0].projection = "changed embedding input".into();
    let mut writer = Writer::new(&fixture.store, &fixture.identity, None);
    assert!(writer.write_chat(&chat, &messages).await.is_err());
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_chat_window_reuses_sparse_dense_inputs_across_row_batches() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    let model = model();
    let chat = Fixture::chat();
    let template = search_messages(&fixture).remove(0);
    let rows = crate::views::shared::CACHE_ROWS;
    // A long agent chat: three searchable messages, one in each row batch,
    // among rows with nothing to search.
    let visible = [1, rows + 2, 2 * rows + 3];
    let messages = (0..2 * rows + 5)
        .map(|index| {
            let mut message = template.clone();
            message.id = format!("row-{index}");
            message.ordinal = i64::try_from(index).unwrap();
            message.projection = if visible.contains(&index) {
                format!("searchable text of row {index}")
            } else {
                String::new()
            };
            message
        })
        .collect::<Vec<_>>();
    let chats = [(chat, messages)];
    let mut writer = Writer::new(&fixture.store, &fixture.identity, Some(model.path()));
    let built = writer.write_chats(&chats).await.unwrap();
    assert!(built[0].1.complete);
    assert_eq!(built[0].1.embeddings, 3);
    assert_eq!(
        writer.finish().await.unwrap().embeddings.model_inferences,
        3
    );

    // Without a model every vector must come back from the stored points,
    // each matched to its own row.
    let mut writer = Writer::new(&fixture.store, &fixture.identity, None);
    let built = writer.write_chats(&chats).await.unwrap();
    assert!(built[0].1.complete);
    let result = writer.finish().await.unwrap();
    assert_eq!(result.embeddings.model_inferences, 0);
    assert_eq!(result.embeddings.reused_vectors, 3);
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_materializer_reuse_tracks_prepared_title_and_parent_context() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    let store = fixture
        .store
        .clone()
        .with_build(crate::views::config::Build {
            embedding_text: crate::views::config::EmbeddingText::TitleAndDialogue,
            retrieval_unit: crate::views::config::RetrievalUnit::ParentAndMessage,
            ..crate::views::config::Build::default()
        });
    store.select_views(&fixture.identity, true).await.unwrap();
    let model = model();
    let mut chat = Fixture::chat();
    let mut messages = search_messages(&fixture);
    for step in 0..4 {
        if step == 1 {
            chat.title = Some("new title".into());
        }
        if step == 2 {
            messages[0].projection = "new parent text".into();
        }
        let mut writer = Writer::new(&store, &fixture.identity, Some(model.path()));
        writer.write_chat(&chat, &messages).await.unwrap();
        let result = writer.finish().await.unwrap();
        assert_eq!(
            result.embeddings.model_inferences,
            if step == 3 { 0 } else { 2 },
            "step {step}"
        );
        assert_eq!(
            result.embeddings.reused_vectors,
            if step == 3 { 2 } else { 0 },
            "step {step}"
        );
    }
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_materializer_reuse_requires_matching_source_model_and_input() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    let model = model();
    let mut writer = Writer::new(&fixture.store, &fixture.identity, Some(model.path()));
    writer
        .write_chat(&Fixture::chat(), &search_messages(&fixture))
        .await
        .unwrap();
    writer.finish().await.unwrap();
    for change in [
        "none",
        "source",
        "text",
        "anchor",
        "generation",
        "dimensions",
        "node",
    ] {
        let mut scope = fixture.identity.clone();
        let mut input = EmbeddingInput {
            provider: crate::provider::Provider::Codex,
            anchor: "message-0",
            text: "needle native",
        };
        match change {
            "none" => {}
            "source" => {
                input.provider = crate::provider::Provider::ClaudeCode;
            }
            "text" => input.text = "different",
            "anchor" => input.anchor = "missing",
            "generation" => scope.generation = "other".into(),
            "dimensions" => scope.dimensions = 2,
            "node" => scope.node_id = "other".into(),
            _ => unreachable!(),
        }
        let vectors = fixture
            .store
            .reusable_vectors(&scope, &[input])
            .await
            .unwrap();
        assert_eq!(vectors.len(), 1);
        assert_eq!(vectors[0].is_some(), change == "none", "{change}");
    }
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_materializer_rejects_corrupt_cached_evidence_and_repairs_it() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    let model = model();
    let mut writer = Writer::new(&fixture.store, &fixture.identity, Some(model.path()));
    writer
        .write_chat(&Fixture::chat(), &search_messages(&fixture))
        .await
        .unwrap();
    writer.finish().await.unwrap();
    let name = collection(&fixture.identity.group_id, false);
    let id = document_id(&fixture.identity.node_id, "message-0");
    let original = fixture
        .store
        .api
        .retrieve::<Document>(&name, std::slice::from_ref(&id))
        .await
        .unwrap()
        .remove(0)
        .payload;
    for field in [
        "node",
        "source",
        "anchor",
        "generation",
        "fingerprint",
        "missing",
        "dimension",
        "zero",
    ] {
        let mut document = original.clone();
        match field {
            "node" => document.node_id = "foreign".into(),
            "source" => {
                document.provider = crate::provider::Provider::ClaudeCode;
            }
            "anchor" => document.unit.anchor = "foreign".into(),
            "generation" => document.unit.generation = Some("foreign".into()),
            "fingerprint" => document.embedding_fingerprint = None,
            "missing" => document.vector = None,
            "dimension" => document.vector = Some(vec![1.0, 0.0]),
            "zero" => document.vector = Some(vec![0.0; 4]),
            _ => unreachable!(),
        }
        fixture
            .store
            .api
            .upsert(
                &name,
                &api::Upsert {
                    points: vec![api::Point {
                        id: id.clone(),
                        vector: api::Vectors {
                            dense: Some((
                                fixture.store.dense_name(&fixture.identity).into(),
                                vec![1.0, 0.0, 0.0, 0.0],
                            )),
                            lexical: None,
                        },
                        payload: document,
                    }],
                },
            )
            .await
            .unwrap();
        let input = EmbeddingInput {
            provider: crate::provider::Provider::Codex,
            anchor: "message-0",
            text: "needle native",
        };
        assert!(
            fixture
                .store
                .reusable_vectors(&fixture.identity, &[input])
                .await
                .unwrap()[0]
                .is_none(),
            "{field}"
        );
        let mut writer = Writer::new(&fixture.store, &fixture.identity, Some(model.path()));
        writer
            .write_chat(&Fixture::chat(), &search_messages(&fixture))
            .await
            .unwrap();
        let result = writer.finish().await.unwrap();
        assert_eq!(result.embeddings.model_inferences, 1, "{field}");
        assert_eq!(result.embeddings.reused_vectors, 1, "{field}");
    }
    fixture.cleanup().await;
}
