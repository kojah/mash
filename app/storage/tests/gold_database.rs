//! Local gold is a separate, paired database that silver can always replay.
use super::*;
use crate::storage::schema::gold_path;

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
fn silver_holds_no_gold_and_gold_holds_no_history() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    indexed(&path);
    assert_eq!(
        gold_path(&path),
        directory.path().join("index.gold.sqlite3")
    );
    let silver = main_tables(&path);
    for table in [
        "message_embeddings",
        "message_fts",
        "gold_manifest",
        "derived_state",
    ] {
        assert!(
            !silver.iter().any(|name| name == table),
            "{table} in silver"
        );
    }
    let gold = main_tables(&gold_path(&path));
    for table in [
        "conversations",
        "messages",
        "tombstones",
        "source_checkpoints",
    ] {
        assert!(!gold.iter().any(|name| name == table), "{table} in gold");
    }
    // The same silver schema serves the shared server, with no gold beside it.
    let server = Connection::open(directory.path().join("server.sqlite3")).unwrap();
    crate::storage::schema::initialize_silver(&server).unwrap();
    let mut server_tables = main_tables(&directory.path().join("server.sqlite3"));
    server_tables.retain(|name| name != "sqlite_sequence");
    let mut local_tables = silver;
    local_tables.retain(|name| name != "sqlite_sequence");
    assert_eq!(server_tables, local_tables);
}

#[test]
fn combined_databases_split_without_losing_vectors_or_readiness() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    indexed(&path);
    let connection = Connection::open(&path).unwrap();
    crate::storage::test_support::combine_gold(&connection, &path);
    drop(connection);
    std::fs::remove_file(gold_path(&path)).unwrap();
    let storage = Storage::open(&path).unwrap();
    assert_eq!(storage.embedding_count("fixture").unwrap(), 1);
    assert!(storage.semantic_generation_is_published("fixture").unwrap());
    assert!(!storage.derived_search_is_dirty().unwrap());
    assert_eq!(storage.search("needle", 10, None, None).unwrap().len(), 1);
    drop(storage);
    assert!(
        !main_tables(&path)
            .iter()
            .any(|name| name == "message_embeddings")
    );
}

#[test]
fn missing_gold_reads_as_unready_and_writers_recreate_it_pending() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    indexed(&path);
    crate::storage::schema::remove_gold(&gold_path(&path)).unwrap();
    let reader = Storage::open_catalog(&path).unwrap();
    assert_eq!(reader.counts().unwrap().messages, 1);
    assert_eq!(reader.counts().unwrap().embeddings, 0);
    assert!(reader.derived_search_is_dirty().unwrap());
    drop(reader);
    assert!(!gold_path(&path).exists(), "readers never create gold");
    let writer = Storage::open_writer(&path).unwrap();
    assert!(gold_path(&path).exists());
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
fn gold_from_another_silver_is_never_used() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("first.sqlite3");
    let second = directory.path().join("second.sqlite3");
    indexed(&first);
    drop(Storage::open(&second).unwrap());
    crate::storage::schema::remove_gold(&gold_path(&second)).unwrap();
    std::fs::copy(gold_path(&first), gold_path(&second)).unwrap();
    let reader = Storage::open_catalog(&second).unwrap();
    assert_eq!(reader.counts().unwrap().embeddings, 0);
    drop(reader);
    let writer = Storage::open_writer(&second).unwrap();
    assert_eq!(writer.counts().unwrap().embeddings, 0);
    assert!(writer.derived_search_is_dirty().unwrap());
}

#[test]
fn deleting_silver_removes_its_gold_and_journals_the_change() {
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
        .execute("DELETE FROM conversations", [])
        .unwrap();
    assert_eq!(storage.counts().unwrap().embeddings, 0);
    assert!(revision(&storage) > before);
}

#[test]
fn discarding_gold_replays_from_unchanged_silver() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    indexed(&path);
    let mut storage = Storage::open_writer(&path).unwrap();
    let silver = storage.silver_certificate().unwrap();
    storage.discard_gold().unwrap();
    assert_eq!(storage.counts().unwrap().embeddings, 0);
    assert!(storage.derived_search_is_dirty().unwrap());
    assert!(storage.gold_manifest().unwrap().is_none());
    assert!(storage.lexical_manifest().unwrap().is_none());
    assert!(!storage.semantic_generation_is_published("fixture").unwrap());
    assert_eq!(storage.silver_certificate().unwrap(), silver);
    storage.commit_writer().unwrap();
}

#[cfg(unix)]
#[test]
fn recreated_gold_is_no_more_visible_than_silver() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    indexed(&path);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    crate::storage::schema::remove_gold(&gold_path(&path)).unwrap();
    drop(Storage::open_writer(&path).unwrap());
    let mode = std::fs::metadata(gold_path(&path))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}
