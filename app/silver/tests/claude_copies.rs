use super::*;
use crate::message_context::MessageContext;

mod handoff;
mod lifecycle;
mod metadata;
mod replay;
mod sessions;

fn transcript(path: &Path, rows: &[(&str, Option<&str>, &str)]) {
    let records = rows
        .iter()
        .map(|(uuid, parent, text)| {
            serde_json::json!({
                "type": "user", "sessionId": "copied-thread", "uuid": uuid,
                "parentUuid": parent, "message": {"role": "user", "content": text},
            })
            .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(path, records).unwrap();
}

fn options(root: &Path) -> IndexOptions {
    IndexOptions {
        providers: [(Provider::ClaudeCode, vec![root.to_owned()])].into(),
        roots_are_authoritative: true,
        since_days: None,
    }
}

#[test]
fn claude_copies_preserve_branches_deduplicate_and_remove_only_source_contributions() {
    for tails in [(false, false), (false, true), (true, true)] {
        for reverse in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let paths = [
                directory.path().join("a.jsonl"),
                directory.path().join("b.jsonl"),
            ];
            for (index, path) in paths.iter().enumerate() {
                let mut rows = vec![("common", None, "common sentinel")];
                if [tails.0, tails.1][index] {
                    rows.push((
                        if index == 0 { "left" } else { "right" },
                        Some("common"),
                        if index == 0 {
                            "left sentinel"
                        } else {
                            "right sentinel"
                        },
                    ));
                }
                transcript(path, &rows);
            }
            let parsed = paths
                .iter()
                .map(|path| parse_claude(path).unwrap().chat.unwrap())
                .collect::<Vec<_>>();
            let common = stable_id("claude-message", &["copied-thread", "common"]);
            assert_eq!(parsed[0].messages.inline()[0].id, common);
            assert_eq!(parsed[1].messages.inline()[0].id, common);
            let database = directory.path().join("index.sqlite3");
            let mut storage = Storage::open_writer(&database).unwrap();
            for index in if reverse { [1, 0] } else { [0, 1] } {
                storage.replace_chat(&parsed[index]).unwrap();
            }
            let hits =
                crate::storage::test_support::projections_with_text(&storage, "sentinel", None);
            assert_eq!(hits.len(), 1 + usize::from(tails.0) + usize::from(tails.1));
            for hit in &hits {
                if hit.id != common {
                    let context = storage.view(&hit.id, 10).unwrap();
                    assert!(context.iter().any(|message| message.id == common));
                    assert_eq!(context.len(), 2, "sibling branch must not become context");
                }
            }
            storage.commit_writer().unwrap();
            drop(storage);
            let mut storage = Storage::open_writer(&database).unwrap();
            let first = usize::from(reverse);
            assert!(
                !storage
                    .remove_source(Provider::ClaudeCode, &parsed[first].source)
                    .unwrap()
            );
            storage.commit_writer().unwrap();
            let remaining = 1 + usize::from([tails.0, tails.1][1 - first]);
            assert_eq!(storage.counts().unwrap().messages, remaining as u64);
            assert_eq!(
                storage.view(&common, 0).unwrap()[0].content,
                "common sentinel"
            );
            drop(storage);
            let mut storage = Storage::open_writer(&database).unwrap();
            assert!(
                storage
                    .remove_source(Provider::ClaudeCode, &parsed[1 - first].source)
                    .unwrap()
            );
            storage.commit_writer().unwrap();
            assert_eq!(storage.counts().unwrap().messages, 0);
            assert_eq!(storage.counts().unwrap().chats, 0);
        }
    }
}

#[test]
fn claude_checkpointed_survivor_is_retained_after_owner_deletion_and_restart() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("sources");
    fs::create_dir(&root).unwrap();
    let paths = [root.join("a.jsonl"), root.join("b.jsonl")];
    for path in &paths {
        transcript(path, &[("common", None, "common sentinel")]);
    }
    let database = directory.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&root)).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let owner: String = rusqlite::Connection::open(&database)
        .unwrap()
        .query_row("SELECT source_path FROM chats", [], |row| row.get(0))
        .unwrap();
    fs::remove_file(owner).unwrap();
    for _ in 0..2 {
        let mut storage = Storage::open_writer(&database).unwrap();
        let summary = index(&mut storage, &options(&root)).unwrap();
        assert_eq!(summary.checkpoint_skipped_sources, 1);
        storage.commit_writer().unwrap();
        assert_eq!(storage.counts().unwrap().chats, 1);
        assert_eq!(
            crate::storage::test_support::projections_with_text(&storage, "common", None).len(),
            1
        );
    }
}

#[test]
fn claude_rewinds_empty_files_and_identity_moves_preserve_other_copies() {
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
    let database = directory.path().join("index.sqlite3");
    for expected in [2, 1, 2, 1, 2] {
        let mut storage = Storage::open_writer(&database).unwrap();
        index(&mut storage, &options(&root)).unwrap();
        storage.commit_writer().unwrap();
        assert_eq!(storage.counts().unwrap().messages, expected);
        drop(storage);
        match expected {
            2 if fs::read_to_string(&b).unwrap().contains("tail sentinel") => {
                transcript(&b, &[("common", None, "common sentinel")]);
            }
            1 if !fs::read_to_string(&b).unwrap().is_empty() => transcript(
                &b,
                &[
                    ("common", None, "common sentinel"),
                    ("tail", Some("common"), "tail sentinel"),
                ],
            ),
            _ => {}
        }
    }
    fs::write(&b, "").unwrap();
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&root)).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 1);
    drop(storage);
    transcript(&b, &[("common", None, "common sentinel")]);
    let moved = fs::read_to_string(&b)
        .unwrap()
        .replace("copied-thread", "other-thread");
    fs::write(&b, moved).unwrap();
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&root)).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().chats, 2);
    assert!(storage.forget("copied-thread").unwrap());
    drop(storage);
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&root)).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().chats, 1);
}

#[test]
fn claude_conflicting_copy_rolls_back_and_can_retry_without_poisoning_checkpoints() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("sources");
    fs::create_dir(&root).unwrap();
    let a = root.join("a.jsonl");
    let b = root.join("b.jsonl");
    transcript(&a, &[("common", None, "common sentinel")]);
    let database = directory.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&root)).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    transcript(&b, &[("common", None, "conflicting sentinel")]);
    let mut storage = Storage::open_writer(&database).unwrap();
    let error = index(&mut storage, &options(&root)).err().unwrap();
    assert_eq!(error.error.kind, crate::ErrorKind::Configuration);
    storage.commit_writer().unwrap();
    assert_eq!(
        crate::storage::test_support::projections_with_text(&storage, "common", None).len(),
        1
    );
    assert!(
        crate::storage::test_support::projections_with_text(&storage, "conflicting", None)
            .is_empty()
    );
    assert!(
        !storage
            .source_checkpoint_matches(
                Provider::ClaudeCode,
                &SourceIdentity::file(&b),
                &source_stamp(&b).unwrap()
            )
            .unwrap()
    );
    drop(storage);
    transcript(
        &b,
        &[
            ("common", None, "common sentinel"),
            ("new", Some("common"), "new sentinel"),
        ],
    );
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&root)).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 2);
}

#[test]
fn claude_native_ancestry_resolves_intermediate_events_and_rejects_cycles() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tree.jsonl");
    transcript(
        &path,
        &[
            ("root", None, "root sentinel"),
            ("child", Some("progress"), "child sentinel"),
        ],
    );
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"\n{\"type\":\"progress\",\"uuid\":\"progress\",\"parentUuid\":\"root\"}\n")
        .unwrap();
    let parsed = parse_claude(&path).unwrap().chat.unwrap();
    assert_eq!(
        parsed.messages.inline()[1].context,
        MessageContext::Parent {
            id: parsed.messages.inline()[0].id.clone()
        }
    );
    transcript(
        &path,
        &[
            ("a", Some("b"), "a sentinel"),
            ("b", Some("a"), "b sentinel"),
        ],
    );
    assert!(parse_claude(&path).is_err());
}

#[test]
fn claude_ancestry_can_be_supplied_by_another_source_and_survives_missing_ancestors() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root.jsonl");
    let child = directory.path().join("child.jsonl");
    transcript(&root, &[("root", None, "root sentinel")]);
    transcript(&child, &[("child", Some("root"), "child sentinel")]);
    let child = parse_claude(&child).unwrap().chat.unwrap();
    let root = parse_claude(&root).unwrap().chat.unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    storage.replace_chat(&child).unwrap();
    storage.replace_chat(&root).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(
        storage
            .view(&child.messages.inline()[0].id, 1)
            .unwrap()
            .len(),
        2
    );
    drop(storage);
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    assert!(
        !storage
            .remove_source(Provider::ClaudeCode, &root.source)
            .unwrap()
    );
    storage.commit_writer().unwrap();
    let messages = storage.view(&child.messages.inline()[0].id, 1).unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, "child sentinel");
}
