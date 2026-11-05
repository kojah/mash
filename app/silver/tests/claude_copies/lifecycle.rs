use super::*;

#[test]
fn claude_renaming_a_contribution_preserves_native_ids_and_context() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("sources");
    fs::create_dir(&root).unwrap();
    let original = root.join("a-original.jsonl");
    let moved = root.join("b-moved.jsonl");
    let duplicate = root.join("z-duplicate.jsonl");
    transcript(
        &original,
        &[
            ("common", None, "common sentinel"),
            ("tail", Some("common"), "tail sentinel"),
        ],
    );
    transcript(&duplicate, &[("common", None, "common sentinel")]);
    let parsed = parse_claude(&original).unwrap().conversation.unwrap();
    let database = directory.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&root)).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    fs::rename(&original, &moved).unwrap();
    for expected_skips in [1, 2] {
        let mut storage = Storage::open_writer(&database).unwrap();
        let summary = index(&mut storage, &options(&root)).unwrap();
        assert_eq!(summary.checkpoint_skipped_sources, expected_skips);
        assert_eq!(summary.changed_messages, 0);
        storage.commit_writer().unwrap();
        assert_eq!(storage.counts().unwrap().messages, 2);
        let context = storage.view(&parsed.messages.inline()[1].id, 1).unwrap();
        assert_eq!(context.len(), 2);
        assert_eq!(context[0].id, parsed.messages.inline()[0].id);
        assert_eq!(context[1].id, parsed.messages.inline()[1].id);
    }
    fs::remove_file(&moved).unwrap();
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&root)).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 1);
    assert!(
        storage
            .view(&parsed.messages.inline()[1].id, 0)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn claude_failed_provider_scan_rolls_back_all_contributions_and_checkpoints() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("sources");
    fs::create_dir(&root).unwrap();
    let a = root.join("a.jsonl");
    let failure = root.join("failure.jsonl");
    for path in [&a, &failure] {
        transcript(path, &[("common", None, "common sentinel")]);
    }
    let database = directory.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&root)).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let old = [&a, &failure].map(|path| source_stamp(path).unwrap());
    for (path, uuid) in [(&a, "left"), (&failure, "right")] {
        // Largest sources are admitted first. Keep the successful source first
        // even when the host has only one parser worker.
        let text = if uuid == "left" {
            "new branch sentinel with enough padding to be scheduled first"
        } else {
            "new branch sentinel"
        };
        transcript(
            path,
            &[
                ("common", None, "common sentinel"),
                (uuid, Some("common"), text),
            ],
        );
    }
    let mut storage = Storage::open_writer(&database).unwrap();
    let discovery =
        discover_history_files(Provider::ClaudeCode, std::slice::from_ref(&root), true).unwrap();
    let mut summary = empty_summary();
    assert!(
        index_provider(
            &mut storage,
            ProviderScan {
                provider: Provider::ClaudeCode,
                roots: std::slice::from_ref(&root),
                cutoff_modified_ns: None,
                parse: Parser::Custom(parse_with_late_failure),
                authoritative: true,
            },
            discovery,
            &mut summary,
            &mut IndexProgress::new(),
        )
        .is_err()
    );
    assert_eq!(
        summary.changed_messages, 1,
        "failure must follow an applied contribution"
    );
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 1);
    for (path, stamp) in [&a, &failure].into_iter().zip(&old) {
        assert!(storage.source_checkpoint_matches(
            Provider::ClaudeCode, &SourceIdentity::file(path), stamp,
        ).unwrap());
    }
    drop(storage);
    // Remove the failed contribution first: an accidentally committed source
    // snapshot would resurrect its branch when the surviving copy is derived.
    fs::remove_file(&failure).unwrap();
    let mut storage = Storage::open_writer(&database).unwrap();
    storage
        .remove_source(Provider::ClaudeCode, &SourceIdentity::file(&failure))
        .unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 1);
    drop(storage);
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&root)).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 2);
    let left = stable_id("claude-message", &["copied-thread", "left"]);
    assert_eq!(storage.view(&left, 1).unwrap().len(), 2);
}

#[test]
fn claude_refresh_writes_only_changed_contribution_rows() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("sources");
    fs::create_dir(&root).unwrap();
    let path = root.join("session.jsonl");
    let database = directory.path().join("index.sqlite3");
    let refresh = |rows: &[(&str, Option<&str>, &str)]| {
        transcript(&path, rows);
        let mut storage = Storage::open_writer(&database).unwrap();
        index(&mut storage, &options(&root)).unwrap();
        storage.commit_writer().unwrap();
    };
    refresh(&[("a", None, "first"), ("b", Some("a"), "second")]);
    let log = rusqlite::Connection::open(&database).unwrap();
    log.execute_batch(
        "CREATE TABLE contribution_writes(operation TEXT NOT NULL);
         CREATE TRIGGER contribution_inserted AFTER INSERT ON claude_source_messages
         BEGIN INSERT INTO contribution_writes VALUES ('insert'); END;
         CREATE TRIGGER contribution_updated AFTER UPDATE ON claude_source_messages
         BEGIN INSERT INTO contribution_writes VALUES ('update'); END;
         CREATE TRIGGER contribution_deleted AFTER DELETE ON claude_source_messages
         BEGIN INSERT INTO contribution_writes VALUES ('delete'); END;",
    )
    .unwrap();
    let writes = || {
        let operations = log
            .prepare("SELECT operation FROM contribution_writes ORDER BY rowid")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        log.execute("DELETE FROM contribution_writes", []).unwrap();
        operations
    };
    // An appended transcript writes only its new row, not the whole source.
    refresh(&[
        ("a", None, "first"),
        ("b", Some("a"), "second"),
        ("c", Some("b"), "third"),
    ]);
    assert_eq!(writes(), ["insert"]);
    refresh(&[
        ("a", None, "first"),
        ("b", Some("a"), "second, edited"),
        ("c", Some("b"), "third"),
    ]);
    assert_eq!(writes(), ["update"]);
    refresh(&[("a", None, "first"), ("b", Some("a"), "second, edited")]);
    assert_eq!(writes(), ["delete"]);
    // Rewriting identical history changes neither rows nor the silver revision,
    // so certified views over it stay current.
    let revision = || {
        Storage::open_silver(&database)
            .unwrap()
            .silver_certificate()
            .unwrap()
            .revision
    };
    let before = revision();
    std::thread::sleep(std::time::Duration::from_millis(20));
    refresh(&[("a", None, "first"), ("b", Some("a"), "second, edited")]);
    assert_eq!(writes(), Vec::<String>::new());
    assert_eq!(revision(), before);
    let storage = Storage::open_silver(&database).unwrap();
    assert_eq!(storage.counts().unwrap().messages, 2);
    let edited = stable_id("claude-message", &["copied-thread", "b"]);
    let context = storage.view(&edited, 1).unwrap();
    assert_eq!(context.len(), 2);
    assert!(
        context
            .iter()
            .any(|message| message.content == "second, edited")
    );
}
