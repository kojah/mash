use super::*;
use crate::silver::{Conversation, NormalizedMessage, SourceIdentity};

fn seed(directory: &Path) -> std::path::PathBuf {
    let database = directory.join("silver.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    for (thread, length) in [("a", 37), ("b", 2)] {
        storage
            .replace_conversation(&Conversation {
                id: thread.into(),
                provider: crate::provider::Provider::Codex,
                title_origin: None,
                timestamp_origins: None,
                claude_lineage: None,
                source: SourceIdentity::file(format!("/nonexistent/mash-{thread}.jsonl")),
                project: None,
                repository: None,
                title: Some(format!("Topic {thread}")),
                created_at: Some(1),
                updated_at: Some(2),
                messages: (0..length)
                    .map(|ordinal| NormalizedMessage {
                        id: format!("{thread}-{}", 100 - ordinal),
                        ordinal,
                        context: crate::message_context::MessageContext::Linear,
                        role: crate::role::Role::User,
                        content: format!("canonical {thread} message {ordinal}"),
                        dialogue_text: (thread == "a" && ordinal % 4 == 0).then(String::new),
                        created_at: Some(ordinal),
                    })
                    .collect::<Vec<_>>()
                    .into(),
            })
            .unwrap();
    }
    storage.commit_writer().unwrap();
    database
}

#[tokio::test]
async fn stored_silver_reuse_needs_no_sources_or_models_and_preserves_thread_batch_boundaries() {
    let directory = tempfile::tempdir().unwrap();
    let database = seed(directory.path());
    let silver = Storage::open_catalog(&database)
        .unwrap()
        .silver_certificate()
        .unwrap();
    let mut target = Target {
        schema_version: 1,
        build: crate::gold::config::Build {
            max_embedding_chars: None,
            representations: crate::gold::config::Representations::LexicalAndDense,
            dense_content: crate::gold::config::DenseContent::AllVisible,
            text_preparation: crate::gold::config::TextPreparation::Preserve,
            embedding_text: crate::gold::config::EmbeddingText::TitleAndDialogue,
        },
        generation: "unavailable-model".into(),
        dimensions: 2,
        publication_version: "pinned-head".into(),
        instance: None,
        publication_revision: 0,
        threads: BTreeMap::new(),
    };
    target.threads = capture(&database, &target, true).unwrap().hashes;
    let (file, _, preparation) = prepare(&database, &target, None, Instant::now())
        .await
        .unwrap();
    assert_eq!(preparation.model_inferences, 0);
    assert_eq!(preparation.reused_embeddings, 29);
    assert_eq!(preparation.reused_threads, 2);
    let events = serde_json::Deserializer::from_reader(file.reopen().unwrap())
        .into_iter::<Event>()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let batches = events
        .iter()
        .filter_map(|event| match event {
            Event::Batch(Batch::Messages { records }) => Some(records),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        batches.iter().map(|batch| batch.len()).collect::<Vec<_>>(),
        [32, 5, 2]
    );
    assert_eq!(batches[0][0].ordinal, 0);
    assert_eq!(batches[1][0].ordinal, 32);
    assert_eq!(batches[2][0].conversation_id, "b");
    assert!(
        batches
            .iter()
            .flat_map(|batch| batch.iter())
            .all(|message| message.vector.is_none() && message.generation.is_none())
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::ReuseGold { .. }))
            .count(),
        2
    );
    assert_eq!(
        Storage::open_catalog(&database)
            .unwrap()
            .silver_certificate()
            .unwrap(),
        silver
    );
    target.threads.clear();
    assert!(
        prepare(&database, &target, None, Instant::now())
            .await
            .err()
            .unwrap()
            .error
            .message
            .contains("run mash index")
    );
    assert_eq!(
        Storage::open_catalog(&database)
            .unwrap()
            .silver_certificate()
            .unwrap(),
        silver
    );
    let error = prepare(
        &database,
        &target,
        Some(Inference {
            models_dir: &directory.path().join("missing-models"),
            embeddings: None,
        }),
        Instant::now(),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(error.error.kind, crate::ErrorKind::Configuration);
}
