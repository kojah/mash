use super::*;
use crate::views::{documents::EmbeddingInput, materialize::Writer};
use std::path::Path;

fn model() -> crate::semantic::remote::Configuration {
    crate::semantic::remote::Configuration {
        url: recovery::embeddings::serve(),
        model: "test".into(),
        dimensions: 4,
        api: crate::semantic::remote::Api::OpenaiCompatible,
        token_env: None,
        request_dimensions: true,
        redaction: crate::redaction::Policy::ON,
    }
}

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_materializer_reuses_session_vectors_after_metadata_edits_without_a_model() {
    let fixture = Fixture::new(VectorIndex::default()).await;
    let model = model();
    let mut chat = Fixture::conversation();
    let mut messages = fixture.messages("native");
    let mut writer = Writer::new(
        &fixture.store,
        &fixture.identity,
        Some((Path::new(""), Some(&model))),
    );
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
    let mut chat = Fixture::conversation();
    let mut messages = fixture.messages("native");
    for step in 0..4 {
        if step == 1 {
            chat.title = Some("new title".into());
        }
        if step == 2 {
            messages[0].projection = "new parent text".into();
        }
        let mut writer = Writer::new(
            &store,
            &fixture.identity,
            Some((Path::new(""), Some(&model))),
        );
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
    let mut writer = Writer::new(
        &fixture.store,
        &fixture.identity,
        Some((Path::new(""), Some(&model))),
    );
    writer
        .write_chat(&Fixture::conversation(), &fixture.messages("native"))
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
    let mut writer = Writer::new(
        &fixture.store,
        &fixture.identity,
        Some((Path::new(""), Some(&model))),
    );
    writer
        .write_chat(&Fixture::conversation(), &fixture.messages("native"))
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
        let mut writer = Writer::new(
            &fixture.store,
            &fixture.identity,
            Some((Path::new(""), Some(&model))),
        );
        writer
            .write_chat(&Fixture::conversation(), &fixture.messages("native"))
            .await
            .unwrap();
        let result = writer.finish().await.unwrap();
        assert_eq!(result.embeddings.model_inferences, 1, "{field}");
        assert_eq!(result.embeddings.reused_vectors, 1, "{field}");
    }
    fixture.cleanup().await;
}
