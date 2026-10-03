use super::*;
use crate::storage::tests::conversation_with_messages;
use std::collections::BTreeMap;

#[test]
fn budgeted_dense_plan_preserves_full_silver_and_lexical_evidence() {
    let directory = tempfile::tempdir().unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    let content = format!("{} trailingevidence", "é".repeat(300));
    storage
        .replace_conversation(&conversation_with_messages(&[("one", &content)]))
        .unwrap();
    let before = storage.messages_needing_embeddings().unwrap();
    let recipe = crate::gold::config::Build {
        max_embedding_chars: Some(256_u32.try_into().unwrap()),
        ..Default::default()
    };
    let mut plan = EmbeddingPlan::with_build(&storage, &recipe).unwrap();
    assert_eq!(plan.total_vectors, 1);
    assert_eq!(plan.next_wave(8).unwrap()[0].content, "é".repeat(256));
    let after = storage.messages_needing_embeddings().unwrap();
    assert_eq!(after[0].id, before[0].id);
    assert_eq!(after[0].content, content);
    storage.refresh_derived_search_state().unwrap();
    assert_eq!(
        storage
            .search("trailingevidence", 10, None, None)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn title_recipe_deduplicates_prepared_text_and_keeps_hidden_messages_excluded() {
    use crate::gold::config::{Build, EmbeddingText};
    let directory = tempfile::tempdir().unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    for (id, title) in [("one", "Qdrant"), ("two", "Postgres")] {
        let mut thread =
            conversation_with_messages(&[(id, "same visible dialogue"), ("hidden", "secret")]);
        thread.id = id.into();
        thread.source = crate::silver::SourceIdentity::file(id);
        thread.title = Some(title.into());
        thread.messages.inline_mut()[1].id = format!("hidden-{id}");
        thread.messages.inline_mut()[1].dialogue_text = Some(String::new());
        storage.replace_conversation(&thread).unwrap();
    }
    let mut default = EmbeddingPlan::new(&storage).unwrap();
    assert_eq!(default.total_vectors, 2);
    assert_eq!(default.next_wave(10).unwrap().len(), 1);
    let mut titled = EmbeddingPlan::with_build(
        &storage,
        &Build {
            max_embedding_chars: None,
            representations: crate::gold::config::Representations::LexicalAndDense,
            dense_content: crate::gold::config::DenseContent::AllVisible,
            text_preparation: crate::gold::config::TextPreparation::Preserve,
            embedding_text: EmbeddingText::TitleAndDialogue,
        },
    )
    .unwrap();
    assert_eq!(titled.total_vectors, 2);
    let texts = titled
        .next_wave(10)
        .unwrap()
        .into_iter()
        .map(|t| t.content)
        .collect::<Vec<_>>();
    assert_eq!(texts.len(), 2);
    assert!(texts.iter().all(|text| text.starts_with("Thread: ")
        && text.ends_with("same visible dialogue")
        && !text.contains("secret")));
}

#[cfg(unix)]
#[test]
fn embedding_plan_files_and_directory_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let storage = Storage::open_writer(&root.path().join("index.sqlite3")).unwrap();
    let plan = EmbeddingPlan::new(&storage).unwrap();
    let file = std::path::Path::new(plan.connection.path().unwrap());
    let directory = file.parent().unwrap();
    assert_eq!(
        directory.metadata().unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
}

#[test]
fn disk_plan_preserves_byte_order_projections_duplicates_and_empty_plans() {
    let directory = tempfile::tempdir().unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    let mut empty = EmbeddingPlan::new(&storage).unwrap();
    assert_eq!(empty.total_vectors, 0);
    assert!(empty.next_wave(8).unwrap().is_empty());
    let mut conversation = conversation_with_messages(&[
        ("z", "duplicate"),
        ("a", "duplicate"),
        ("unicode", "é"),
        ("ascii", "zz"),
        ("nul", "a\0b"),
        ("prefix", "a"),
        ("projected", "canonical text"),
        ("hidden", "context only"),
    ]);
    conversation.messages.inline_mut()[6].dialogue_text = Some("visible projection".into());
    conversation.messages.inline_mut()[7].dialogue_text = Some(String::new());
    storage.replace_conversation(&conversation).unwrap();
    let mut expected = BTreeMap::<(usize, String), Vec<String>>::new();
    for message in storage.messages_needing_embeddings().unwrap() {
        expected
            .entry((message.content.len(), message.content))
            .or_default()
            .push(message.id);
    }
    let mut plan = EmbeddingPlan::new(&storage).unwrap();
    assert_eq!(plan.total_vectors, 7);
    let mut actual = Vec::new();
    loop {
        let wave = plan.next_wave(2).unwrap();
        assert!(wave.len() <= 2);
        if wave.is_empty() {
            break;
        }
        for text in wave {
            let ids = plan.message_ids(text.group_id, None).unwrap();
            actual.push(((text.content.len(), text.content), ids));
        }
    }
    assert_eq!(actual, expected.into_iter().collect::<Vec<_>>());
    assert!(plan.next_wave(2).unwrap().is_empty());
}

#[test]
fn duplicate_id_pages_survive_source_dequeues_and_plan_files_are_removed() {
    let directory = tempfile::tempdir().unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    let ids = (0..MESSAGE_PAGE_SIZE * 3 + 7)
        .map(|i| format!("message-{i:06}"))
        .collect::<Vec<_>>();
    let messages = ids
        .iter()
        .map(|id| (id.as_str(), "one shared text"))
        .collect::<Vec<_>>();
    storage
        .replace_conversation(&conversation_with_messages(&messages))
        .unwrap();
    let mut plan = EmbeddingPlan::new(&storage).unwrap();
    let path = std::path::Path::new(plan.connection.path().unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    let text = plan.next_wave(64).unwrap().pop().unwrap();
    assert_eq!(plan.total_vectors, ids.len() as u64);
    let mut after: Option<String> = None;
    let mut actual = Vec::new();
    loop {
        let page = plan.message_ids(text.group_id, after.as_deref()).unwrap();
        assert!(page.len() <= MESSAGE_PAGE_SIZE);
        if page.is_empty() {
            break;
        }
        let writes = page
            .iter()
            .map(|id| super::super::EmbeddingWrite {
                message_id: id,
                vector: &[127],
                norm: 127.0,
            })
            .collect::<Vec<_>>();
        storage.replace_embeddings("generation", &writes).unwrap();
        after = page.last().cloned();
        actual.extend(page);
    }
    assert_eq!(actual, ids);
    assert!(!storage.has_pending_embeddings().unwrap());
    assert_eq!(
        storage.embedding_count("generation").unwrap(),
        ids.len() as u64
    );
    drop(plan);
    assert!(!path.exists());
}

#[test]
fn invalid_text_fails_planning_without_dequeuing_or_replacing_vectors() {
    let directory = tempfile::tempdir().unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    storage
        .replace_conversation(&conversation_with_messages(&[("one", "valid")]))
        .unwrap();
    storage
        .replace_embeddings(
            "previous",
            &[super::super::EmbeddingWrite {
                message_id: "one",
                vector: &[127],
                norm: 127.0,
            }],
        )
        .unwrap();
    storage
        .connection
        .execute_batch(
            "INSERT INTO pending_embeddings(message_id) VALUES ('one');
         UPDATE messages SET search_projection = CAST(x'ff' AS TEXT) WHERE id = 'one'",
        )
        .unwrap();
    assert!(EmbeddingPlan::new(&storage).is_err());
    assert!(storage.has_pending_embeddings().unwrap());
    assert_eq!(storage.embedding_count("previous").unwrap(), 1);
    let bytes: Vec<u8> = storage
        .connection
        .query_row(
            "SELECT vector FROM message_embeddings WHERE message_id = 'one'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(bytes, [127]);
}
