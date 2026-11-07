//! SQLite stores canonical silver only; local and shared use the same schema.
use super::*;

fn seed(path: &Path) {
    let mut storage = Storage::open_writer(path).unwrap();
    storage.replace_chat(&chat("durable needle")).unwrap();
    storage.commit_writer().unwrap();
}

fn tables(connection: &Connection) -> Vec<String> {
    connection
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn canonical_silver_reader_never_opens_or_synthesizes_gold() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    seed(&path);
    let old_gold = directory.path().join("index.views.sqlite3");
    std::fs::write(&old_gold, b"unreadable retired gold").unwrap();
    let reader = Storage::open_silver(&path).unwrap();
    assert_eq!(reader.silver_counts().unwrap().messages, 1);
    assert_eq!(
        reader.view("message-1", 0).unwrap()[0].content,
        "durable needle"
    );
    let databases = reader
        .connection
        .prepare("PRAGMA database_list")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(databases, ["main"]);
    assert!(reader.connection.execute("DELETE FROM chats", []).is_err());
    assert_eq!(
        std::fs::read(&old_gold).unwrap(),
        b"unreadable retired gold"
    );
}

#[test]
fn local_and_shared_silver_have_one_schema_without_gold_tables() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    seed(&path);
    let local = Storage::open_silver(&path).unwrap();
    let mut server = Connection::open_in_memory().unwrap();
    crate::storage::schema::initialize_silver(&mut server).unwrap();
    let local_tables = tables(&local.connection);
    assert_eq!(local_tables, tables(&server));
    for table in [
        "message_embeddings",
        "message_fts",
        "views_manifest",
        "derived_state",
        "connector_units",
        "connector_embeddings",
        "semantic_chunks",
        "pending_embeddings",
    ] {
        assert!(
            !local_tables.iter().any(|name| name == table),
            "{table} in silver"
        );
    }
    assert!(!directory.path().join("index.views.sqlite3").exists());
}

#[test]
fn deleting_silver_journals_the_canonical_change() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    seed(&path);
    let storage = Storage::open_writer(&path).unwrap();
    let revision = |storage: &Storage| {
        storage
            .connection
            .query_row("SELECT revision FROM shared_publication", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
    };
    let before = revision(&storage);
    storage
        .connection
        .execute("DELETE FROM chats ", [])
        .unwrap();
    assert!(revision(&storage) > before);
    assert_eq!(storage.counts().unwrap().messages, 0);
}
