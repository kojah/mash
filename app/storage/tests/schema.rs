use super::*;

fn user_version(path: &Path) -> i64 {
    Connection::open(path)
        .expect("reopen seed")
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("schema version")
}

#[test]
fn newer_schema_is_rejected_without_rewriting_version() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let connection = Connection::open(&path).expect("seed database");
    connection
        .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .expect("newer version");
    drop(connection);

    let error = Storage::open(&path).err().expect("newer schema rejected");
    assert_eq!(error.error.kind, crate::ErrorKind::SchemaIncompatible);
    // Usually a process that predates an upgrade: reconnecting is the fix,
    // not exporting with an older release.
    assert!(error.error.message.contains("newer than this mash"));
    assert!(error.error.message.contains("Reconnect Mash MCP"));
    assert!(!error.error.message.contains("mash import"));
    assert_eq!(user_version(&path), SCHEMA_VERSION + 1);
}

/// Upgrades start at the last single-file schema; older indexes are rebuilt
/// from provider histories or exported with the release that created them.
#[test]
fn schemas_before_the_supported_upgrade_are_rejected_untouched() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let connection = Connection::open(&path).expect("seed database");
    connection
        .execute_batch(
            "CREATE TABLE chats(id TEXT PRIMARY KEY);
             INSERT INTO chats VALUES('kept');
             PRAGMA user_version = 44;",
        )
        .expect("older schema");
    drop(connection);

    let error = Storage::open(&path).err().expect("older schema rejected");
    assert_eq!(error.error.kind, crate::ErrorKind::SchemaIncompatible);
    assert!(error.error.message.contains("mash import"));
    assert_eq!(user_version(&path), 44);
    assert!(!directory.path().join("mash.views.sqlite3").exists());
    let kept: i64 = Connection::open(&path)
        .unwrap()
        .query_row("SELECT count(*) FROM chats", [], |row| row.get(0))
        .unwrap();
    assert_eq!(kept, 1);
}

fn baseline(path: &Path) -> Connection {
    let connection = Connection::open(path).unwrap();
    crate::storage::packed::register(&connection).unwrap();
    connection
        .execute_batch(super::super::schema::SILVER)
        .unwrap();
    connection
        .execute_batch(super::super::schema::SILVER_INDEXES)
        .unwrap();
    connection
        .execute_batch(
            "PRAGMA user_version=62;
        INSERT INTO chats(id,provider,title) VALUES('kept','codex','history');
        INSERT INTO messages(id,chat_id,ordinal,role,content_packed)
        VALUES('message','kept',0,'user','retain this history');",
        )
        .unwrap();
    connection
}

#[test]
fn records_migration_preserves_history_identity_and_chat_search_revision() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    let connection = baseline(&path);
    let snapshot = |connection: &Connection| -> (String, String, i64, String) {
        connection
            .query_row(
                "SELECT d.instance,p.instance,s.revision,m.content
            FROM local_database_identity d,shared_publication p,silver_state s,messages m",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap()
    };
    let before = snapshot(&connection);
    drop(connection);
    let storage = Storage::open(&path).unwrap();
    assert_eq!(snapshot(&storage.connection), before);
    assert_eq!(user_version(&path), SCHEMA_VERSION);
    let owner: String = storage
        .connection
        .query_row("SELECT id FROM record_owner", [], |row| row.get(0))
        .unwrap();
    drop(storage);
    let reopened = Storage::open(&path).unwrap();
    assert_eq!(
        reopened
            .connection
            .query_row("SELECT id FROM record_owner", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        owner
    );
    assert_eq!(snapshot(&reopened.connection), before);
}

#[test]
fn records_migration_rolls_back_all_ddl_and_version_on_failure() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    let connection = baseline(&path);
    // Force a failure after record_owner was created inside the migration.
    connection
        .execute_batch("CREATE TABLE records(existing TEXT)")
        .unwrap();
    drop(connection);
    assert!(Storage::open(&path).is_err());
    assert_eq!(user_version(&path), 62);
    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='record_owner'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM messages", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    connection.execute_batch("DROP TABLE records").unwrap();
    drop(connection);
    Storage::open(&path).unwrap();
    assert_eq!(user_version(&path), SCHEMA_VERSION);
}

#[test]
fn records_migration_respects_existing_writer_admission() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    drop(baseline(&path));
    let owner = crate::storage::admission::acquire(&path).unwrap();
    let error = Storage::open(&path).err().unwrap();
    assert_eq!(error.error.kind, crate::ErrorKind::IndexBusy);
    assert_eq!(user_version(&path), 62);
    // The admitted operation may itself initialize on a blocking worker.
    crate::storage::admission::on_blocking_thread(Some(owner), || Storage::open(&path)).unwrap();
    assert_eq!(user_version(&path), SCHEMA_VERSION);
}

#[test]
fn records_migration_does_not_reuse_another_databases_admission() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    drop(baseline(&path));
    let owner = crate::storage::admission::acquire(&path).unwrap();
    let other =
        crate::storage::admission::acquire(&directory.path().join("other.sqlite3")).unwrap();
    let error = crate::storage::admission::on_blocking_thread(Some(other), || Storage::open(&path))
        .err()
        .unwrap();
    assert_eq!(error.error.kind, crate::ErrorKind::IndexBusy);
    assert_eq!(user_version(&path), 62);
    drop(owner);
}
