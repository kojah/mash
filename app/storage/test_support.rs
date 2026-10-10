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
        output_redaction: crate::redaction::Policy::ON,
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

/// Raw access to local silver with its paired views attached.
pub(crate) fn connect(path: &Path) -> Connection {
    let connection = Connection::open(path).expect("open local silver");
    attach_views(&connection, path);
    connection
}

fn attach_views(connection: &Connection, path: &Path) {
    let attached: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_database_list WHERE name = 'views')",
            [],
            |row| row.get(0),
        )
        .expect("database list");
    if !attached {
        connection
            .execute(
                "ATTACH DATABASE ?1 AS views",
                [super::schema::views_path(path).to_str().unwrap()],
            )
            .expect("attach views");
    }
}

/// Apply raw SQL through a mash connection, with its publication journal and
/// views-following triggers installed.
pub(crate) fn write(path: &Path, sql: &str) {
    Storage::open_existing(path)
        .expect("local index")
        .connection
        .execute_batch(sql)
        .expect("raw write");
}
