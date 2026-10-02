//! Sanitized native shape: reduced duplicate history preceding compaction.
use super::*;

fn append_replay(path: &Path, logical: Option<&str>, tail: &str, interrupt: Option<&str>) {
    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
    writeln!(file).unwrap();
    for (uuid, parent) in [("root", None), ("answer", Some("root"))] {
        writeln!(file, "{}", serde_json::json!({
            "type": "user", "sessionId": "copied-thread", "uuid": uuid,
            "parentUuid": parent, "message": {"role":"user", "content":format!("{uuid} sentinel")},
        })).unwrap();
    }
    if let Some(interrupt) = interrupt {
        writeln!(file, "{interrupt}").unwrap();
    }
    writeln!(file, "{}", serde_json::json!({
        "type":"system", "subtype":"compact_boundary", "uuid":"boundary",
        "sessionId":"copied-thread", "parentUuid":null,
        "logicalParentUuid":logical,
        "compactMetadata":{"preservedSegment":{"headUuid":"answer", "tailUuid":tail, "anchorUuid":"summary"}},
    })).unwrap();
    writeln!(
        file,
        "{}",
        serde_json::json!({
            "type":"user", "uuid":"summary", "sessionId":"copied-thread", "parentUuid":"boundary",
            "message":{"role":"user", "content":"summary sentinel"},
        })
    )
    .unwrap();
}

fn original(path: &Path) {
    transcript(
        path,
        &[
            ("root", None, "root sentinel"),
            ("omitted", Some("root"), "omitted sentinel"),
            ("answer", Some("omitted"), "answer sentinel"),
            ("tail", Some("answer"), "tail sentinel"),
        ],
    );
}

#[test]
fn spilled_native_ancestry_preserves_compaction_context_and_restart_checkpoint() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("sources");
    fs::create_dir(&root).unwrap();
    let current = root.join("current.jsonl");
    original(&current);
    // More than the production inline budget; keep the real native replay
    // linked to operational ancestry, rather than injecting a test-only store.
    let mut file = fs::OpenOptions::new().append(true).open(&current).unwrap();
    let mut parent = "tail".to_owned();
    writeln!(file).unwrap();
    for index in 0..20_000 {
        let uuid = format!("operational-{index}");
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "type":"progress", "uuid":uuid, "parentUuid":parent,
                "sessionId":"copied-thread",
            })
        )
        .unwrap();
        parent = uuid;
    }
    drop(file);
    append_replay(&current, Some(&parent), &parent, None);
    let database = directory.path().join("index.sqlite3");
    let tail = stable_id("claude-message", &["copied-thread", "tail"]);
    for restart in [false, true] {
        let mut storage = Storage::open_writer(&database).unwrap();
        let summary = index(&mut storage, &options(&root)).unwrap();
        assert_eq!(summary.checkpoint_skipped_sources, u64::from(restart));
        storage.commit_writer().unwrap();
        assert_eq!(storage.counts().unwrap().messages, 5);
        assert_eq!(
            storage
                .view(&tail, 10)
                .unwrap()
                .iter()
                .map(|m| m.content.as_str())
                .collect::<Vec<_>>(),
            [
                "root sentinel",
                "omitted sentinel",
                "answer sentinel",
                "tail sentinel"
            ]
        );
    }
}

#[test]
fn claude_compaction_replay_keeps_archived_context_copies_and_checkpointed_survivors() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("sources");
    fs::create_dir(&root).unwrap();
    let archive = root.join("archive.jsonl");
    let current = root.join("current.jsonl");
    original(&archive);
    fs::copy(&archive, &current).unwrap();
    append_replay(&current, Some("tail"), "tail", None);
    let parsed = parse_claude(&current).unwrap().conversation.unwrap();
    assert_eq!(
        parsed.messages.len(),
        5,
        "replayed payloads are deduplicated"
    );
    let database = directory.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    let summary = index(&mut storage, &options(&root)).unwrap();
    assert_eq!(summary.malformed_records, 0);
    storage.commit_writer().unwrap();
    let tail_id = stable_id("claude-message", &["copied-thread", "tail"]);
    let context = storage.view(&tail_id, 10).unwrap();
    assert_eq!(
        context
            .iter()
            .map(|row| row.content.as_str())
            .collect::<Vec<_>>(),
        [
            "root sentinel",
            "omitted sentinel",
            "answer sentinel",
            "tail sentinel"
        ]
    );
    drop(storage);
    fs::remove_file(&archive).unwrap();
    let mut storage = Storage::open_writer(&database).unwrap();
    assert_eq!(
        index(&mut storage, &options(&root))
            .unwrap()
            .checkpoint_skipped_sources,
        1
    );
    storage.commit_writer().unwrap();
    assert_eq!(storage.view(&tail_id, 10).unwrap().len(), 4);
    assert_eq!(storage.counts().unwrap().messages, 5);
}

#[test]
fn claude_replay_requires_uninterrupted_native_boundary_and_matching_tail() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("current.jsonl");
    for (logical, tail, interrupt) in [
        (None, "tail", None),
        (Some("missing"), "missing", None),
        (Some("tail"), "wrong", None),
        (Some("root"), "root", None),
        (
            Some("tail"),
            "tail",
            Some("{\"type\":\"attachment\",\"uuid\":\"new-event\",\"parentUuid\":\"tail\"}"),
        ),
        (
            Some("tail"),
            "tail",
            Some("{\"type\":\"user\",\"message\":{\"content\":\"unidentified dialogue\"}}"),
        ),
    ] {
        original(&path);
        append_replay(&path, logical, tail, interrupt);
        assert!(
            parse_claude(&path)
                .err()
                .unwrap()
                .error
                .message
                .contains("conflicting ancestry")
        );
    }
    original(&path);
    append_replay(&path, Some("tail"), "tail", Some("{broken"));
    let incomplete = parse_claude(&path).unwrap();
    assert!(
        incomplete.conversation.is_none(),
        "corruption cannot prove replay ancestry"
    );
    assert_eq!(incomplete.malformed_records, 1);
    assert_eq!(
        incomplete.first_record_issue.unwrap().category,
        crate::ingestion::diagnostics::Category::Syntax
    );
    original(&path);
    append_replay(&path, Some("tail"), "tail", None);
    let rows = fs::read_to_string(&path).unwrap();
    // A subtype on an unrelated record is insufficient evidence.
    fs::write(
        &path,
        rows.replace("\"type\":\"system\"", "\"type\":\"attachment\""),
    )
    .unwrap();
    assert!(parse_claude(&path).is_err());
    // A later valid boundary cannot pardon an earlier unrelated conflict.
    transcript(
        &path,
        &[
            ("root", None, "root sentinel"),
            ("omitted", Some("root"), "omitted sentinel"),
            ("answer", Some("omitted"), "answer sentinel"),
            ("answer", None, "answer sentinel"),
            ("tail", Some("answer"), "tail sentinel"),
        ],
    );
    append_replay(&path, Some("tail"), "tail", None);
    assert!(parse_claude(&path).is_err());
}

#[test]
fn claude_proven_replay_payload_conflict_rolls_back_and_retries_cleanly() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("sources");
    fs::create_dir(&root).unwrap();
    let path = root.join("current.jsonl");
    original(&path);
    let database = directory.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&root)).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    append_replay(&path, Some("tail"), "tail", None);
    let valid = fs::read_to_string(&path).unwrap();
    let last = valid.rfind("answer sentinel").unwrap();
    let mut invalid = valid.clone();
    invalid.replace_range(last..last + "answer sentinel".len(), "conflicting sentinel");
    fs::write(&path, invalid).unwrap();
    let mut storage = Storage::open_writer(&database).unwrap();
    let error = index(&mut storage, &options(&root)).err().unwrap();
    assert!(
        error
            .error
            .message
            .contains("conflicting content or ancestry")
    );
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 4);
    assert!(
        !storage
            .source_checkpoint_matches(
                Provider::ClaudeCode,
                &SourceIdentity::file(&path),
                &source_stamp(&path).unwrap()
            )
            .unwrap()
    );
    drop(storage);
    fs::write(&path, valid).unwrap();
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&root)).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 5);
}
