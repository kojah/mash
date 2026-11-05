//! Native refusal metadata may be repeated with a new parent but no dialogue children.
use super::*;

fn events(path: &Path, rows: &[(&str, Option<&str>)]) {
    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
    writeln!(file).unwrap();
    for (uuid, parent) in rows {
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "type": "system", "subtype": "model_refusal_fallback",
                "sessionId": "copied-thread", "uuid": uuid, "parentUuid": parent,
            })
        )
        .unwrap();
    }
}

#[test]
fn claude_unreferenced_metadata_reparenting_preserves_dialogue_and_checkpoint_readiness() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("sources");
    fs::create_dir(&root).unwrap();
    let path = root.join("native.jsonl");
    transcript(
        &path,
        &[
            ("root", None, "root sentinel"),
            ("child", Some("root"), "child sentinel"),
        ],
    );
    events(
        &path,
        &[("fallback", Some("root")), ("fallback", Some("child"))],
    );
    let database = directory.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    let summary = index(&mut storage, &options(&root)).unwrap();
    assert_eq!(summary.malformed_records, 0);
    assert_eq!(summary.changed_messages, 2);
    storage.commit_writer().unwrap();
    let child = stable_id("claude-message", &["copied-thread", "child"]);
    let context = storage.view(&child, 10).unwrap();
    assert_eq!(context.len(), 2);
    assert_eq!(
        context[0].id,
        stable_id("claude-message", &["copied-thread", "root"])
    );
    assert_eq!(context[1].id, child);
    drop(storage);
    let mut storage = Storage::open_writer(&database).unwrap();
    assert_eq!(
        index(&mut storage, &options(&root))
            .unwrap()
            .checkpoint_skipped_sources,
        1
    );
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 2);
}

#[test]
fn claude_referenced_metadata_conflicts_and_dialogue_conflicts_still_fail_closed() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("native.jsonl");
    for parent in ["fallback", "relay"] {
        transcript(
            &path,
            &[
                ("root", None, "root sentinel"),
                ("child", Some(parent), "child sentinel"),
            ],
        );
        events(
            &path,
            &[
                ("fallback", Some("root")),
                ("fallback", None),
                ("fallback", Some("root")),
                ("relay", Some("fallback")),
            ],
        );
        let error = parse_claude(&path).err().unwrap();
        assert_eq!(error.error.kind, crate::ErrorKind::Configuration);
        assert!(error.error.message.contains("conflicting ancestry"));
        assert!(error.error.message.contains("fallback"));
    }
    transcript(
        &path,
        &[
            ("root", None, "root sentinel"),
            ("child", Some("root"), "child sentinel"),
            ("child", None, "child sentinel"),
        ],
    );
    assert!(
        parse_claude(&path)
            .err()
            .unwrap()
            .error
            .message
            .contains("conflicting ancestry")
    );
}

#[test]
fn claude_repeated_consistent_metadata_still_bridges_dialogue_ancestry() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("native.jsonl");
    transcript(
        &path,
        &[
            ("root", None, "root sentinel"),
            ("child", Some("fallback"), "child sentinel"),
        ],
    );
    events(
        &path,
        &[("fallback", Some("root")), ("fallback", Some("root"))],
    );
    let conversation = parse_claude(&path).unwrap().conversation.unwrap();
    assert_eq!(conversation.messages.len(), 2);
    assert_eq!(
        conversation.messages.inline()[1].context,
        MessageContext::Parent {
            id: conversation.messages.inline()[0].id.clone()
        }
    );
}
