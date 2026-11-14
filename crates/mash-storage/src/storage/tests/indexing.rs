use super::*;

use crate::silver::SourceStamp;
use std::collections::BTreeSet;

#[test]
fn sessions_in_one_database_have_independent_checkpoints_and_reconciliation() {
    use crate::provider::Provider;
    use crate::silver::SourceIdentity;
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("mash.sqlite3");
    let physical = directory.path().join("history.db");
    let mut storage = Storage::open_writer(&database).unwrap();
    let mut sources = Vec::new();
    for (session, path, modified) in [
        ("keep", physical.clone(), 100),
        ("deleted", physical.clone(), 100),
        ("archived", physical.clone(), 1),
        ("outside", PathBuf::from("/outside/history.db"), 100),
    ] {
        let source = SourceIdentity {
            path,
            session: Some(session.into()),
        };
        let mut value = chat(&format!("{session} searchable evidence"));
        // Codex thread times are selected from retained messages, not arbitrary
        // independent fixture dates.
        value.created_at = value.messages.inline()[0].created_at;
        value.updated_at = value.created_at;
        value.id = session.into();
        value.messages.inline_mut()[0].id = format!("{session}-message");
        value.source = source.clone();
        storage.replace_chat(&value).unwrap();
        storage
            .record_source_checkpoint(Provider::Codex, &source, &SourceStamp::file(10, modified))
            .unwrap();
        sources.push(source);
    }
    storage.commit_writer().unwrap();
    drop(storage);
    let mut storage = Storage::open_writer(&database).unwrap();
    assert_eq!(storage.counts().unwrap().chats, 4);
    let paths: Vec<(String, String)> = storage.connection.prepare(
        "SELECT source_path, source_session FROM chats WHERE id IN ('keep','deleted') ORDER BY id"
    ).unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap()
        .collect::<Result<_, _>>().unwrap();
    assert!(
        paths
            .iter()
            .all(|(path, _)| path == &physical.to_string_lossy())
    );
    assert_ne!(paths[0].1, paths[1].1);
    assert!(
        storage
            .source_checkpoint_matches(Provider::Codex, &sources[0], &SourceStamp::file(10, 100))
            .unwrap()
    );
    assert!(
        !storage
            .source_checkpoint_matches(
                Provider::ClaudeCode,
                &sources[0],
                &SourceStamp::file(10, 100)
            )
            .unwrap()
    );

    let observed = [sources[0].key()].into();
    assert_eq!(
        storage
            .purge_missing_sources(Provider::Codex, &observed, &[physical], Some(50))
            .unwrap(),
        1
    );
    storage.commit_writer().unwrap();
    assert!(storage.view("deleted-message", 0).unwrap().is_empty());
    for session in ["keep", "archived", "outside"] {
        assert_eq!(
            storage
                .view(&format!("{session}-message"), 0)
                .unwrap()
                .len(),
            1
        );
    }
    assert!(
        !storage
            .source_checkpoint_matches(Provider::Codex, &sources[1], &SourceStamp::file(10, 100))
            .unwrap()
    );
    assert!(
        storage
            .source_checkpoint_matches(Provider::Codex, &sources[2], &SourceStamp::file(10, 1))
            .unwrap()
    );
    drop(storage);
    let mut storage = Storage::open_writer(&database).unwrap();
    assert!(storage.remove_source(Provider::Codex, &sources[0]).unwrap());
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().chats, 2);
    assert_eq!(storage.view("archived-message", 0).unwrap().len(), 1);
}

#[test]
fn writer_checkpoint_survives_a_later_rollback() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut writer = Storage::open_writer(&path).expect("writer");
    writer
        .replace_chat(&chat_with_messages(&[("message-1", "durable needle")]))
        .expect("durable mutation");
    writer
        .record_source_checkpoint(
            crate::provider::Provider::Codex,
            &crate::silver::SourceIdentity::file("/tmp/session-1.jsonl"),
            &SourceStamp::file(100, 200),
        )
        .expect("durable source checkpoint");
    writer.checkpoint_writer().expect("checkpoint batch");
    writer
        .replace_chat(&chat("rolled back text"))
        .expect("later mutation");
    drop(writer);

    let storage = Storage::open_existing(&path).expect("reopen database");
    assert_eq!(
        storage.view("message-1", 0).expect("read committed batch")[0].content,
        "durable needle"
    );
    assert!(
        storage
            .source_checkpoint_matches(
                crate::provider::Provider::Codex,
                &crate::silver::SourceIdentity::file("/tmp/session-1.jsonl"),
                &SourceStamp::file(100, 200)
            )
            .expect("read checkpoint")
    );
}

#[test]
fn concurrent_writer_is_rejected_and_first_writer_can_commit() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut first = Storage::open_writer(&path).expect("first writer");
    let error = Storage::open_writer(&path)
        .err()
        .expect("second writer rejected");
    assert_eq!(error.error.kind, crate::ErrorKind::IndexBusy);
    first.commit_writer().expect("first writer commits");
    Storage::open_writer(&path).expect("writer available after commit");
}

#[test]
fn chat_reconciliation_writes_only_changed_messages() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut storage = Storage::open_writer(&path).expect("writer");
    let first = storage
        .replace_chat(&chat("first"))
        .expect("initial insert");
    assert_eq!(first.changed_message_ids, ["message-1"]);
    let unchanged = storage
        .replace_chat(&chat("first"))
        .expect("unchanged refresh");
    assert!(unchanged.unchanged);
    assert_eq!(unchanged.changed_message_ids, Vec::<String>::new());
    let unchanged_revision = storage.silver_certificate().unwrap().revision;
    let changed = storage
        .replace_chat(&chat("second"))
        .expect("changed refresh");
    assert_eq!(changed.changed_message_ids, ["message-1"]);
    assert!(storage.silver_certificate().unwrap().revision > unchanged_revision);
    assert_eq!(storage.view("message-1", 0).unwrap()[0].content, "second");
}

#[test]
fn chat_reconciliation_replaces_a_changed_message_id_at_the_same_ordinal() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut storage = Storage::open_writer(&path).expect("writer");
    storage
        .replace_chat(&chat("first"))
        .expect("initial insert");

    let mut replacement = chat("second");
    replacement.messages.inline_mut()[0].id = "message-2".to_owned();
    let change = storage
        .replace_chat(&replacement)
        .expect("replace message identity");

    assert_eq!(change.changed_message_ids, ["message-2"]);
    assert_eq!(change.removed_messages, 1);
    assert_eq!(storage.counts().expect("counts").messages, 1);
    storage.commit_writer().expect("commit replacement");
    assert!(storage.view("message-1", 0).unwrap().is_empty());
    assert_eq!(storage.view("message-2", 0).unwrap()[0].id, "message-2");
}

#[test]
fn chat_reconciliation_inserts_and_reorders_stable_ids_with_tree_context() {
    fn assert_parent(storage: &Storage) {
        let parent: String = storage
            .connection
            .query_row(
                "SELECT context_parent FROM messages WHERE id = 'reply'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(parent, "message-1");
    }
    fn history(insert: bool) -> crate::silver::Chat {
        let mut value = chat("original request");
        let shift = i64::from(insert);
        value.messages.inline_mut()[0].context = crate::message_context::MessageContext::Root;
        value.messages.inline_mut()[0].ordinal = shift;
        value
            .messages
            .inline_mut()
            .push(crate::silver::NormalizedMessage {
                attachments: Vec::new(),
                id: "reply".into(),
                ordinal: shift + 1,
                content: "original reply".into(),
                role: crate::role::Role::Assistant,
                context: crate::message_context::MessageContext::Parent {
                    id: "message-1".into(),
                },
                dialogue_text: None,
                created_at: None,
                tools: crate::tools::Activity::default(),
            });
        if insert {
            value.messages.inline_mut().insert(
                0,
                crate::silver::NormalizedMessage {
                    attachments: Vec::new(),
                    id: "inserted".into(),
                    ordinal: 0,
                    content: "recovered dialogue".into(),
                    role: crate::role::Role::User,
                    context: crate::message_context::MessageContext::Root,
                    dialogue_text: None,
                    created_at: None,
                    tools: crate::tools::Activity::default(),
                },
            );
        }
        value
    }
    let directory = tempfile::tempdir().unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    let original = history(false);
    storage.replace_chat(&original).unwrap();
    assert_parent(&storage);
    let shifted = history(true);
    storage.replace_chat(&shifted).unwrap();
    storage.commit_writer().unwrap();
    assert_parent(&storage);
    let viewed = storage.view("reply", 4).unwrap();
    assert_eq!(
        viewed.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        ["message-1", "reply"]
    );
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    // Removing the inserted item shifts the same IDs down into occupied slots.
    storage.replace_chat(&original).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 2);
    assert!(storage.view("inserted", 0).unwrap().is_empty());
    assert_eq!(storage.view("reply", 0).unwrap()[0].ordinal, 1);
    assert_parent(&storage);

    // A later failure must not commit temporary ordinals or partial context.
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    storage.begin_provider_scan().unwrap();
    let mut invalid = history(true);
    invalid.messages.inline_mut()[2].context = crate::message_context::MessageContext::Parent {
        id: "missing".into(),
    };
    assert!(storage.replace_chat(&invalid).is_err());
    storage.rollback_provider_scan().unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 2);
    assert_eq!(storage.view("message-1", 0).unwrap()[0].ordinal, 0);
    assert_eq!(storage.view("reply", 0).unwrap()[0].ordinal, 1);
}

#[test]
fn purging_a_chat_removes_its_canonical_entries() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut writer = Storage::open_writer(&path).expect("writer");
    writer
        .replace_chat(&chat("disappearing sentinel"))
        .expect("seed conversation");
    writer.commit_writer().expect("commit seed");
    drop(writer);

    let mut writer = Storage::open_writer(&path).expect("purge writer");
    assert_eq!(
        writer
            .purge_missing_sources(
                crate::provider::Provider::Codex,
                &BTreeSet::new(),
                &[std::path::PathBuf::from("/tmp")],
                None,
            )
            .expect("purge missing source"),
        1
    );
    writer.commit_writer().expect("commit purge");
    assert!(writer.view("message-1", 0).unwrap().is_empty());
    assert_eq!(writer.counts().unwrap().messages, 0);
}

#[test]
fn tombstone_prevents_reinsertion() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut writer = Storage::open_writer(&path).expect("writer");
    writer.replace_chat(&chat("remember me")).expect("insert");
    writer.commit_writer().expect("commit");
    drop(writer);

    let mut storage = Storage::open(&path).expect("open database");
    assert!(storage.forget("session-1").expect("forget"));
    drop(storage);

    let mut writer = Storage::open_writer(&path).expect("second writer");
    let change = writer
        .replace_chat(&chat("remember me"))
        .expect("tombstone check");
    assert!(change.tombstoned);
    writer.commit_writer().expect("commit");
    assert_eq!(writer.counts().expect("counts").chats, 0);
}

#[test]
fn ordinary_writer_commits_leave_wal_checkpointing_to_sqlite() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let wal_path = std::path::PathBuf::from(format!("{}-wal", path.display()));
    let mut writer = Storage::open_writer(&path).expect("writer");
    writer
        .replace_chat(&chat("small incremental write"))
        .expect("insert conversation");
    writer.commit_writer().expect("commit writer");

    assert!(
        std::fs::metadata(wal_path).is_ok_and(|metadata| metadata.len() > 0),
        "a small commit must not force a blocking WAL truncation"
    );
}
