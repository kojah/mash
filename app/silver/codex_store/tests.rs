use super::*;
use crate::silver::{IndexOptions, index};
use crate::storage::Storage;
use proptest::prelude::*;

fn fixture(root: &Path) -> (PathBuf, Connection, Connection) {
    let path = root.join("thread_history_1.sqlite");
    let history = Connection::open(&path).unwrap();
    history.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
        CREATE TABLE thread_items(thread_id TEXT,turn_id TEXT,item_id TEXT,rollout_ordinal INTEGER,created_at_ms INTEGER,item_json TEXT);
        CREATE TABLE thread_realtime_items(thread_id TEXT,item_id TEXT,rollout_ordinal INTEGER,created_at_ms INTEGER,item_json TEXT);").unwrap();
    let state = Connection::open(state_path(&path)).unwrap();
    state.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
        CREATE TABLE threads(id TEXT PRIMARY KEY,title TEXT,cwd TEXT,git_origin_url TEXT,
        created_at INTEGER,updated_at INTEGER,created_at_ms INTEGER,updated_at_ms INTEGER,rollout_path TEXT,history_mode TEXT);").unwrap();
    state.execute("INSERT INTO threads VALUES('alpha','SQLite history','/repo','https://example.test/repo',1,2,1000,2000,?1,'paginated')",
        [root.join("rollout.jsonl").to_str().unwrap()]).unwrap();
    for (ordinal, value) in [
        serde_json::json!({"type":"userMessage","id":"u","content":[{"type":"text","text":"visiblequartz"}]}),
        serde_json::json!({"type":"agentMessage","id":"a","text":"visibleamber"}),
        serde_json::json!({"type":"reasoning","id":"r","summary":["hiddenreasoning"],"content":[]}),
        serde_json::json!({"type":"mcpToolCall","id":"t","server":"mash","tool":"view","status":"completed","arguments":{"id":"example"},"result":{"content":[{"type":"text","text":"hiddentool"}]}}),
    ].into_iter().enumerate() {
        history.execute("INSERT INTO thread_items VALUES('alpha','turn',?1,?2,2000,?3)",
            rusqlite::params![value["id"].as_str().unwrap(),i64::try_from(ordinal).unwrap(),serde_json::to_string(&value).unwrap()]).unwrap();
    }
    (path, history, state)
}

fn options(path: &Path) -> IndexOptions {
    IndexOptions {
        providers: [(Provider::Codex, vec![path.into()])].into(),
        roots_are_authoritative: true,
        since_days: None,
    }
}

#[test]
fn codex_paginated_large_file_change_preserves_tool_context_and_dialogue() {
    let directory = tempfile::tempdir().unwrap();
    let (path, history, _state) = fixture(directory.path());
    let diff = "largepatchcanary\n".repeat(600_000);
    let item = serde_json::json!({
        "id":"patch", "type":"fileChange", "status":"completed",
        "changes":[{"path":"example.rs", "diff":diff}]
    });
    let encoded = serde_json::to_string(&item).unwrap();
    assert!(encoded.len() > super::super::MAX_RECORD_BYTES);
    history
        .execute(
            "INSERT INTO thread_items VALUES('alpha','turn','patch',4,2000,?1)",
            [encoded],
        )
        .unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    index(&mut storage, &options(&path)).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 5);
    assert_eq!(
        crate::storage::test_support::projections_with_text(&storage, "visiblequartz", None).len(),
        1
    );
    assert!(
        crate::storage::test_support::projections_with_text(&storage, "largepatchcanary", None)
            .is_empty()
    );
    let source = sources(vec![HistorySource::file(&path)]).unwrap().remove(0);
    let parsed = parse(&source.identity).unwrap().chat.unwrap();
    assert!(matches!(
        parsed.messages,
        super::super::messages::Messages::Staged(_)
    ));
    let message = parsed.messages.rows().unwrap().nth(4).unwrap().unwrap();
    let retained: serde_json::Value = serde_json::from_str(&message.content).unwrap();
    assert_eq!(retained["changes"][0]["diff"], diff);
}

#[test]
fn codex_paginated_item_limit_rejects_before_materializing_payload() {
    let connection = Connection::open_in_memory().unwrap();
    // No `data` column exists: oversized rows must fail before trying to read it.
    let mut statement = connection.prepare("SELECT ?1 AS bytes").unwrap();
    let mut rows = statement
        .query([i64::try_from(MAX_ITEM_BYTES + 1).unwrap()])
        .unwrap();
    let error = sqlite_source::bounded_json_with_limit::<items::Item>(
        rows.next().unwrap().unwrap(),
        MAX_ITEM_BYTES,
    )
    .err()
    .unwrap();
    assert!(error.error.message.contains("64 MiB ingestion limit"));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]
    #[test]
    fn codex_paginated_repeated_text_does_not_collapse_native_items(texts in prop::collection::vec("[a-z ]{1,40}", 1..20)) {
        let directory=tempfile::tempdir().unwrap();
        let (path,history,_state)=fixture(directory.path());
        history.execute("DELETE FROM thread_items",[]).unwrap();
        for (ordinal,text) in texts.iter().enumerate() {
            let id=format!("item-{ordinal}");
            let json=serde_json::to_string(&serde_json::json!({"id":id,"type":"agentMessage","text":text})).unwrap();
            history.execute("INSERT INTO thread_items VALUES('alpha','turn',?1,?2,2000,?3)",
                rusqlite::params![id,i64::try_from(ordinal).unwrap(),json]).unwrap();
        }
        let source=sources(vec![HistorySource::file(&path)]).unwrap().remove(0);
        let parsed=parse(&source.identity).unwrap().chat.unwrap();
        let messages=parsed.messages.inline();
        prop_assert_eq!(messages.len(),texts.len());
        let ids=messages.iter().map(|m| m.id.clone()).collect::<BTreeSet<_>>();
        prop_assert_eq!(ids.len(),texts.len());
        for (message,text) in messages.iter().zip(&texts) {
            prop_assert_eq!(&message.content,text);
        }
        history.execute("UPDATE thread_items SET rollout_ordinal=-rollout_ordinal",[]).unwrap();
        let reordered=parse(&source.identity).unwrap().chat.unwrap();
        let reordered_ids=reordered.messages.inline().iter().map(|m| m.id.clone()).collect::<BTreeSet<_>>();
        prop_assert_eq!(ids,reordered_ids);
    }
}

#[test]
fn codex_paginated_replaces_jsonl_mirror_without_purging_same_native_chat() {
    let directory = tempfile::tempdir().unwrap();
    let (_path, _history, _state) = fixture(directory.path());
    let mirror = directory.path().join("rollout.jsonl");
    std::fs::write(&mirror,concat!(
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"alpha\",\"cwd\":\"/repo\"}}\n",
        "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"legacyzircon\"}]}}\n"
    )).unwrap();
    let database = directory.path().join("index.sqlite3");
    let mut writer = Storage::open_writer(&database).unwrap();
    index(&mut writer, &options(&mirror)).unwrap();
    writer.commit_writer().unwrap();
    assert_eq!(writer.counts().unwrap().chats, 1);
    drop(writer);
    let mut writer = Storage::open_writer(&database).unwrap();
    index(&mut writer, &options(directory.path())).unwrap();
    writer.commit_writer().unwrap();
    assert_eq!(writer.counts().unwrap().chats, 1);
    assert_eq!(
        crate::storage::test_support::projections_with_text(
            &writer,
            "visiblequartz",
            Some(Provider::Codex)
        )
        .len(),
        1
    );
    assert_eq!(
        crate::storage::test_support::projections_with_text(
            &writer,
            "legacyzircon",
            Some(Provider::Codex)
        )
        .len(),
        0
    );
}

#[test]
fn codex_paginated_history_reads_wal_preserves_identity_and_hides_context() {
    let directory = tempfile::tempdir().unwrap();
    let (path, history, state) = fixture(directory.path());
    let sources = sources(vec![HistorySource::file(&path)]).unwrap();
    assert_eq!(sources.len(), 1);
    let stamp = sources[0].stamp().unwrap();
    let parsed = parse(&sources[0].identity).unwrap().chat.unwrap();
    assert_eq!(parsed.id, "alpha");
    assert_eq!(parsed.title.as_deref(), Some("SQLite history"));
    assert_eq!(parsed.created_at, Some(1000));
    let messages = parsed.messages.inline();
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[0].dialogue(), "visiblequartz");
    assert_eq!(messages[1].dialogue(), "visibleamber");
    assert_eq!(messages[2].dialogue(), "");
    assert!(messages[3].content.contains("hiddentool"));
    assert_eq!(messages[3].dialogue(), "");
    assert_eq!(
        messages[3].tools.tool_calls[0].name.as_deref(),
        Some("view")
    );
    assert!(
        sqlite_source::open(&path)
            .unwrap()
            .execute("DELETE FROM thread_items", [])
            .is_err()
    );
    assert!(stamp.verify(&sources[0].identity).is_ok());
    let state_main = std::fs::read(state_path(&path)).unwrap();
    state
        .execute("UPDATE threads SET title='changed title'", [])
        .unwrap();
    assert_eq!(std::fs::read(state_path(&path)).unwrap(), state_main);
    assert!(stamp.verify(&sources[0].identity).is_err());
    assert_eq!(
        parse(&sources[0].identity)
            .unwrap()
            .chat
            .unwrap()
            .messages
            .inline()[0]
            .id,
        messages[0].id
    );
    let stamp = sources[0].stamp().unwrap();
    history
        .execute("UPDATE thread_items SET created_at_ms=2001", [])
        .unwrap();
    assert!(stamp.verify(&sources[0].identity).is_err());
}

#[test]
fn codex_paginated_discovery_prefers_metadata_named_mirror_and_preserves_legacy_files() {
    let directory = tempfile::tempdir().unwrap();
    let (path, _history, state) = fixture(directory.path());
    let mirror = directory.path().join("rollout.jsonl");
    let legacy = directory.path().join("legacy.jsonl");
    let physical = || {
        vec![
            HistorySource::file(&path),
            HistorySource::file(&mirror),
            HistorySource::file(&legacy),
        ]
    };
    let found = sources(physical()).unwrap();
    assert_eq!(found.len(), 2);
    assert!(found.iter().any(|s| s.identity.path == legacy));
    assert!(!found.iter().any(|s| s.identity.path == mirror));
    state
        .execute("UPDATE threads SET history_mode='legacy'", [])
        .unwrap();
    let found = sources(physical()).unwrap();
    assert_eq!(found.len(), 2);
    assert!(found.iter().all(|s| s.identity.session.is_none()));
    state
        .execute("UPDATE threads SET history_mode='future-layout'", [])
        .unwrap();
    assert!(sources(physical()).is_err());
}

#[test]
fn codex_paginated_index_refresh_and_rejection_keep_committed_history() {
    let directory = tempfile::tempdir().unwrap();
    let (path, history, _state) = fixture(directory.path());
    let database = directory.path().join("index.sqlite3");
    let mut writer = Storage::open_writer(&database).unwrap();
    index(&mut writer, &options(&path)).unwrap();
    writer.commit_writer().unwrap();
    assert_eq!(writer.counts().unwrap().chats, 1);
    assert_eq!(
        crate::storage::test_support::projections_with_text(
            &writer,
            "visiblequartz",
            Some(Provider::Codex)
        )
        .len(),
        1
    );
    assert!(
        crate::storage::test_support::projections_with_text(
            &writer,
            "hiddenreasoning",
            Some(Provider::Codex)
        )
        .is_empty()
    );
    drop(writer);
    let mut writer = Storage::open_writer(&database).unwrap();
    assert_eq!(
        index(&mut writer, &options(&path))
            .unwrap()
            .checkpoint_skipped_sources,
        1
    );
    writer.commit_writer().unwrap();
    drop(writer);
    history
        .execute(
            "UPDATE thread_items SET item_json='{}' WHERE item_id='u'",
            [],
        )
        .unwrap();
    let mut writer = Storage::open_writer(&database).unwrap();
    assert!(index(&mut writer, &options(&path)).is_err());
    drop(writer);
    let writer = Storage::open_writer(&database).unwrap();
    assert_eq!(
        crate::storage::test_support::projections_with_text(
            &writer,
            "visiblequartz",
            Some(Provider::Codex)
        )
        .len(),
        1
    );
}

#[test]
fn codex_paginated_rejects_unknown_items_and_mismatched_identity() {
    let directory = tempfile::tempdir().unwrap();
    let (path, history, _state) = fixture(directory.path());
    let source = sources(vec![HistorySource::file(&path)]).unwrap().remove(0);
    for json in [
        r#"{"type":"futureDialogue","id":"u"}"#,
        r#"{"type":"agentMessage","id":"other","text":"lost"}"#,
    ] {
        history
            .execute(
                "UPDATE thread_items SET item_json=?1 WHERE item_id='u'",
                [json],
            )
            .unwrap();
        assert!(parse(&source.identity).is_err());
    }
    history.execute("DELETE FROM thread_items", []).unwrap();
    history
        .execute(
            "INSERT INTO thread_realtime_items VALUES('alpha','future',1,2000,'{}')",
            [],
        )
        .unwrap();
    assert!(parse(&source.identity).is_err());
}

#[test]
fn codex_paginated_realtime_transcripts_interleave_without_repeating_promoted_items() {
    let directory = tempfile::tempdir().unwrap();
    let (path, history, _state) = fixture(directory.path());
    for (ordinal, item) in [
        (
            -1,
            serde_json::json!({"id":"start","type":"realtime_session_started","realtime_session_id":"voice"}),
        ),
        (
            4,
            serde_json::json!({"id":"u","type":"transcript_segment","realtime_session_id":"voice","role":"user","text":"spoken tourmaline"}),
        ),
        (
            5,
            serde_json::json!({"id":"promoted","type":"bem_item_promoted","realtime_session_id":"voice","turn_id":"turn","item_id":"t","presentation":{"type":"inline_markdown"}}),
        ),
        (
            6,
            serde_json::json!({"id":"end","type":"realtime_session_closed","realtime_session_id":"voice","outcome":"ended"}),
        ),
    ] {
        history
            .execute(
                "INSERT INTO thread_realtime_items VALUES('alpha',?1,?2,2000,?3)",
                rusqlite::params![
                    item["id"].as_str().unwrap(),
                    ordinal,
                    serde_json::to_string(&item).unwrap()
                ],
            )
            .unwrap();
    }
    let source = sources(vec![HistorySource::file(path)]).unwrap().remove(0);
    let parsed = parse(&source.identity).unwrap().chat.unwrap();
    let messages = parsed.messages.inline();
    assert_eq!(messages.len(), 8);
    assert_eq!(messages[0].dialogue(), "");
    assert_eq!(messages[5].dialogue(), "spoken tourmaline");
    assert_ne!(messages[1].id, messages[5].id);
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.content.contains("hiddentool"))
            .count(),
        1
    );
    assert_eq!(messages[6].dialogue(), "");
    assert_eq!(messages[7].dialogue(), "");
}
