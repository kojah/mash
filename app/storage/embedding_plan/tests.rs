use super::*;
use crate::storage::tests::conversation_with_messages;
use std::collections::BTreeMap;

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
