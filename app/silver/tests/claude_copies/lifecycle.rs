use super::*;

#[test]
fn claude_schema_upgrade_replays_real_legacy_ids_and_preserves_unrelated_vectors() {
    for version in [32, 33] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("sources");
        fs::create_dir(&root).unwrap();
        let a = root.join("a.jsonl");
        let b = root.join("b.jsonl");
        transcript(&a, &[("common", None, "common sentinel")]);
        transcript(
            &b,
            &[
                ("common", None, "common sentinel"),
                ("tail", Some("common"), "tail sentinel"),
            ],
        );
        let mut legacy = parse_claude(&b).unwrap().conversation.unwrap();
        legacy.claude_lineage = None;
        for (message, uuid) in legacy
            .messages
            .inline_mut()
            .iter_mut()
            .zip(["common", "tail"])
        {
            message.id = stable_id("message", &["claude-code", &b.to_string_lossy(), uuid]);
            message.context = MessageContext::Linear;
        }
        let database = directory.path().join("index.sqlite3");
        let unrelated = test_conversation(
            "codex-retained",
            Provider::Codex,
            directory.path().join("codex.jsonl"),
            "unrelated sentinel",
        );
        let mut storage = Storage::open_writer(&database).unwrap();
        storage.replace_conversation(&legacy).unwrap();
        storage.replace_conversation(&unrelated).unwrap();
        let writes = legacy
            .messages
            .inline()
            .iter()
            .chain(unrelated.messages.inline())
            .map(|message| crate::storage::EmbeddingWrite {
                message_id: &message.id,
                vector: &[127],
                norm: 127.0,
            })
            .collect::<Vec<_>>();
        storage.replace_embeddings("fixture", &writes).unwrap();
        storage.mark_semantic_index_ready("fixture").unwrap();
        for path in [&a, &b] {
            storage
                .record_source_checkpoint(
                    Provider::ClaudeCode,
                    &SourceIdentity::file(path),
                    &source_stamp(path).unwrap(),
                )
                .unwrap();
        }
        storage.commit_writer().unwrap();
        drop(storage);
        let connection = rusqlite::Connection::open(&database).unwrap();
        connection
            .execute_batch("DROP TABLE claude_source_messages; DROP TABLE claude_sources;")
            .unwrap();
        connection
            .pragma_update(None, "user_version", version)
            .unwrap();
        drop(connection);

        let mut storage = Storage::open_writer(&database).unwrap();
        assert!(!storage.semantic_index_is_ready("fixture").unwrap());
        assert_eq!(storage.embedding_count("fixture").unwrap(), 3);
        let summary = index(&mut storage, &options(&root)).unwrap();
        assert_eq!(summary.checkpoint_skipped_sources, 0);
        storage.commit_writer().unwrap();
        assert_eq!(storage.counts().unwrap().messages, 3);
        assert_eq!(storage.embedding_count("fixture").unwrap(), 1);
        for message in legacy.messages.inline() {
            assert!(storage.view(&message.id, 0).unwrap().is_empty());
        }
        drop(storage);
        fs::remove_file(&b).unwrap();
        for _ in 0..2 {
            let mut storage = Storage::open_writer(&database).unwrap();
            let summary = index(&mut storage, &options(&root)).unwrap();
            assert_eq!(summary.checkpoint_skipped_sources, 1);
            storage.commit_writer().unwrap();
            assert_eq!(storage.counts().unwrap().messages, 2);
            assert_eq!(storage.embedding_count("fixture").unwrap(), 1);
            assert_eq!(storage.search("common", 10, None, None).unwrap().len(), 1);
            assert_eq!(
                storage.search("unrelated", 10, None, None).unwrap().len(),
                1
            );
        }
    }
}

#[test]
fn claude_renaming_a_contribution_preserves_native_ids_context_and_vectors() {
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
    let writes = parsed
        .messages
        .inline()
        .iter()
        .map(|message| crate::storage::EmbeddingWrite {
            message_id: &message.id,
            vector: &[127],
            norm: 127.0,
        })
        .collect::<Vec<_>>();
    storage.replace_embeddings("fixture", &writes).unwrap();
    storage.mark_semantic_index_ready("fixture").unwrap();
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
        assert_eq!(storage.embedding_count("fixture").unwrap(), 2);
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
    assert_eq!(storage.embedding_count("fixture").unwrap(), 1);
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
