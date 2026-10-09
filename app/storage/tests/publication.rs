use super::*;
use crate::shared::protocol::BatchKind;

#[test]
fn journal_survives_reopen_tracks_complete_threads_and_preserves_newer_edits() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    let mut writer = Storage::open_writer(&path).unwrap();
    writer.replace_conversation(&conversation("first")).unwrap();
    writer.commit_writer().unwrap();
    drop(writer);
    let storage = Storage::open_existing(&path).unwrap();
    storage.prepare_publication_target("destination").unwrap();
    let first = storage.with_publication_snapshot(|_| Ok(())).unwrap();
    storage
        .acknowledge_publication("destination", first)
        .unwrap();
    drop(storage);
    let mut writer = Storage::open_writer(&path).unwrap();
    writer
        .replace_conversation(&conversation("changed"))
        .unwrap();
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
        .execute(
            "UPDATE objects SET title='newer' WHERE origin='provider' AND id='session-1'",
            [],
        )
        .unwrap();
    storage
        .acknowledge_publication("destination", sent)
        .unwrap();
    assert!(Storage::publication_is_pending(&path, "destination").unwrap());
    storage
        .with_publication_snapshot(|snapshot| {
            let mut threads = 0;
            snapshot.visit_changed(BatchKind::Conversations, sent, |batch| {
                threads += batch.len();
                Ok(())
            })?;
            assert_eq!(threads, 1);
            Ok(())
        })
        .unwrap();
    storage
        .connection
        .execute(
            "DELETE FROM objects WHERE origin='provider' AND id='session-1'",
            [],
        )
        .unwrap();
    storage
        .with_publication_snapshot(|snapshot| {
            assert_eq!(snapshot.removed_threads(sent)?, vec!["session-1"]);
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
        transaction.execute("INSERT INTO objects(origin,source,id,object_role,kind,conversation_id) VALUES('provider','codex','rollback','container','codex:chat','rollback')",[]).unwrap();
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
