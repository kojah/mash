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
            "CREATE TABLE conversations(id TEXT PRIMARY KEY);
             INSERT INTO conversations VALUES('kept');
             PRAGMA user_version = 44;",
        )
        .expect("older schema");
    drop(connection);

    let error = Storage::open(&path).err().expect("older schema rejected");
    assert_eq!(error.error.kind, crate::ErrorKind::SchemaIncompatible);
    assert!(error.error.message.contains("mash import"));
    assert_eq!(user_version(&path), 44);
    assert!(!crate::storage::schema::views_path(&path).exists());
    let kept: i64 = Connection::open(&path)
        .unwrap()
        .query_row("SELECT count(*) FROM conversations", [], |row| row.get(0))
        .unwrap();
    assert_eq!(kept, 1);
}
