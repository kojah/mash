use super::*;
use std::path::PathBuf;

pub(super) use super::test_support::{chat, chat_with_messages};

mod chats;
mod indexing;
mod properties;
mod publication;
mod schema;
mod staging;
mod views_database;

#[test]
fn writers_commit_without_an_fsync_per_transaction() {
    let directory = tempfile::tempdir().unwrap();
    let storage = Storage::open(&directory.path().join("index.sqlite3")).unwrap();
    let mode: String = storage
        .connection
        .query_row("PRAGMA main.journal_mode", [], |row| row.get(0))
        .unwrap();
    let sync: i64 = storage
        .connection
        .query_row("PRAGMA main.synchronous", [], |row| row.get(0))
        .unwrap();
    assert_eq!((mode.as_str(), sync), ("wal", 1));
}

#[test]
fn policy_upgrade_is_pending_and_retries_keep_the_same_revision() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("index.sqlite3");
    let mut storage = crate::storage::Storage::open_writer(&path).unwrap();
    storage.commit_writer().unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE shared_publication SET revision=1 WHERE singleton=1",
            [],
        )
        .unwrap();
    storage.prepare_publication_target("first").unwrap();
    let revision = storage.with_publication_snapshot(|_| Ok(())).unwrap();
    storage
        .acknowledge_publication(
            "first",
            revision,
            crate::storage::publication::JournalCoverage::Incremental { through: revision },
        )
        .unwrap();
    assert!(!crate::storage::Storage::publication_is_pending(&path, "first").unwrap());
    storage.prepare_publication_target("redaction-v1").unwrap();
    let next = storage.with_publication_snapshot(|_| Ok(())).unwrap();
    assert_eq!(next, revision + 1);
    assert!(crate::storage::Storage::publication_is_pending(&path, "redaction-v1").unwrap());
    storage.prepare_publication_target("redaction-v1").unwrap();
    assert_eq!(storage.with_publication_snapshot(|_| Ok(())).unwrap(), next);
}
