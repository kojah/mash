//! Local views live in a separate, paired database that silver can always replay.
use super::*;
use crate::storage::schema::views_path;

fn indexed(path: &Path) {
    let mut storage = Storage::open_writer(path).unwrap();
    storage
        .replace_conversation(&conversation("durable needle"))
        .unwrap();
    storage.rebuild_derived_search_state().unwrap();
    storage
        .replace_embeddings(
            "fixture",
            &[EmbeddingWrite {
                message_id: "message-1",
                vector: &[127, 0],
                norm: 127.0,
            }],
        )
        .unwrap();
    storage.mark_semantic_index_ready("fixture").unwrap();
    storage.commit_writer().unwrap();
}

fn main_tables(path: &Path) -> Vec<String> {
    let connection = Connection::open(path).unwrap();
    let mut statement = connection
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn silver_holds_no_views_and_views_holds_no_history() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    indexed(&path);
    assert_eq!(
        views_path(&path),
        directory.path().join("index.views.sqlite3")
    );
    let silver = main_tables(&path);
    for table in [
        "message_embeddings",
        "message_fts",
        "views_manifest",
        "derived_state",
    ] {
        assert!(
            !silver.iter().any(|name| name == table),
            "{table} in silver"
        );
    }
    let views = main_tables(&views_path(&path));
    for table in [
        "objects",
        "native_chats",
        "native_messages",
        "tombstones",
        "source_checkpoints",
    ] {
        assert!(!views.iter().any(|name| name == table), "{table} in views");
    }
    // The same silver schema serves the shared server, with no views beside it.
    let server = Connection::open(directory.path().join("server.sqlite3")).unwrap();
    crate::storage::schema::initialize_silver(&server).unwrap();
    let mut server_tables = main_tables(&directory.path().join("server.sqlite3"));
    server_tables.retain(|name| name != "sqlite_sequence");
    let mut local_tables = silver;
    local_tables.retain(|name| name != "sqlite_sequence");
    assert_eq!(server_tables, local_tables);
}

#[test]
fn missing_views_reads_as_unready_and_writers_recreate_it_pending() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    indexed(&path);
    crate::storage::schema::remove_views(&views_path(&path)).unwrap();
    let reader = Storage::open_catalog(&path).unwrap();
    assert_eq!(reader.counts().unwrap().messages, 1);
    assert_eq!(reader.counts().unwrap().embeddings, 0);
    assert!(reader.derived_search_is_dirty().unwrap());
    drop(reader);
    assert!(!views_path(&path).exists(), "readers never create views");
    let writer = Storage::open_writer(&path).unwrap();
    assert!(views_path(&path).exists());
    assert_eq!(writer.embedding_count("fixture").unwrap(), 0);
    assert!(writer.derived_search_is_dirty().unwrap());
    let pending: i64 = writer
        .connection
        .query_row("SELECT count(*) FROM pending_embeddings", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(pending, 1);
}

#[test]
fn views_from_another_silver_is_never_used() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("first.sqlite3");
    let second = directory.path().join("second.sqlite3");
    indexed(&first);
    drop(Storage::open(&second).unwrap());
    crate::storage::schema::remove_views(&views_path(&second)).unwrap();
    std::fs::copy(views_path(&first), views_path(&second)).unwrap();
    let reader = Storage::open_catalog(&second).unwrap();
    assert_eq!(reader.counts().unwrap().embeddings, 0);
    drop(reader);
    let writer = Storage::open_writer(&second).unwrap();
    assert_eq!(writer.counts().unwrap().embeddings, 0);
    assert!(writer.derived_search_is_dirty().unwrap());
}

#[test]
fn deleting_silver_removes_its_views_and_journals_the_change() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    indexed(&path);
    let revision = |storage: &Storage| -> i64 {
        storage
            .connection
            .query_row("SELECT revision FROM shared_publication", [], |row| {
                row.get(0)
            })
            .unwrap()
    };
    let storage = Storage::open_writer(&path).unwrap();
    let before = revision(&storage);
    storage
        .connection
        .execute("DELETE FROM objects WHERE origin = 'provider'", [])
        .unwrap();
    assert_eq!(storage.counts().unwrap().embeddings, 0);
    assert!(revision(&storage) > before);
}

#[test]
fn discarding_views_replays_from_unchanged_silver() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    indexed(&path);
    let mut storage = Storage::open_writer(&path).unwrap();
    let silver = storage.silver_certificate().unwrap();
    storage.discard_views().unwrap();
    assert_eq!(storage.counts().unwrap().embeddings, 0);
    assert!(storage.derived_search_is_dirty().unwrap());
    assert!(storage.views_manifest().unwrap().is_none());
    assert!(storage.lexical_manifest().unwrap().is_none());
    assert!(!storage.semantic_generation_is_published("fixture").unwrap());
    assert_eq!(storage.silver_certificate().unwrap(), silver);
    storage.commit_writer().unwrap();
}

#[cfg(unix)]
#[test]
fn recreated_views_is_no_more_visible_than_silver() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    indexed(&path);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    crate::storage::schema::remove_views(&views_path(&path)).unwrap();
    drop(Storage::open_writer(&path).unwrap());
    let mode = std::fs::metadata(views_path(&path))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}
