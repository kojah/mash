use super::*;
use crate::shared::batches::BatchKind;

#[test]
fn journal_pruning_waits_for_baseline_completion_and_contiguous_coverage() {
    use crate::storage::publication::JournalCoverage;

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    let mut writer = Storage::open_writer(&path).unwrap();
    for index in 0..4 {
        let mut chat = chat(&format!("edit-{index}"));
        chat.id = format!("session-{index}");
        chat.source = crate::silver::SourceIdentity::file(PathBuf::from(format!(
            "/tmp/session-{index}.jsonl"
        )));
        chat.messages =
            crate::silver::messages::Messages::stage([crate::silver::NormalizedMessage {
                attachments: Vec::new(),
                id: format!("message-{index}"),
                ordinal: 0,
                context: crate::message_context::MessageContext::Linear,
                role: crate::role::Role::User,
                content: format!("edit-{index}"),
                dialogue_text: None,
                created_at: Some(i64::MAX / 2),
                tools: crate::tools::Activity::default(),
            }])
            .unwrap();
        writer.replace_chat(&chat).unwrap();
    }
    writer.commit_writer().unwrap();
    drop(writer);
    let storage = Storage::open_existing(&path).unwrap();
    storage.prepare_publication_target("destination").unwrap();

    let journal_rows = || -> i64 {
        storage
            .connection
            .query_row("SELECT COUNT(*) FROM shared_changes", [], |row| row.get(0))
            .unwrap()
    };
    let floor = || -> i64 {
        storage
            .connection
            .query_row("SELECT floor FROM shared_change_state", [], |row| {
                row.get(0)
            })
            .unwrap()
    };
    assert_eq!(journal_rows(), 4, "one row per edited conversation");
    let before = floor();
    let revisions = {
        let mut statement = storage
            .connection
            .prepare("SELECT revision FROM shared_changes ORDER BY revision")
            .unwrap();
        statement
            .query_map([], |row| row.get::<_, i64>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    let latest: i64 = storage
        .connection
        .query_row("SELECT revision FROM shared_publication", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(revisions.len(), 4);
    assert_eq!(revisions[3], latest);

    // A baseline in progress acknowledges the published range but retains every
    // journal row, so live edits still replay after an interruption.
    storage
        .acknowledge_publication(
            "destination",
            revisions[1],
            JournalCoverage::BaselinePending,
        )
        .unwrap();
    assert_eq!(journal_rows(), 4);
    assert!(floor() >= before, "the prune floor never moves backwards");
    // The newest revision is still unpublished, so the node stays pending.
    assert!(Storage::publication_is_pending(&path, "destination").unwrap());

    // Partial baseline coverage prunes only through the contiguous watermark,
    // never through the newest received revision.
    storage
        .acknowledge_publication(
            "destination",
            latest,
            JournalCoverage::BaselineComplete {
                through: revisions[0],
            },
        )
        .unwrap();
    assert_eq!(journal_rows(), 3, "only revisions through the watermark go");
    assert!(floor() >= before);

    // Baseline complete plus coverage through the newest revision prunes the
    // rest of the journal.
    storage
        .acknowledge_publication(
            "destination",
            latest,
            JournalCoverage::BaselineComplete { through: latest },
        )
        .unwrap();
    assert_eq!(journal_rows(), 0);
    assert!(floor() >= before);
    assert!(!Storage::publication_is_pending(&path, "destination").unwrap());
}

#[test]
fn journal_survives_reopen_tracks_complete_threads_and_preserves_newer_edits() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    let mut writer = Storage::open_writer(&path).unwrap();
    writer.replace_chat(&chat("first")).unwrap();
    writer.commit_writer().unwrap();
    drop(writer);
    let storage = Storage::open_existing(&path).unwrap();
    storage.prepare_publication_target("destination").unwrap();
    let first = storage.with_publication_snapshot(|_| Ok(())).unwrap();
    storage
        .acknowledge_publication(
            "destination",
            first,
            crate::storage::publication::JournalCoverage::Incremental { through: first },
        )
        .unwrap();
    drop(storage);
    let mut writer = Storage::open_writer(&path).unwrap();
    writer.replace_chat(&chat("changed")).unwrap();
    writer.commit_writer().unwrap();
    drop(writer);
    let storage = Storage::open_existing(&path).unwrap();
    let mut owner = String::new();
    let sent = storage
        .with_publication_snapshot(|snapshot| {
            owner.clone_from(&snapshot.instance);
            assert_eq!(snapshot.delta_base(Some(&owner), first)?, Some(first));
            let mut messages = 0;
            snapshot.visit_changed(BatchKind::Messages, first, |batch| {
                messages += batch.len();
                Ok(())
            })?;
            assert_eq!(messages, 1);
            Ok(())
        })
        .unwrap();
    storage
        .connection
        .execute("UPDATE chats SET title='newer' WHERE id='session-1'", [])
        .unwrap();
    storage
        .acknowledge_publication(
            "destination",
            sent,
            crate::storage::publication::JournalCoverage::Incremental { through: sent },
        )
        .unwrap();
    assert!(Storage::publication_is_pending(&path, "destination").unwrap());
    storage
        .with_publication_snapshot(|snapshot| {
            let mut threads = 0;
            snapshot.visit_changed(BatchKind::Chats, sent, |batch| {
                threads += batch.len();
                Ok(())
            })?;
            assert_eq!(threads, 1);
            Ok(())
        })
        .unwrap();
    storage
        .connection
        .execute("DELETE FROM chats WHERE id='session-1'", [])
        .unwrap();
    storage
        .with_publication_snapshot(|snapshot| {
            assert_eq!(snapshot.removed_chats(sent)?, vec!["session-1"]);
            assert_eq!(snapshot.delta_base(Some("other-instance"), sent)?, None);
            Ok(())
        })
        .unwrap();
    storage
        .prepare_publication_target("new-destination")
        .unwrap();
    storage
        .with_publication_snapshot(|snapshot| {
            assert_eq!(snapshot.delta_base(Some(&owner), sent)?, None);
            Ok(())
        })
        .unwrap();
}

#[test]
fn journal_changes_roll_back_with_the_source_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    let storage = Storage::open(&path).unwrap();
    let before = storage.with_publication_snapshot(|_| Ok(())).unwrap();
    {
        let transaction = storage.connection.unchecked_transaction().unwrap();
        transaction
            .execute(
                "INSERT INTO chats(id,provider) VALUES('rollback','codex')",
                [],
            )
            .unwrap();
        let changed: i64 = transaction
            .query_row("SELECT count(*) FROM shared_changes", [], |row| row.get(0))
            .unwrap();
        assert_eq!(changed, 1);
    }
    assert_eq!(
        storage.with_publication_snapshot(|_| Ok(())).unwrap(),
        before
    );
    let remaining: i64 = storage
        .connection
        .query_row("SELECT count(*) FROM shared_changes", [], |row| row.get(0))
        .unwrap();
    assert_eq!(remaining, 0);
}
