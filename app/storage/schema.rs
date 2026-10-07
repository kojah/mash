use rusqlite::{Connection, OptionalExtension};
use std::fmt::Write;
use std::path::{Path, PathBuf};

use crate::AppError;

/// Silver database version. Version 46 moved gold into its own database.
pub(super) const SCHEMA_VERSION: i64 = 46;
/// The last layout that kept silver and gold in one database file.
pub(super) const COMBINED_VERSION: i64 = 45;
const GOLD_VERSION: i64 = 1;

/// The one silver schema, shared by local nodes and the shared server.
pub(crate) const SILVER: &str = include_str!("silver.sql");
/// Silver indexes and triggers; they require every silver column to exist.
pub(crate) const SILVER_INDEXES: &str = include_str!("silver_indexes.sql");
const GOLD: &str = include_str!("gold.sql");
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

/// Open local silver and attach its paired gold database for writing. Fresh
/// databases are created split; the last single-file schema is split once.
pub(super) fn initialize(connection: &Connection, gold: &Path) -> Result<(), AppError> {
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(AppError::database)?;
    if version > SCHEMA_VERSION {
        return Err(AppError::schema(format!(
            "database schema version {version} is newer than supported version {SCHEMA_VERSION}"
        )));
    }
    if version != 0 && version < COMBINED_VERSION {
        return Err(AppError::schema(format!(
            "local index schema {version} is too old to upgrade; move it aside and run `mash index` to rebuild it from provider histories, or export it with the mash release that created it"
        )));
    }
    connection
        .execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")
        .map_err(AppError::database)?;
    if version == 0 {
        initialize_silver(connection)?;
    } else if version == COMBINED_VERSION {
        split_gold(connection, gold)?;
    }
    attach_gold(connection, gold, true)
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

/// Version 45 to 46: move every retrieval artifact into the gold database.
/// Gold commits first; an interruption before silver commits repeats the copy.
fn split_gold(connection: &Connection, gold: &Path) -> Result<(), AppError> {
    remove_gold(gold)?;
    attach(connection, gold, true)?;
    match_silver_permissions(connection, gold)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(AppError::database)?;
    create_gold(&transaction, "gold")?;
    transaction
        .execute_batch(
            "INSERT INTO gold.message_embeddings(message_id, generation, dimensions, norm, vector)
                SELECT message_id, generation, dimensions, norm, vector FROM main.message_embeddings;
             INSERT INTO gold.pending_embeddings(message_id)
                SELECT message_id FROM main.pending_embeddings;
             INSERT INTO gold.semantic_chunks(
                chunk_id, generation, dimensions, vector_count, message_rowids, norms,
                providers, created_ats, vectors
             ) SELECT chunk_id, generation, dimensions, vector_count, message_rowids, norms,
                      providers, created_ats, vectors
                 FROM main.semantic_chunks;
             INSERT INTO gold.message_fts(rowid, content, message_id, conversation_id)
                SELECT rowid, content, message_id, conversation_id FROM main.message_fts;
             UPDATE gold.gold_manifest
                SET manifest = (SELECT manifest FROM main.gold_manifest WHERE singleton = 1);
             UPDATE gold.lexical_manifest
                SET manifest = (SELECT manifest FROM main.lexical_manifest WHERE singleton = 1);
             UPDATE gold.derived_state SET
                search_dirty = (SELECT search_dirty FROM main.derived_state WHERE singleton = 1),
                semantic_ready_generation =
                    (SELECT semantic_ready_generation FROM main.derived_state WHERE singleton = 1),
                resume_generation = (SELECT resume_generation FROM main.derived_state WHERE singleton = 1),
                resume_silver_revision =
                    (SELECT resume_silver_revision FROM main.derived_state WHERE singleton = 1),
                resume_interpretation_version =
                    (SELECT resume_interpretation_version FROM main.derived_state WHERE singleton = 1);",
        )
        .map_err(AppError::database)?;
    transaction.commit().map_err(AppError::database)?;
    connection
        .execute_batch("DETACH DATABASE gold")
        .map_err(AppError::database)?;
    drop_combined_gold(connection)
}

/// Second half of the split: silver drops its copies of gold and adopts the
/// shared silver schema.
fn drop_combined_gold(connection: &Connection) -> Result<(), AppError> {
    connection
        .pragma_update(None, "foreign_keys", false)
        .map_err(AppError::database)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(AppError::database)?;
    let mut statements = String::new();
    for table in GOLD_TABLES.iter().chain(&["shared_forgets"]) {
        write!(statements, "DROP TABLE IF EXISTS main.{table};")
            .map_err(|_| AppError::internal("format gold split"))?;
    }
    for table in [
        "conversations",
        "messages",
        "message_embeddings",
        "tombstones",
        "conversation_enrichment",
    ] {
        for operation in ["INSERT", "UPDATE", "DELETE"] {
            write!(
                statements,
                "DROP TRIGGER IF EXISTS shared_{table}_{operation};"
            )
            .map_err(|_| AppError::internal("format gold split"))?;
        }
    }
    transaction
        .execute_batch(&statements)
        .map_err(AppError::database)?;
    // Server silver has no provider-native identity, so the key becomes optional.
    transaction
        .execute_batch(
            "CREATE TABLE conversations_next (
                id TEXT PRIMARY KEY,
                provider TEXT NOT NULL CHECK (
                    provider IN (
                        'claude-code', 'codex', 'opencode', 'copilot', 'cursor', 'antigravity'
                    )
                ),
                source_path TEXT NOT NULL,
                title TEXT,
                created_at INTEGER,
                updated_at INTEGER,
                source_fingerprint TEXT NOT NULL DEFAULT '',
                project TEXT,
                repository TEXT,
                source_key TEXT,
                source_session TEXT,
                thread_fingerprint TEXT,
                UNIQUE (provider, source_key)
             );
             INSERT INTO conversations_next(
                id, provider, source_path, title, created_at, updated_at, source_fingerprint,
                project, repository, source_key, source_session
             ) SELECT id, provider, source_path, title, created_at, updated_at, source_fingerprint,
                      project, repository, source_key, source_session
                 FROM conversations;
             DROP TABLE conversations;
             ALTER TABLE conversations_next RENAME TO conversations;",
        )
        .map_err(AppError::database)?;
    transaction
        .execute_batch(SILVER)
        .map_err(AppError::database)?;
    transaction
        .execute_batch(SILVER_INDEXES)
        .map_err(AppError::database)?;
    let violations: Option<String> = transaction
        .query_row(
            "SELECT \"table\" FROM pragma_foreign_key_check LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(AppError::database)?;
    if violations.is_some() {
        return Err(AppError::schema("silver references are inconsistent"));
    }
    transaction
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(AppError::database)?;
    transaction.commit().map_err(AppError::database)?;
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(AppError::database)
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
