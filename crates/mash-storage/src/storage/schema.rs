use rusqlite::Connection;
use std::fmt::Write;

use crate::AppError;
use crate::database_error::Operation;

/// First supported migration baseline. Earlier releases require export/import.
const BASELINE: i64 = 62;
pub const MIN_SCHEMA_VERSION: i64 = BASELINE;
pub const SCHEMA_VERSION: i64 = 67;

/// The one silver schema, shared by local nodes and the shared server.
pub const SILVER: &str = include_str!("silver.sql");
/// Silver indexes and triggers; they require every silver column to exist.
pub const SILVER_INDEXES: &str = include_str!("silver_indexes.sql");
const RECORDS: &str = include_str!("records.sql");
const RECORD_PROVENANCE: &str = include_str!("record_provenance.sql");
const RECORD_EXTRACTIONS: &str = include_str!("record_extractions.sql");
const RECORD_KINDS: &str = include_str!("record_kinds.sql");
const PUBLICATION_MESSAGES: &str = include_str!("publication_messages.sql");
/// Writers run in WAL mode with NORMAL sync: commits are atomic and survive a
/// process crash without an fsync each; only an OS crash or power loss can
/// drop the latest commits, and the database stays consistent. Per connection.
const WRITER_PRAGMAS: &str =
    "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;";

/// Create or validate a silver database without any local views.
pub fn initialize_silver(connection: &mut Connection) -> Result<(), AppError> {
    super::packed::register(connection).map_err(AppError::database)?;
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(AppError::database)?;
    if version != 0 && !(BASELINE..=SCHEMA_VERSION).contains(&version) {
        return Err(unsupported(version));
    }
    // A current database never takes an extra lock. Upgrades retain the same
    // admission as indexing, including the awaited silver-to-views interval.
    let _owner = if version == SCHEMA_VERSION {
        None
    } else {
        connection
            .path()
            .filter(|path| !path.is_empty())
            .map(|path| {
                let path = std::path::Path::new(path);
                if let Some(owner) = super::admission::current()
                    && owner.covers(path)?
                {
                    return Ok(owner);
                }
                super::admission::acquire(path)
            })
            .transpose()?
    };
    // Another upgrader could have finished between the initial read and lock.
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(AppError::database)?;
    if version != 0 && !(BASELINE..=SCHEMA_VERSION).contains(&version) {
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
            .execute_batch(RECORDS)
            .map_err(AppError::database)?;
        transaction
            .execute_batch(RECORD_PROVENANCE)
            .map_err(AppError::database)?;
        transaction
            .execute_batch(RECORD_EXTRACTIONS)
            .map_err(AppError::database)?;
        transaction
            .execute_batch(RECORD_KINDS)
            .map_err(AppError::database)?;
        transaction
            .execute_batch(PUBLICATION_MESSAGES)
            .map_err(AppError::database)?;
        transaction
            .pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(AppError::database)?;
        transaction.commit().map_err(AppError::database)?;
    } else if version < SCHEMA_VERSION {
        // Preserve the published user_version numbering. Entries before the
        // baseline are placeholders, NEVER a supported legacy upgrade path.
        let count = usize::try_from(BASELINE)
            .map_err(|_| AppError::internal("invalid migration baseline"))?;
        let mut steps = vec![rusqlite_migration::M::up(""); count];
        steps.push(rusqlite_migration::M::up(RECORDS));
        steps.push(rusqlite_migration::M::up(RECORD_PROVENANCE));
        steps.push(rusqlite_migration::M::up(RECORD_EXTRACTIONS));
        steps.push(rusqlite_migration::M::up(RECORD_KINDS));
        steps.push(rusqlite_migration::M::up(PUBLICATION_MESSAGES));
        connection.set_transaction_behavior(rusqlite::TransactionBehavior::Immediate);
        let result = rusqlite_migration::Migrations::new(steps).to_latest(connection);
        connection.set_transaction_behavior(rusqlite::TransactionBehavior::Deferred);
        result.map_err(|error| AppError::schema(format!("silver migration failed: {error}")))?;
    }
    Ok(())
}

/// The typed error for any silver schema this release does not write. A newer
/// schema usually means this process predates an upgrade that already moved
/// the index forward, such as an MCP server started before it.
pub fn unsupported(version: i64) -> AppError {
    if version > SCHEMA_VERSION {
        return AppError::schema(format!(
            "local index schema {version} is newer than this mash's schema {SCHEMA_VERSION}; a newer mash release wrote it. Reconnect Mash MCP or restart `mash index --watch` if mash was already upgraded; otherwise upgrade mash"
        ));
    }
    if (BASELINE..SCHEMA_VERSION).contains(&version) {
        return AppError::schema(format!(
            "local index schema {version} requires an in-place upgrade to {SCHEMA_VERSION}; run `mash index` with this release, then reconnect Mash MCP"
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
            // Whether a delta must resend the chat whole rather than amend
            // it: a chat created or removed, a tombstone, or a change to the
            // chat fields every search document of the chat carries.
            let whole = match (table, operation) {
                ("chats", "UPDATE") => {
                    "OLD.id IS NOT NEW.id OR OLD.provider IS NOT NEW.provider
                     OR OLD.project IS NOT NEW.project OR OLD.repository IS NOT NEW.repository"
                }
                ("chats" | "tombstones", _) => "1",
                _ => "0",
            };
            let mut changes = String::new();
            for reference in &references {
                let id = if table == "chats" {
                    format!("{reference}.id")
                } else {
                    format!("{reference}.chat_id")
                };
                write!(changes,
                    "INSERT INTO main.shared_changes(chat_id,revision,whole) SELECT {id},revision,({whole})<>0 FROM main.shared_publication WHERE singleton=1 AND {id} IS NOT NULL
                     ON CONFLICT(chat_id) DO UPDATE SET revision=excluded.revision,whole=MAX(whole,excluded.whole);"
                ).map_err(|_|AppError::internal("format publication trigger"))?;
                if table == "messages" {
                    write!(changes,
                        "INSERT INTO main.shared_message_changes(chat_id,message_id,revision) SELECT {reference}.chat_id,{reference}.id,revision FROM main.shared_publication WHERE singleton=1
                         ON CONFLICT(chat_id,message_id) DO UPDATE SET revision=excluded.revision;"
                    ).map_err(|_|AppError::internal("format publication trigger"))?;
                }
            }
            // Only published message columns journal a change; derived turn
            // membership and ingestion fingerprints never reach the server.
            let event = if (table, operation) == ("messages", "UPDATE") {
                "UPDATE OF id, chat_id, ordinal, role, context_parent, content_packed, search_projection, created_at, metadata, attachments"
            } else {
                operation
            };
            write!(
                statements,
                "CREATE TEMP TRIGGER IF NOT EXISTS shared_{table}_{operation}
                 AFTER {event} ON main.{table} BEGIN
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
