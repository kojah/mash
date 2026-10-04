use super::*;
use crate::silver::{Conversation, NormalizedMessage, SourceIdentity};

#[test]
fn incremental_capture_includes_only_changed_complete_threads_and_explicit_removals() {
    let directory = tempfile::tempdir().unwrap();
    let database = seed(directory.path());
    let storage = Storage::open_existing(&database).unwrap();
    storage.prepare_publication_target("journal").unwrap();
    let mut instance = String::new();
    let base = storage
        .with_publication_snapshot(|snapshot| {
            instance.clone_from(&snapshot.instance);
            Ok(())
        })
        .unwrap();
    storage.acknowledge_publication("journal", base).unwrap();
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection.execute_batch("PRAGMA foreign_keys=ON; UPDATE conversations SET title='changed' WHERE id='a'; DELETE FROM conversations WHERE id='b';").unwrap();
    let target = Target {
        schema_version: 1,
        build: crate::gold::config::Build::default(),
        generation: "unused".into(),
        dimensions: 384,
        publication_version: "pinned".into(),
        instance: Some(instance),
        publication_revision: base,
        threads: BTreeMap::new(),
    };
    let source = capture(&database, &target, true, true).unwrap();
    assert!(source.incremental);
    assert_eq!(source.conversations.keys().collect::<Vec<_>>(), vec!["a"]);
    let events: Vec<Event> = std::fs::read_to_string(source.file.path())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let Event::Start(start) = &events[0] else {
        panic!("expected start")
    };
    assert!(!start.snapshot && !start.reuse_gold);
    assert_eq!(start.base_revision, Some(base));
    assert!(
        events
            .iter()
            .any(|event| matches!(event,Event::Remove{id} if id=="b"))
    );
    let mut messages = 0;
    for event in events {
        if let Event::Batch(Batch::Messages { records }) = event {
            assert!(records.iter().all(|record| record.conversation_id == "a"));
            messages += records.len();
        }
    }
    assert_eq!(messages, 37);
}

#[test]
fn redacted_snapshot_fingerprints_match_uploaded_dependencies() {
    let root = tempfile::tempdir().unwrap();
    let database = seed(root.path());
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection.execute("UPDATE messages SET content='password=\"hunter2\"',search_projection='password=\"hunter2\"' WHERE id='a-100'",[]).unwrap();
    connection
        .execute(
            "UPDATE conversations SET title='password=\"rosebud\"' WHERE id='a'",
            [],
        )
        .unwrap();
    let target = Target {
        schema_version: 1,
        build: crate::gold::config::Build::default(),
        generation: "unused".into(),
        dimensions: 384,
        publication_version: "pinned".into(),
        instance: None,
        publication_revision: 0,
        threads: BTreeMap::new(),
    };
    let source = capture(&database, &target, false, false).unwrap();
    let stream = std::fs::read_to_string(source.file.path()).unwrap();
    assert!(!stream.contains("hunter2") && !stream.contains("rosebud"));
    let mut fingerprints = BTreeMap::new();
    for line in stream.lines() {
        match serde_json::from_str::<Event>(line).unwrap() {
            Event::Batch(Batch::Conversations { records }) => {
                for c in records {
                    fingerprints.insert(c.id.clone(), Fingerprint::new(&c).unwrap());
                }
            }
            Event::Batch(Batch::Messages { records }) => {
                for m in records {
                    fingerprints
                        .get_mut(&m.conversation_id)
                        .unwrap()
                        .push(&m)
                        .unwrap();
                }
            }
            _ => {}
        }
    }
    for (id, fingerprint) in fingerprints {
        assert_eq!(source.hashes[&id], fingerprint.finish());
    }
    let canonical: String = connection
        .query_row("SELECT content FROM messages WHERE id='a-100'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(canonical.contains("hunter2"));
}

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
                        attachments: Vec::new(),
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
async fn parent_units_resolve_from_consistent_sqlite_snapshot_across_batches_and_threads() {
    let directory = tempfile::tempdir().unwrap();
    let database = seed(directory.path());
    let silver = Storage::open_catalog(&database)
        .unwrap()
        .silver_certificate()
        .unwrap();
    let target = Target {
        schema_version: 1,
        build: crate::gold::config::Build {
            retrieval_unit: crate::gold::config::RetrievalUnit::ParentAndMessage,
            ..Default::default()
        },
        generation: "unused-model".into(),
        dimensions: 4,
        publication_version: "pinned".into(),
        instance: None,
        publication_revision: 0,
        threads: BTreeMap::new(),
    };
    let source = capture(&database, &target, false, false).unwrap();
    let mut reader = Reader::new(source.file.reopen().unwrap()).unwrap();
    let mut inputs = BTreeMap::new();
    let mut sizes = Vec::new();
    loop {
        let (next, batch) = reader.next().unwrap();
        reader = next;
        if batch.is_empty() {
            break;
        }
        sizes.push(batch.len());
        let (next, batch, prepared) = prepare_units(reader, batch, &target.build, None)
            .await
            .unwrap();
        reader = next;
        for (row, text) in batch
            .iter()
            .filter(|m| !m.projection.is_empty())
            .zip(prepared)
        {
            inputs.insert(row.id.clone(), text);
        }
    }
    assert_eq!(sizes, [32, 5, 2]);
    assert_eq!(inputs["a-67"], "canonical a message 33");
    assert_eq!(
        inputs["a-66"],
        "canonical a message 34\n\nParent message:\ncanonical a message 33"
    );
    assert_eq!(inputs["b-100"], "canonical b message 0");
    assert_eq!(
        inputs["b-99"],
        "canonical b message 1\n\nParent message:\ncanonical b message 0"
    );
    assert_eq!(inputs.len(), 29);
    assert_eq!(
        Storage::open_catalog(&database)
            .unwrap()
            .silver_certificate()
            .unwrap(),
        silver
    );
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
            retrieval_unit: crate::gold::config::RetrievalUnit::Message,
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
    target.threads = capture(&database, &target, true, false).unwrap().hashes;
    let (file, _, preparation) = prepare(&database, &target, None, Instant::now(), false)
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
        prepare(&database, &target, None, Instant::now(), false)
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
        false,
    )
    .await
    .err()
    .unwrap();
    assert_eq!(error.error.kind, crate::ErrorKind::Configuration);
}
