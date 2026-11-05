use super::*;
use crate::silver::{Chat, NormalizedMessage, SourceIdentity};

fn publication_target(fixture: &crate::views::local::test_support::Fixture) -> Target {
    Target {
        build: fixture.configuration.views.build.clone(),
        generation: fixture.local.scope.generation.clone(),
        dimensions: fixture.local.scope.dimensions,
        instance: None,
        publication_revision: 0,
        redaction: crate::redaction::Policy::ON,
    }
}

async fn prepared_snapshot(
    database: &Path,
    target: &Target,
    configuration: &crate::config::ResolvedConfig,
) -> (tempfile::NamedTempFile, SnapshotWork) {
    let (file, _, work) = prepare(
        database,
        target,
        Some(Inference {
            models_dir: Path::new("/nonexistent/mash-test-models"),
            embeddings: configuration.embeddings.as_ref(),
            local: Some(configuration),
        }),
        Instant::now(),
        Selection::Replace,
    )
    .await
    .unwrap();
    (file, work)
}

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_publication_reuses_local_vectors_and_survives_missing_local_views() {
    let directory = tempfile::tempdir().unwrap();
    let database = seed(directory.path());
    let fixture = crate::views::local::test_support::Fixture::new(&database, "publisher").await;
    let target = publication_target(&fixture);
    // A dead embedding endpoint proves that matching cached vectors need no inference.
    let mut offline = fixture.configuration.clone();
    offline.embeddings.as_mut().unwrap().url = "http://127.0.0.1:9/embeddings".into();
    let (cached, work) = prepared_snapshot(&database, &target, &offline).await;
    assert_eq!(work.forwarded_embeddings, 29);
    assert_eq!(work.model_inferences, 0);
    let cached = std::fs::read_to_string(cached.path()).unwrap();
    fixture.cleanup().await;
    let (fresh, work) = prepared_snapshot(&database, &target, &fixture.configuration).await;
    assert_eq!(work.forwarded_embeddings, 0);
    assert_eq!(work.model_inferences, 29);
    assert_eq!(std::fs::read_to_string(fresh.path()).unwrap(), cached);
}

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_publication_reuse_obeys_target_recipe_and_model() {
    let directory = tempfile::tempdir().unwrap();
    let database = seed(directory.path());
    let fixture = crate::views::local::test_support::Fixture::new(&database, "publisher").await;
    let original = publication_target(&fixture);
    let mut target = original.clone();
    target.build.embedding_text = crate::views::config::EmbeddingText::TitleAndDialogue;
    let (_, work) = prepared_snapshot(&database, &target, &fixture.configuration).await;
    assert_eq!(work.forwarded_embeddings, 0);
    assert_eq!(work.model_inferences, 29);

    let mut other_model = fixture.configuration.clone();
    other_model.embeddings.as_mut().unwrap().model = "different-model".into();
    target = original;
    target.generation = crate::semantic::configured_generation(other_model.embeddings.as_ref());
    let (_, work) = prepared_snapshot(&database, &target, &other_model).await;
    assert_eq!(work.forwarded_embeddings, 0);
    assert_eq!(work.model_inferences, 29);
    fixture.cleanup().await;
}

#[tokio::test]
#[ignore = "requires disposable Qdrant"]
async fn qdrant_publication_reuse_obeys_redacted_snapshot_input() {
    let directory = tempfile::tempdir().unwrap();
    let database = seed(directory.path());
    let fixture = crate::views::local::test_support::Fixture::new(&database, "publisher").await;
    crate::storage::test_support::write(
        &database,
        "UPDATE messages SET content_packed='password=\"hunter2\"',search_projection='password=\"hunter2\"' WHERE id='b-100'",
    );
    let (file, work) = prepared_snapshot(
        &database,
        &publication_target(&fixture),
        &fixture.configuration,
    )
    .await;
    assert_eq!(work.forwarded_embeddings, 28);
    assert_eq!(work.model_inferences, 1);
    assert!(
        !std::fs::read_to_string(file.path())
            .unwrap()
            .contains("hunter2")
    );
    fixture.cleanup().await;
}

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
    storage
        .acknowledge_publication(
            "journal",
            base,
            crate::storage::publication::JournalCoverage::Incremental { through: base },
        )
        .unwrap();
    crate::storage::test_support::write(
        &database,
        "UPDATE chats SET title='changed' WHERE id='a'; DELETE FROM chats WHERE id='b';",
    );
    let target = Target {
        build: crate::views::config::Build::default(),
        generation: "unused".into(),
        dimensions: 384,
        instance: Some(instance),
        publication_revision: base,
        redaction: crate::redaction::Policy::ON,
    };
    let source = capture(&database, &target, Selection::Delta { journal_base: base }).unwrap();
    assert_eq!(source.chats.keys().collect::<Vec<_>>(), vec!["a"]);
    let events: Vec<Event> = std::fs::read_to_string(source.file.path())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let Event::Start(start) = &events[0] else {
        panic!("expected start")
    };
    assert!(!start.snapshot);
    assert_eq!(start.base_revision, Some(base));
    // Baselines can give the server a different revision sequence. Select
    // changed local rows from the mapped journal base, but send its server head.
    let mut mapped = target.clone();
    mapped.publication_revision = base - 1;
    let mapped_source =
        capture(&database, &mapped, Selection::Delta { journal_base: base }).unwrap();
    assert_eq!(mapped_source.chats.keys().collect::<Vec<_>>(), vec!["a"]);
    let first: Event = serde_json::from_str(
        std::fs::read_to_string(mapped_source.file.path())
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    let Event::Start(mapped_start) = first else {
        panic!("expected start")
    };
    assert_eq!(mapped_start.base_revision, Some(base - 1));
    assert!(!mapped_start.snapshot);
    assert!(
        events
            .iter()
            .any(|event| matches!(event,Event::Remove{id} if id=="b"))
    );
    let mut messages = 0;
    for event in events {
        if let Event::Batch(Batch::Messages { records }) = event {
            assert!(records.iter().all(|record| record.chat_id == "a"));
            messages += records.len();
        }
    }
    assert_eq!(messages, 37);
}

#[test]
fn snapshots_upload_redacted_records_and_keep_canonical_silver() {
    let root = tempfile::tempdir().unwrap();
    let database = seed(root.path());
    let connection = crate::storage::test_support::connect(&database);
    connection.execute("UPDATE messages SET content_packed='password=\"hunter2\"',search_projection='password=\"hunter2\"' WHERE id='a-100'",[]).unwrap();
    connection
        .execute(
            "UPDATE chats SET title='password=\"rosebud\"' WHERE id='a'",
            [],
        )
        .unwrap();
    let target = Target {
        build: crate::views::config::Build::default(),
        generation: "unused".into(),
        dimensions: 384,
        instance: None,
        publication_revision: 0,
        redaction: crate::redaction::Policy::ON,
    };
    let source = capture(&database, &target, Selection::Replace).unwrap();
    let stream = std::fs::read_to_string(source.file.path()).unwrap();
    assert!(!stream.contains("hunter2") && !stream.contains("rosebud"));
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
            .replace_chat(&Chat {
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
                        tools: crate::tools::Activity::default(),
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
    let silver = Storage::open_silver(&database)
        .unwrap()
        .silver_certificate()
        .unwrap();
    let target = Target {
        build: crate::views::config::Build {
            retrieval_unit: crate::views::config::RetrievalUnit::ParentAndMessage,
            ..Default::default()
        },
        generation: "unused-model".into(),
        dimensions: 4,
        instance: None,
        publication_revision: 0,
        redaction: crate::redaction::Policy::ON,
    };
    let source = capture(&database, &target, Selection::Replace).unwrap();
    let mut reader =
        Reader::new(source.file.reopen().unwrap(), crate::redaction::Policy::ON).unwrap();
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
        Storage::open_silver(&database)
            .unwrap()
            .silver_certificate()
            .unwrap(),
        silver
    );
}
