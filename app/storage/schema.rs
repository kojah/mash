use rusqlite::{Connection, OptionalExtension};
use std::fmt::Write;

use std::path::{Path, PathBuf};

use crate::AppError;

/// Silver database version; any other version is rejected, never migrated.
pub(super) const SCHEMA_VERSION: i64 = 49;
const GOLD_VERSION: i64 = 3;

/// The one silver schema, shared by local nodes and the shared server.
pub(crate) const SILVER: &str = include_str!("silver.sql");
/// Silver indexes and triggers; they require every silver column to exist.
pub(crate) const SILVER_INDEXES: &str = include_str!("silver_indexes.sql");
const GOLD: &str = include_str!("gold.sql");

/// Create or validate a silver database without any local gold.
pub(crate) fn initialize_silver(connection: &Connection) -> Result<(), AppError> {
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(AppError::database)?;
    if version != 0 && version != SCHEMA_VERSION {
        return Err(AppError::schema(format!(
            "silver schema version {version} is not supported version {SCHEMA_VERSION}"
        )));
    }
    connection
        .execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")
        .map_err(AppError::database)?;
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

/// Open local silver and attach its paired gold database for writing. There
/// are no in-place migrations: another silver version is exported with the
/// release that wrote it and imported into a fresh database by this one.
pub(super) fn initialize(connection: &Connection, gold: &Path) -> Result<(), AppError> {
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(AppError::database)?;
    if version != 0 && version != SCHEMA_VERSION {
        return Err(unsupported(version));
    }
    connection
        .execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")
        .map_err(AppError::database)?;
    if version == 0 {
        initialize_silver(connection)?;
    }
    attach_gold(connection, gold, true)
}

/// The typed error for any silver schema this release does not write.
pub(super) fn unsupported(version: i64) -> AppError {
    AppError::schema(format!(
        "local index schema {version} is not this release's schema {SCHEMA_VERSION}; export it with the mash release that wrote it, then `mash import --keep-identity` it with this one"
    ))
}

/// Path of the gold database paired with a local silver database.
pub(crate) fn gold_path(silver: &Path) -> PathBuf {
    let name = silver
        .file_name()
        .map_or_else(|| "mash".into(), |name| name.to_string_lossy().into_owned());
    let gold = match name.strip_suffix(".sqlite3") {
        Some(stem) => format!("{stem}.gold.sqlite3"),
        None => format!("{name}.gold"),
    };
    silver.with_file_name(gold)
}

/// Remove a gold database and its journals; gold is always rebuildable.
pub(crate) fn remove_gold(gold: &Path) -> Result<(), AppError> {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut path = gold.as_os_str().to_owned();
        path.push(suffix);
        match std::fs::remove_file(PathBuf::from(path)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(AppError::io(error)),
        }
    }
    Ok(())
}

/// Gold DDL directed at one schema: the attached `gold` database, or `temp`
/// for a reader that has no usable gold.
fn qualified_gold(schema: &str) -> String {
    GOLD.replace(
        "CREATE TABLE IF NOT EXISTS ",
        &format!("CREATE TABLE IF NOT EXISTS {schema}."),
    )
    .replace(
        "CREATE VIRTUAL TABLE IF NOT EXISTS ",
        &format!("CREATE VIRTUAL TABLE IF NOT EXISTS {schema}."),
    )
    .replace(
        "INSERT OR IGNORE INTO ",
        &format!("INSERT OR IGNORE INTO {schema}."),
    )
}

fn silver_instance(connection: &Connection) -> Result<String, AppError> {
    connection
        .query_row(
            "SELECT instance FROM main.shared_publication WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .map_err(AppError::database)
}

/// Create empty gold for this silver: nothing is ready and every searchable
/// message awaits an embedding.
fn create_gold(connection: &Connection, schema: &str) -> Result<(), AppError> {
    let instance = silver_instance(connection)?;
    connection
        .execute_batch(&format!(
            "{}
             PRAGMA {schema}.user_version = {GOLD_VERSION};",
            qualified_gold(schema)
        ))
        .map_err(AppError::database)?;
    connection
        .execute(
            &format!(
                "INSERT INTO {schema}.silver_pairing(singleton, instance) VALUES (1, ?1)
                 ON CONFLICT(singleton) DO UPDATE SET instance = excluded.instance"
            ),
            [instance],
        )
        .map_err(AppError::database)?;
    Ok(())
}

fn attach(connection: &Connection, gold: &Path, writable: bool) -> Result<(), AppError> {
    let path = gold
        .to_str()
        .ok_or_else(|| AppError::configuration("gold database path must be valid UTF-8"))?;
    connection
        .execute("ATTACH DATABASE ?1 AS gold", [path])
        .map_err(AppError::database)?;
    if writable {
        connection
            .query_row("PRAGMA gold.journal_mode = WAL", [], |_| Ok(()))
            .map_err(AppError::database)?;
    }
    Ok(())
}

fn gold_usable(connection: &Connection) -> Result<bool, AppError> {
    let version: i64 = connection
        .query_row("PRAGMA gold.user_version", [], |row| row.get(0))
        .map_err(AppError::database)?;
    if version > GOLD_VERSION {
        return Err(AppError::schema(format!(
            "gold schema version {version} is newer than supported version {GOLD_VERSION}"
        )));
    }
    if version < GOLD_VERSION {
        return Ok(false);
    }
    let paired: Option<String> = connection
        .query_row(
            "SELECT instance FROM gold.silver_pairing WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(AppError::database)?;
    Ok(paired.is_some_and(|paired| silver_instance(connection).is_ok_and(|s| s == paired)))
}

/// Attach the gold database paired with this silver. A writer replaces
/// missing, foreign or unreadable gold with empty gold; a reader sees empty,
/// never-ready gold in its temporary schema instead.
pub(super) fn attach_gold(
    connection: &Connection,
    gold: &Path,
    writable: bool,
) -> Result<(), AppError> {
    let attached = if writable || gold.is_file() {
        attach(connection, gold, writable).is_ok()
    } else {
        false
    };
    let usable = attached && gold_usable(connection).unwrap_or(false);
    if !usable {
        if attached {
            connection
                .execute_batch("DETACH DATABASE gold")
                .map_err(AppError::database)?;
        }
        if writable {
            remove_gold(gold)?;
            attach(connection, gold, true)?;
            match_silver_permissions(connection, gold)?;
            let transaction = connection
                .unchecked_transaction()
                .map_err(AppError::database)?;
            create_gold(&transaction, "gold")?;
            backfill_pending_embeddings(&transaction)?;
            transaction.commit().map_err(AppError::database)?;
        } else {
            create_gold(connection, "temp")?;
        }
    }
    if writable {
        install_connection_triggers(connection)?;
    }
    Ok(())
}

/// Empty every gold artifact inside the writer's transaction, leaving every
/// searchable message pending: the explicit replay of gold from silver.
pub(super) fn discard_gold(connection: &Connection) -> Result<(), AppError> {
    connection
        .execute_batch(
            "DELETE FROM gold.message_embeddings;
             DELETE FROM gold.pending_embeddings;
             DELETE FROM gold.semantic_chunks;
             DELETE FROM gold.message_fts;
             DELETE FROM gold.connector_embeddings;
             DELETE FROM gold.connector_fts;
             DELETE FROM gold.connector_units;
             UPDATE gold.gold_manifest SET manifest = NULL;
             UPDATE gold.lexical_manifest SET manifest = NULL;
             UPDATE gold.derived_state SET search_dirty = 1, semantic_ready_generation = NULL,
                resume_generation = NULL, resume_silver_revision = NULL,
                resume_interpretation_version = NULL;",
        )
        .map_err(AppError::database)?;
    backfill_pending_embeddings(connection)
}

/// Gold holds derived copies of private history; it never gets wider access
/// than its silver database.
fn match_silver_permissions(connection: &Connection, gold: &Path) -> Result<(), AppError> {
    if let Some(silver) = connection.path().filter(|path| !path.is_empty()) {
        let permissions = std::fs::metadata(silver)
            .map_err(AppError::io)?
            .permissions();
        std::fs::set_permissions(gold, permissions).map_err(AppError::io)?;
    }
    Ok(())
}

/// Cross-database consequences of silver writes, installed on each writer:
/// gold rows follow their messages, and changes enter the publication journal.
fn install_connection_triggers(connection: &Connection) -> Result<(), AppError> {
    let mut statements = String::from(
        "CREATE TEMP TRIGGER IF NOT EXISTS gold_message_delete
         AFTER DELETE ON main.messages BEGIN
         DELETE FROM gold.message_embeddings WHERE message_id = OLD.id;
         DELETE FROM gold.pending_embeddings WHERE message_id = OLD.id;
         END;",
    );
    for (table, schema) in [
        ("conversations", "main"),
        ("messages", "main"),
        ("message_embeddings", "gold"),
        ("tombstones", "main"),
        ("conversation_enrichment", "main"),
    ] {
        for operation in ["INSERT", "UPDATE", "DELETE"] {
            let mut changes = String::new();
            for reference in match operation {
                "INSERT" => vec!["NEW"],
                "DELETE" => vec!["OLD"],
                _ => vec!["OLD", "NEW"],
            } {
                let id = match table {
                    "conversations" => format!("{reference}.id"),
                    "message_embeddings" => format!(
                        "(SELECT conversation_id FROM main.messages WHERE id={reference}.message_id)"
                    ),
                    _ => format!("{reference}.conversation_id"),
                };
                write!(changes,
                    "INSERT INTO main.shared_changes SELECT {id},revision FROM main.shared_publication WHERE singleton=1 AND {id} IS NOT NULL
                     ON CONFLICT(conversation_id) DO UPDATE SET revision=excluded.revision;"
                ).map_err(|_|AppError::internal("format publication trigger"))?;
            }
            write!(
                statements,
                "CREATE TEMP TRIGGER IF NOT EXISTS shared_{table}_{operation}
                 AFTER {operation} ON {schema}.{table} BEGIN
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

fn backfill_pending_embeddings(connection: &Connection) -> Result<(), AppError> {
    connection
        .execute_batch(
            "DELETE FROM pending_embeddings
              WHERE message_id NOT IN (SELECT id FROM messages);
             INSERT OR IGNORE INTO pending_embeddings(message_id)
             SELECT m.id
               FROM messages m
               LEFT JOIN message_embeddings e ON e.message_id = m.id
              WHERE COALESCE(m.search_projection, m.content) <> ''
                AND e.message_id IS NULL;",
        )
        .map_err(AppError::database)
}
