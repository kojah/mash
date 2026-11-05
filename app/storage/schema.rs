use rusqlite::Connection;
use std::fmt::Write;

use crate::AppError;
use crate::database_error::Operation;

/// Silver database version; any other version is rejected, never migrated.
pub(crate) const SCHEMA_VERSION: i64 = 62;

/// The one silver schema, shared by local nodes and the shared server.
pub(crate) const SILVER: &str = include_str!("silver.sql");
/// Silver indexes and triggers; they require every silver column to exist.
pub(crate) const SILVER_INDEXES: &str = include_str!("silver_indexes.sql");
/// Writers run in WAL mode with NORMAL sync: commits are atomic and survive a
/// process crash without an fsync each; only an OS crash or power loss can
/// drop the latest commits, and the database stays consistent. Per connection.
const WRITER_PRAGMAS: &str =
    "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;";

/// Create or validate a silver database without any local views.
pub(crate) fn initialize_silver(connection: &Connection) -> Result<(), AppError> {
    super::packed::register(connection).map_err(AppError::database)?;
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(AppError::database)?;
    if version != 0 && version != SCHEMA_VERSION {
        return Err(unsupported(version));
    }
    connection
        .execute_batch(WRITER_PRAGMAS)
        .map_err(AppError::database_at(Operation::ConfigureWriter))?;
    if version == 0 {
        let transaction = connection
            .unchecked_transaction()
            .map_err(AppError::database)?;
        transaction
            .execute_batch(SILVER)
            .map_err(AppError::database)?;
        transaction
            .execute_batch(SILVER_INDEXES)
            .map_err(AppError::database)?;
        transaction
            .pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(AppError::database)?;
        transaction.commit().map_err(AppError::database)?;
    }
    Ok(())
}

/// The typed error for any silver schema this release does not write. A newer
/// schema usually means this process predates an upgrade that already moved
/// the index forward, such as an MCP server started before it.
pub(crate) fn unsupported(version: i64) -> AppError {
    if version > SCHEMA_VERSION {
        return AppError::schema(format!(
            "local index schema {version} is newer than this mash's schema {SCHEMA_VERSION}; a newer mash release wrote it. Reconnect Mash MCP or restart `mash index --watch` if mash was already upgraded; otherwise upgrade mash"
        ));
    }
    AppError::schema(format!(
        "local index schema {version} is older than this mash's schema {SCHEMA_VERSION}; export it with the mash release that wrote it, then `mash import --keep-identity` it with this one"
    ))
}

pub(super) fn install_publication_triggers(connection: &Connection) -> Result<(), AppError> {
    let mut statements = String::new();
    // Publication journals canonical agent history; turns are derived on receipt.
    for table in ["chats", "messages", "tombstones", "chat_enrichment"] {
        for operation in ["INSERT", "UPDATE", "DELETE"] {
            let references = match operation {
                "INSERT" => vec!["NEW"],
                "DELETE" => vec!["OLD"],
                _ => vec!["OLD", "NEW"],
            };
            let mut changes = String::new();
            for reference in &references {
                let id = if table == "chats" {
                    format!("{reference}.id")
                } else {
                    format!("{reference}.chat_id")
                };
                write!(changes,
                    "INSERT INTO main.shared_changes SELECT {id},revision FROM main.shared_publication WHERE singleton=1 AND {id} IS NOT NULL
                     ON CONFLICT(chat_id) DO UPDATE SET revision=excluded.revision;"
                ).map_err(|_|AppError::internal("format publication trigger"))?;
            }
            write!(
                statements,
                "CREATE TEMP TRIGGER IF NOT EXISTS shared_{table}_{operation}
                 AFTER {operation} ON main.{table} BEGIN
                 UPDATE main.shared_publication SET revision = revision + 1 WHERE singleton = 1;
                 {changes}
                 END;"
            )
            .map_err(|_| AppError::internal("format publication trigger"))?;
        }
    }
    connection
        .execute_batch(&statements)
        .map_err(AppError::database)
}
