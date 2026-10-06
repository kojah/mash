//! Read-only snapshots for manual planning diagnostics without readiness spoofing.
use super::*;

pub(crate) fn pending_snapshot(path: &Path) -> Storage {
    let connection = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("explicit pending snapshot");
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("snapshot schema");
    assert_eq!(version, SCHEMA_VERSION, "prepare a schema-current snapshot");
    Storage {
        attachments_config: crate::attachments::Configuration::default(),
        connection,
        writer_active: false,
        search_updates: SearchUpdates::Immediate,
        transaction_base_messages: 0,
    }
}

pub(crate) fn assert_quantized_embedding(storage: &Storage, id: &str, vector: &[u8], norm: f32) {
    let (stored_vector, stored_norm): (Vec<u8>, f64) = storage
        .connection
        .query_row(
            "SELECT vector, norm FROM message_embeddings WHERE message_id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("stored embedding");
    assert_eq!(
        stored_vector, vector,
        "stored vector differs from in-memory plan"
    );
    assert_eq!(
        stored_norm,
        f64::from(norm),
        "stored norm differs from in-memory plan"
    );
}

const GOLD_TABLES: [&str; 8] = [
    "silver_pairing",
    "gold_manifest",
    "lexical_manifest",
    "derived_state",
    "message_embeddings",
    "pending_embeddings",
    "semantic_chunks",
    "message_fts",
];

/// Raw access to local silver with its paired gold attached.
pub(crate) fn connect(path: &Path) -> Connection {
    let connection = Connection::open(path).expect("open local silver");
    attach_gold(&connection, path);
    connection
}

fn attach_gold(connection: &Connection, path: &Path) {
    let attached: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_database_list WHERE name = 'gold')",
            [],
            |row| row.get(0),
        )
        .expect("database list");
    if !attached {
        connection
            .execute(
                "ATTACH DATABASE ?1 AS gold",
                [super::schema::gold_path(path).to_str().unwrap()],
            )
            .expect("attach gold");
    }
}

/// Recreate the single-file layout that predates schema 46, so tests can
/// fake an older version. Main shadows gold afterwards; opening the file again
/// upgrades it and splits gold out once more.
pub(crate) fn combine_gold(connection: &Connection, path: &Path) {
    attach_gold(connection, path);
    for table in GOLD_TABLES {
        let sql: String = connection
            .query_row(
                "SELECT sql FROM gold.sqlite_master WHERE name = ?1",
                [table],
                |row| row.get(0),
            )
            .expect("gold table definition");
        connection.execute_batch(&sql).expect("combined gold table");
        let (target, columns) = if table == "message_fts" {
            (
                "message_fts(rowid, content, message_id, conversation_id)",
                "rowid, content, message_id, conversation_id",
            )
        } else {
            (table, "*")
        };
        connection
            .execute_batch(&format!(
                "INSERT INTO main.{target} SELECT {columns} FROM gold.{table}"
            ))
            .expect("combined gold rows");
    }
    connection
        .execute_batch("PRAGMA user_version = 45")
        .expect("combined version");
}

/// Apply raw SQL through a mash connection, with its publication journal and
/// gold-following triggers installed.
pub(crate) fn write(path: &Path, sql: &str) {
    Storage::open_existing(path)
        .expect("local index")
        .connection
        .execute_batch(sql)
        .expect("raw write");
}
