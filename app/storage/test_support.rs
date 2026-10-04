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
