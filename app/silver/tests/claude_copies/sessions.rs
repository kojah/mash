//! Native forks retain original session IDs on inherited dialogue.
use super::*;

fn write_sessions(path: &Path, rows: &[(&str, &str, Option<&str>)]) {
    let records = rows
        .iter()
        .map(|(session, uuid, parent)| {
            serde_json::json!({
                "type": "user", "sessionId": session, "uuid": uuid,
                "parentUuid": parent,
                "message": {"role": "user", "content": format!("{uuid} sentinel")},
            })
            .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(path, records).unwrap();
}

#[test]
fn claude_forks_keep_original_and_inherited_threads_distinct_across_copies_and_restart() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("sources");
    fs::create_dir(&root).unwrap();
    let original = root.join("original.jsonl");
    let fork = root.join("arbitrary-filename.jsonl");
    let copy = root.join("renamed-copy.jsonl");
    write_sessions(
        &original,
        &[
            ("original", "root", None),
            ("original", "old-tail", Some("root")),
        ],
    );
    // Branch from an earlier message, not necessarily the prefix's last row.
    write_sessions(
        &fork,
        &[
            ("original", "root", None),
            ("original", "old-tail", Some("root")),
            ("fork", "new-tail", Some("root")),
        ],
    );
    fs::copy(&fork, &copy).unwrap();
    let parsed = parse_claude(&fork).unwrap().chat.unwrap();
    assert_eq!(parsed.id, "fork");
    let new_tail = stable_id("claude-message", &["fork", "new-tail"]);
    let database = directory.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    let summary = index(&mut storage, &options(&root)).unwrap();
    assert_eq!(summary.malformed_records, 0);
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().chats, 2);
    assert_eq!(storage.counts().unwrap().messages, 5);
    let context = storage.view(&new_tail, 10).unwrap();
    assert_eq!(context.len(), 2);
    assert_eq!(
        context[0].id,
        stable_id("claude-message", &["fork", "root"])
    );
    assert_eq!(context[1].id, new_tail);
    assert_ne!(
        context[0].id,
        stable_id("claude-message", &["original", "root"])
    );
    drop(storage);
    fs::remove_file(&fork).unwrap();
    let mut storage = Storage::open_writer(&database).unwrap();
    assert_eq!(
        index(&mut storage, &options(&root))
            .unwrap()
            .checkpoint_skipped_sources,
        2
    );
    storage.commit_writer().unwrap();
    assert_eq!(storage.view(&new_tail, 10).unwrap().len(), 2);
    drop(storage);
    fs::remove_file(&copy).unwrap();
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&root)).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().chats, 1);
    assert_eq!(storage.counts().unwrap().messages, 2);
}

#[test]
fn claude_nested_forks_resolve_native_metadata_at_each_transition() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("fork.jsonl");
    write_sessions(
        &path,
        &[
            ("original", "root", None),
            ("first-fork", "middle", Some("bridge")),
            ("second-fork", "tail", Some("middle")),
        ],
    );
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"\n{\"type\":\"attachment\",\"uuid\":\"bridge\",\"parentUuid\":\"root\"}\n")
        .unwrap();
    let parsed = parse_claude(&path).unwrap().chat.unwrap();
    assert_eq!(parsed.id, "second-fork");
    assert_eq!(parsed.messages.len(), 3);
    assert_eq!(
        parsed.messages.inline()[1].context,
        MessageContext::Parent {
            id: parsed.messages.inline()[0].id.clone()
        }
    );
    assert_eq!(
        parsed.messages.inline()[2].context,
        MessageContext::Parent {
            id: parsed.messages.inline()[1].id.clone()
        }
    );
}

#[test]
fn claude_unproven_session_transitions_roll_back_without_checkpointing() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("sources");
    fs::create_dir(&root).unwrap();
    let original = root.join("original.jsonl");
    let fork = root.join("fork.jsonl");
    write_sessions(&original, &[("original", "root", None)]);
    let database = directory.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&root)).unwrap();
    storage.commit_writer().unwrap();
    for parent in [None, Some("unknown"), Some("future")] {
        drop(storage);
        storage = Storage::open_writer(&database).unwrap();
        write_sessions(
            &fork,
            &[
                ("original", "root", None),
                ("fork", "tail", parent),
                ("fork", "future", Some("root")),
            ],
        );
        let error = index(&mut storage, &options(&root)).err().unwrap();
        assert!(
            error
                .error
                .message
                .contains("not linked to inherited dialogue"),
            "{}",
            error.error.message
        );
        storage.commit_writer().unwrap();
        assert_eq!(storage.counts().unwrap().messages, 1);
        assert!(
            !storage
                .source_checkpoint_matches(
                    Provider::ClaudeCode,
                    &SourceIdentity::file(&fork),
                    &source_stamp(&fork).unwrap()
                )
                .unwrap()
        );
    }
    // Missing parent is distinct from explicit root and must not use the
    // ordinary linear fallback to manufacture evidence of a native fork.
    let records = fs::read_to_string(&fork).unwrap();
    let records = records.replace("\"parentUuid\":\"future\",", "");
    fs::write(&fork, records).unwrap();
    assert!(parse_claude(&fork).is_err());
    write_sessions(
        &fork,
        &[
            ("original", "root", None),
            ("fork", "tail", Some("root")),
            ("original", "return", Some("tail")),
        ],
    );
    assert!(
        parse_claude(&fork)
            .err()
            .unwrap()
            .error
            .message
            .contains("interleaves")
    );
    write_sessions(
        &fork,
        &[("original", "root", None), ("fork", "tail", Some("root"))],
    );
    drop(storage);
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&root)).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().chats, 2);
    assert_eq!(storage.counts().unwrap().messages, 3);
}
