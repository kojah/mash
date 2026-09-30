use super::*;

#[test]
fn purge_requires_a_complete_provider_scan() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database = directory.path().join("mash.sqlite3");
    let claude_root = directory.path().join("claude");
    fs::create_dir_all(&claude_root).expect("Claude root");
    let missing_source = claude_root.join("missing.jsonl");

    let mut writer = Storage::open_writer(&database).expect("writer");
    writer
        .replace_conversation(&Conversation {
            id: "old-session".to_owned(),
            provider: Provider::ClaudeCode,
            source_path: missing_source,
            project: None,
            repository: None,
            title: None,
            created_at: None,
            updated_at: None,
            messages: vec![NormalizedMessage {
                id: "old-message".to_owned(),
                ordinal: 0,
                role: crate::role::Role::User,
                content: "preserve until complete".to_owned(),
                search_projection: None,
                created_at: None,
            }],
        })
        .expect("seed source");
    writer.commit_writer().expect("commit seed");
    drop(writer);

    fs::write(claude_root.join("malformed.jsonl"), "not-json\n").expect("malformed source");
    let options = IndexOptions {
        providers: [(Provider::ClaudeCode, vec![claude_root.clone()])].into(),
        roots_are_authoritative: true,
        since_days: None,
    };
    let mut writer = Storage::open_writer(&database).expect("incomplete writer");
    let incomplete = index(&mut writer, &options).expect("bounded malformed scan");
    assert_eq!(incomplete.malformed_records, 1);
    writer.commit_writer().expect("commit incomplete scan");
    assert_eq!(writer.counts().expect("counts").conversations, 1);
    drop(writer);

    fs::remove_file(claude_root.join("malformed.jsonl")).expect("remove malformed source");
    let mut writer = Storage::open_writer(&database).expect("complete writer");
    let complete = index(&mut writer, &options).expect("complete scan");
    assert_eq!(complete.purged_conversations, 1);
    writer.commit_writer().expect("commit complete scan");
    assert_eq!(writer.counts().expect("counts").conversations, 0);
}

#[test]
fn authoritative_roots_index_only_the_two_concrete_formats() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let claude = directory.path().join("claude");
    let codex = directory.path().join("codex");
    fs::create_dir(&claude).expect("Claude root");
    fs::create_dir(&codex).expect("Codex root");
    write_claude_session(
        &claude.join("claude.jsonl"),
        "claude-session",
        "claude sentinel",
    );
    fs::write(
        claude.join("unsupported.jsonl"),
        "{\"info\":{\"id\":\"opencode-session\"},\"messages\":[{\"role\":\"user\",\"content\":\"unsupported sentinel\"}]}\n",
    )
    .expect("unsupported fixture");
    fs::write(
        codex.join("codex.jsonl"),
        concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"codex-session\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"id\":\"codex-message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"codex sentinel\"}]}}\n"
        ),
    )
    .expect("Codex fixture");
    let mut writer = Storage::open_writer(&directory.path().join("mash.sqlite3")).expect("writer");
    let summary = index(
        &mut writer,
        &IndexOptions {
            providers: [
                (Provider::ClaudeCode, vec![claude]),
                (Provider::Codex, vec![codex]),
            ]
            .into(),
            roots_are_authoritative: true,
            since_days: None,
        },
    )
    .expect("configured index");

    assert_eq!(summary.scanned_files, 3);
    assert_eq!(writer.counts().expect("counts").conversations, 2);
    writer.commit_writer().expect("commit configured index");
    assert_eq!(
        writer
            .search(
                "claude",
                10,
                Some(crate::provider::Provider::ClaudeCode),
                None
            )
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        writer
            .search("codex", 10, Some(crate::provider::Provider::Codex), None)
            .unwrap()
            .len(),
        1
    );
    assert!(
        writer
            .search("unsupported", 10, None, None)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn every_authoritative_root_is_preflighted_before_ingestion_writes() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let claude_root = directory.path().join("claude");
    fs::create_dir(&claude_root).expect("Claude root");
    write_claude_session(&claude_root.join("session.jsonl"), "session", "sentinel");
    let options = IndexOptions {
        providers: [
            (Provider::ClaudeCode, vec![claude_root]),
            (
                Provider::Codex,
                vec![directory.path().join("missing-codex")],
            ),
        ]
        .into(),
        roots_are_authoritative: true,
        since_days: None,
    };
    let mut writer = Storage::open_writer(&directory.path().join("mash.sqlite3")).expect("writer");

    let error = index(&mut writer, &options).err().expect("missing root");

    assert_eq!(error.error.kind, crate::ErrorKind::Configuration);
    assert_eq!(writer.counts().expect("counts").conversations, 0);
    assert_eq!(writer.counts().expect("counts").messages, 0);
}

#[test]
fn missing_builtin_root_is_nonfatal_but_never_authorizes_purge() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path().join("claude");
    fs::create_dir(&root).expect("Claude root");
    write_claude_session(&root.join("session.jsonl"), "session", "durable sentinel");
    let database = directory.path().join("mash.sqlite3");
    let options = IndexOptions {
        providers: [(Provider::ClaudeCode, vec![root.clone()])].into(),
        roots_are_authoritative: false,
        since_days: None,
    };
    let mut writer = Storage::open_writer(&database).expect("initial writer");
    index(&mut writer, &options).expect("initial index");
    writer.commit_writer().expect("commit initial index");
    drop(writer);
    fs::rename(&root, directory.path().join("offline")).expect("take root offline");

    let mut writer = Storage::open_writer(&database).expect("offline writer");
    let refresh = index(&mut writer, &options).expect("nonfatal refresh");

    assert_eq!(refresh.purged_conversations, 0);
    assert_eq!(writer.counts().expect("counts").conversations, 1);
    assert_eq!(
        writer
            .search("durable", 10, None, None)
            .expect("preserved search row")
            .len(),
        1
    );
    drop(writer);

    fs::write(&root, "not a JSONL history").expect("replace root with unusable file");
    let mut writer = Storage::open_writer(&database).expect("wrong-type writer");
    let refresh = index(&mut writer, &options).expect("nonfatal wrong-type refresh");
    assert_eq!(refresh.purged_conversations, 0);
    assert_eq!(writer.counts().expect("counts").conversations, 1);
}

#[test]
fn configured_root_disappearing_after_discovery_rolls_back_without_purge() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path().join("configured");
    fs::create_dir(&root).expect("configured root");
    let discovery =
        discover_jsonl_files(std::slice::from_ref(&root), true).expect("initial empty discovery");
    let database = directory.path().join("mash.sqlite3");
    let mut writer = Storage::open_writer(&database).expect("seed writer");
    writer
        .replace_conversation(&test_conversation(
            "existing",
            Provider::ClaudeCode,
            root.join("missing.jsonl"),
            "durable sentinel",
        ))
        .expect("seed conversation");
    writer.commit_writer().expect("commit seed");
    drop(writer);
    fs::remove_dir(&root).expect("remove configured root");

    let mut writer = Storage::open_writer(&database).expect("configured writer");
    let error = index_provider(
        &mut writer,
        ProviderScan {
            provider: Provider::ClaudeCode,
            roots: std::slice::from_ref(&root),
            cutoff_modified_ns: None,
            parse: parse_claude,
            authoritative: true,
        },
        discovery,
        &mut empty_summary(),
        &mut IndexProgress::new(),
    )
    .expect_err("disappearing configured root");

    assert_eq!(error.error.kind, crate::ErrorKind::Configuration);
    assert_eq!(writer.counts().expect("counts").conversations, 1);
    writer.commit_writer().expect("commit unchanged state");
    assert_eq!(writer.search("durable", 10, None, None).unwrap().len(), 1);
}

#[test]
fn late_configured_source_failure_rolls_back_provider_changes() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database = directory.path().join("mash.sqlite3");
    let old_source = directory.path().join("old.jsonl");
    let mut writer = Storage::open_writer(&database).expect("seed writer");
    for (id, content) in [("existing", "durable sentinel"), ("forgotten", "forgotten")] {
        writer
            .replace_conversation(&test_conversation(
                id,
                Provider::ClaudeCode,
                if id == "existing" {
                    old_source.clone()
                } else {
                    directory.path().join("forgotten.jsonl")
                },
                content,
            ))
            .expect("seed conversation");
    }
    writer
        .record_source_checkpoint(
            crate::provider::Provider::ClaudeCode,
            old_source.to_str().unwrap(),
            10,
            20,
        )
        .expect("seed checkpoint");
    writer
        .replace_embeddings(
            "test-generation",
            &[crate::storage::EmbeddingWrite {
                message_id: "existing-message",
                vector: &[127],
                norm: 127.0,
            }],
        )
        .expect("seed embedding");
    writer.commit_writer().expect("commit seed");
    drop(writer);
    let mut storage = Storage::open(&database).expect("forget storage");
    assert!(storage.forget("forgotten").expect("seed tombstone"));
    drop(storage);

    let root = directory.path().join("configured");
    fs::create_dir(&root).expect("configured root");
    let valid = root.join("valid.jsonl");
    let failure = root.join("failure.jsonl");
    write_claude_session(&valid, "new-session", "uncommitted sentinel");
    fs::write(&failure, "placeholder\n").expect("failure fixture");
    let discovery = discover_jsonl_files(std::slice::from_ref(&root), true)
        .expect("initial configured discovery");
    let mut writer = Storage::open_writer(&database).expect("configured writer");
    let mut summary = empty_summary();
    let error = index_provider(
        &mut writer,
        ProviderScan {
            provider: Provider::ClaudeCode,
            roots: std::slice::from_ref(&root),
            cutoff_modified_ns: None,
            parse: parse_with_late_failure,
            authoritative: true,
        },
        discovery,
        &mut summary,
        &mut IndexProgress::new(),
    )
    .expect_err("late source failure");

    assert_eq!(error.error.kind, crate::ErrorKind::Configuration);
    assert_eq!(summary.committed_batches, 0);
    assert_eq!(writer.counts().expect("counts").conversations, 1);
    assert_eq!(writer.embedding_count("test-generation").unwrap(), 1);
    assert!(
        writer
            .source_checkpoint_matches(
                crate::provider::Provider::ClaudeCode,
                old_source.to_str().unwrap(),
                10,
                20
            )
            .unwrap()
    );
    assert_eq!(writer.search("durable", 10, None, None).unwrap().len(), 1);
    assert!(
        writer
            .search("uncommitted", 10, None, None)
            .unwrap()
            .is_empty()
    );
    let tombstone = writer
        .replace_conversation(&test_conversation(
            "forgotten",
            Provider::ClaudeCode,
            directory.path().join("forgotten.jsonl"),
            "forgotten",
        ))
        .expect("tombstone remains");
    assert!(tombstone.tombstoned);
}

#[test]
fn horizon_admits_the_boundary_and_preserves_deleted_old_sources() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path().join("claude");
    fs::create_dir(&root).expect("Claude root");
    let old_path = root.join("old.jsonl");
    let recent_path = root.join("recent.jsonl");
    write_claude_session(&old_path, "old-session", "old sentinel");
    fs::write(
        &recent_path,
        concat!(
            "{\"type\":\"user\",\"sessionId\":\"recent-session\",\"uuid\":\"recent-1\",\"timestamp\":\"2020-01-01T00:00:00Z\",\"message\":{\"role\":\"user\",\"content\":\"recent sentinel\"}}\n",
            "{\"type\":\"assistant\",\"sessionId\":\"recent-session\",\"uuid\":\"recent-2\",\"timestamp\":\"2020-01-02T00:00:00Z\",\"message\":{\"role\":\"assistant\",\"content\":\"complete conversation sentinel\"}}\n"
        ),
    )
    .expect("boundary fixture");
    let run_started = UNIX_EPOCH + Duration::from_hours(200 * 24);
    let boundary = run_started - Duration::from_hours(90 * 24);
    set_modified(&old_path, boundary - Duration::from_nanos(1));
    set_modified(&recent_path, boundary);
    let database = directory.path().join("mash.sqlite3");
    let bounded = IndexOptions {
        providers: [(Provider::ClaudeCode, vec![root.clone()])].into(),
        roots_are_authoritative: true,
        since_days: Some(90),
    };
    let mut writer = Storage::open_writer(&database).expect("bounded writer");
    let admitted = index_at(&mut writer, &bounded, run_started).expect("bounded index");
    assert_eq!(admitted.scanned_files, 1);
    assert_eq!(writer.counts().expect("bounded counts").conversations, 1);
    assert_eq!(writer.counts().expect("bounded counts").messages, 2);
    writer.commit_writer().expect("commit bounded index");
    assert_eq!(writer.search("complete", 10, None, None).unwrap().len(), 1);
    assert!(writer.search("old", 10, None, None).unwrap().is_empty());
    drop(writer);

    let all_history = IndexOptions {
        since_days: None,
        ..bounded
    };
    let mut writer = Storage::open_writer(&database).expect("all-history writer");
    let archival = index_at(&mut writer, &all_history, run_started).expect("all-history index");
    assert_eq!(archival.scanned_files, 2);
    assert_eq!(
        writer.counts().expect("all-history counts").conversations,
        2
    );
    let old_message_id = stable_id(
        "message",
        &["claude-code", &old_path.to_string_lossy(), "old-session-m1"],
    );
    writer
        .replace_embeddings(
            "old-generation",
            &[crate::storage::EmbeddingWrite {
                message_id: &old_message_id,
                vector: &[127],
                norm: 127.0,
            }],
        )
        .expect("old embedding");
    let (old_size_bytes, old_modified_ns) = source_stamp(&old_path).expect("old source stamp");
    writer.commit_writer().expect("commit all-history index");
    drop(writer);

    fs::remove_file(&old_path).expect("remove old source");
    fs::remove_file(&recent_path).expect("remove recent source");
    let bounded = IndexOptions {
        since_days: Some(90),
        ..all_history
    };
    let mut writer = Storage::open_writer(&database).expect("bounded writer");
    let refresh = index_at(&mut writer, &bounded, run_started).expect("bounded refresh");

    assert_eq!(refresh.purged_conversations, 1);
    assert_eq!(writer.counts().expect("counts").conversations, 1);
    assert_eq!(writer.embedding_count("old-generation").unwrap(), 1);
    assert!(
        writer
            .source_checkpoint_matches(
                crate::provider::Provider::ClaudeCode,
                old_path.to_str().unwrap(),
                old_size_bytes,
                old_modified_ns,
            )
            .expect("old checkpoint")
    );
    writer.commit_writer().expect("commit bounded refresh");
    assert_eq!(writer.search("old", 10, None, None).unwrap().len(), 1);
    assert!(writer.search("recent", 10, None, None).unwrap().is_empty());
}

#[test]
fn selecting_one_provider_never_reconciles_an_unselected_provider() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path().join("codex");
    fs::create_dir(&root).expect("Codex root");
    let database = directory.path().join("mash.sqlite3");
    let mut writer = Storage::open_writer(&database).expect("writer");
    for (id, provider, source_path) in [
        (
            "claude-session",
            Provider::ClaudeCode,
            directory.path().join("claude.jsonl"),
        ),
        ("codex-session", Provider::Codex, root.join("missing.jsonl")),
        (
            "claude-forgotten",
            Provider::ClaudeCode,
            directory.path().join("claude-forgotten.jsonl"),
        ),
    ] {
        writer
            .replace_conversation(&test_conversation(
                id,
                provider,
                source_path,
                &format!("{provider} sentinel"),
            ))
            .expect("seed conversation");
    }
    writer
        .record_source_checkpoint(
            crate::provider::Provider::ClaudeCode,
            directory.path().join("claude.jsonl").to_str().unwrap(),
            10,
            20,
        )
        .expect("Claude checkpoint");
    writer
        .replace_embeddings(
            "claude-generation",
            &[crate::storage::EmbeddingWrite {
                message_id: "claude-session-message",
                vector: &[127],
                norm: 127.0,
            }],
        )
        .expect("Claude embedding");
    writer.commit_writer().expect("commit seed");
    drop(writer);
    let mut storage = Storage::open(&database).expect("tombstone storage");
    assert!(
        storage
            .forget("claude-forgotten")
            .expect("Claude tombstone")
    );
    drop(storage);

    let mut writer = Storage::open_writer(&database).expect("refresh writer");
    let summary = index(
        &mut writer,
        &IndexOptions {
            providers: [(Provider::Codex, vec![root])].into(),
            roots_are_authoritative: true,
            since_days: None,
        },
    )
    .expect("Codex-only refresh");

    assert_eq!(summary.purged_conversations, 1);
    assert_eq!(writer.counts().expect("counts").conversations, 1);
    assert_eq!(writer.embedding_count("claude-generation").unwrap(), 1);
    assert!(
        writer
            .source_checkpoint_matches(
                crate::provider::Provider::ClaudeCode,
                directory.path().join("claude.jsonl").to_str().unwrap(),
                10,
                20,
            )
            .unwrap()
    );
    assert_eq!(
        writer
            .search("claude", 10, None, None)
            .expect("Claude remains")
            .len(),
        1
    );
    let tombstone = writer
        .replace_conversation(&test_conversation(
            "claude-forgotten",
            Provider::ClaudeCode,
            directory.path().join("claude-forgotten.jsonl"),
            "forgotten",
        ))
        .expect("Claude tombstone remains");
    assert!(tombstone.tombstoned);
}
