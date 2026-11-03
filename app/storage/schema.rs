use rusqlite::{Connection, OptionalExtension};
use std::fmt::Write;

use std::path::{Path, PathBuf};

use crate::AppError;
use crate::database_error::Operation;

/// Silver database version; any other version is rejected, never migrated.
pub(crate) const SCHEMA_VERSION: i64 = 54;
pub(crate) const VIEWS_VERSION: i64 = 4;

/// The one silver schema, shared by local nodes and the shared server.
pub(crate) const SILVER: &str = include_str!("silver.sql");
/// Silver indexes and triggers; they require every silver column to exist.
pub(crate) const SILVER_INDEXES: &str = include_str!("silver_indexes.sql");
const VIEWS: &str = include_str!("views.sql");
/// Writers run in WAL mode with NORMAL sync: commits are atomic and survive a
/// process crash without an fsync each; only an OS crash or power loss can
/// drop the latest commits, and the database stays consistent. Per connection.
const WRITER_PRAGMAS: &str =
    "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;";

/// Create or validate a silver database without any local views.
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

/// Open local silver and attach its paired views database for writing. There
/// are no in-place migrations: another silver version is exported with the
/// release that wrote it and imported into a fresh database by this one.
pub(super) fn initialize(connection: &Connection, views: &Path) -> Result<(), AppError> {
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
        initialize_silver(connection)?;
    }
    attach_views(connection, views, true)
}

/// The typed error for any silver schema this release does not write. A newer
/// schema usually means this process predates an upgrade that already moved
/// the index forward, such as an MCP server started before it.
pub(super) fn unsupported(version: i64) -> AppError {
    if version > SCHEMA_VERSION {
        return AppError::schema(format!(
            "local index schema {version} is newer than this mash's schema {SCHEMA_VERSION}; a newer mash release wrote it. Reconnect Mash MCP or restart `mash index --watch` if mash was already upgraded; otherwise upgrade mash"
        ));
    }
    AppError::schema(format!(
        "local index schema {version} is older than this mash's schema {SCHEMA_VERSION}; export it with the mash release that wrote it, then `mash import --keep-identity` it with this one"
    ))
}

/// Path of the views database paired with a local silver database.
pub(crate) fn views_path(silver: &Path) -> PathBuf {
    let name = silver
        .file_name()
        .map_or_else(|| "mash".into(), |name| name.to_string_lossy().into_owned());
    let views = match name.strip_suffix(".sqlite3") {
        Some(stem) => format!("{stem}.views.sqlite3"),
        None => format!("{name}.views"),
    };
    silver.with_file_name(views)
}

/// Remove a views database and its journals; views are always rebuildable.
pub(crate) fn remove_views(views: &Path) -> Result<(), AppError> {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut path = views.as_os_str().to_owned();
        path.push(suffix);
        match std::fs::remove_file(PathBuf::from(path)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(AppError::io(error)),
        }
    }
    Ok(())
}

/// View DDL directed at one schema: the attached `views` database, or `temp`
/// for a reader that has no usable views.
fn qualified_views(schema: &str) -> String {
    VIEWS
        .replace(
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

/// Create empty views for this silver: nothing is ready and every searchable
/// message awaits an embedding.
fn create_views(connection: &Connection, schema: &str) -> Result<(), AppError> {
    let instance = silver_instance(connection)?;
    connection
        .execute_batch(&format!(
            "{}
             PRAGMA {schema}.user_version = {VIEWS_VERSION};",
            qualified_views(schema)
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

fn attach(connection: &Connection, views: &Path, writable: bool) -> Result<(), AppError> {
    let path = views
        .to_str()
        .ok_or_else(|| AppError::configuration("views database path must be valid UTF-8"))?;
    connection
        .execute("ATTACH DATABASE ?1 AS views", [path])
        .map_err(AppError::database_at(Operation::AttachViews))?;
    if writable {
        connection
            .query_row("PRAGMA views.journal_mode = WAL", [], |_| Ok(()))
            .map_err(AppError::database_at(Operation::ConfigureWriter))?;
        connection
            .execute_batch("PRAGMA views.synchronous = NORMAL;")
            .map_err(AppError::database_at(Operation::ConfigureWriter))?;
    }
    Ok(())
}

fn views_usable(connection: &Connection) -> Result<bool, AppError> {
    let version: i64 = connection
        .query_row("PRAGMA views.user_version", [], |row| row.get(0))
        .map_err(AppError::database)?;
    if version > VIEWS_VERSION {
        return Err(AppError::schema(format!(
            "views schema version {version} is newer than supported version {VIEWS_VERSION}"
        )));
    }
    if version < VIEWS_VERSION {
        return Ok(false);
    }
    let paired: Option<String> = connection
        .query_row(
            "SELECT instance FROM views.silver_pairing WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(AppError::database)?;
    Ok(paired.is_some_and(|paired| silver_instance(connection).is_ok_and(|s| s == paired)))
}

/// Attach the views database paired with this silver. A writer replaces
/// missing, foreign or unreadable views with empty views; a reader sees empty,
/// never-ready views in its temporary schema instead.
pub(super) fn attach_views(
    connection: &Connection,
    views: &Path,
    writable: bool,
) -> Result<(), AppError> {
    let attached = (writable || views.is_file()) && attach(connection, views, writable).is_ok();
    let usable = attached && views_usable(connection).unwrap_or(false);
    if !usable {
        if attached {
            connection
                .execute_batch("DETACH DATABASE views")
                .map_err(AppError::database)?;
        }
        if writable {
            remove_views(views)?;
            attach(connection, views, true)?;
            match_silver_permissions(connection, views)?;
            let transaction = connection
                .unchecked_transaction()
                .map_err(AppError::database)?;
            create_views(&transaction, "views")?;
            backfill_pending_embeddings(&transaction)?;
            transaction.commit().map_err(AppError::database)?;
        } else {
            create_views(connection, "temp")?;
        }
    }
    if writable {
        install_connection_triggers(connection)?;
    }
    Ok(())
}

/// Empty every views artifact inside the writer's transaction, leaving every
/// searchable message pending: the explicit replay of views from silver.
pub(super) fn discard_views(connection: &Connection) -> Result<(), AppError> {
    connection
        .execute_batch(
            "DELETE FROM views.message_embeddings;
             DELETE FROM views.pending_embeddings;
             DELETE FROM views.semantic_chunks;
             DELETE FROM views.message_fts;
             DELETE FROM views.connector_embeddings;
             DELETE FROM views.connector_fts;
             DELETE FROM views.connector_units;
             UPDATE views.views_manifest SET manifest = NULL;
             UPDATE views.lexical_manifest SET manifest = NULL;
             UPDATE views.derived_state SET search_dirty = 1, semantic_ready_generation = NULL,
                resume_generation = NULL, resume_silver_revision = NULL,
                resume_interpretation_version = NULL;",
        )
        .map_err(AppError::database)?;
    backfill_pending_embeddings(connection)
}

/// Views hold derived copies of private history; it never gets wider access
/// than its silver database.
fn match_silver_permissions(connection: &Connection, views: &Path) -> Result<(), AppError> {
    if let Some(silver) = connection.path().filter(|path| !path.is_empty()) {
        let permissions = std::fs::metadata(silver)
            .map_err(AppError::io)?
            .permissions();
        std::fs::set_permissions(views, permissions).map_err(AppError::io)?;
    }
    Ok(())
}

/// Cross-database consequences of silver writes, installed on each writer:
/// views rows follow their messages, and changes enter the publication journal.
fn install_connection_triggers(connection: &Connection) -> Result<(), AppError> {
    let mut statements = String::from(
        "CREATE TEMP TRIGGER IF NOT EXISTS views_message_delete
         AFTER DELETE ON main.objects WHEN OLD.origin = 'provider' BEGIN
         DELETE FROM views.message_embeddings WHERE message_id = OLD.id;
         DELETE FROM views.pending_embeddings WHERE message_id = OLD.id;
         END;",
    );
    // Node publication carries native history only; connector objects reach
    // shared silver through their own ingress.
    for (table, schema, when) in [
        ("objects", "main", Some("origin = 'provider'")),
        ("native_chats", "main", None),
        ("native_messages", "main", None),
        ("message_embeddings", "views", None),
        ("tombstones", "main", None),
        ("conversation_enrichment", "main", None),
    ] {
        for operation in ["INSERT", "UPDATE", "DELETE"] {
            let references = match operation {
                "INSERT" => vec!["NEW"],
                "DELETE" => vec!["OLD"],
                _ => vec!["OLD", "NEW"],
            };
            let mut changes = String::new();
            for reference in &references {
                let id = match table {
                    "native_messages" => format!(
                        "(SELECT conversation_id FROM main.objects WHERE storage_id={reference}.storage_id)"
                    ),
                    "message_embeddings" => format!(
                        "(SELECT conversation_id FROM main.objects WHERE origin='provider' AND object_role='entry' AND id={reference}.message_id)"
                    ),
                    _ => format!("{reference}.conversation_id"),
                };
                write!(changes,
                    "INSERT INTO main.shared_changes SELECT {id},revision FROM main.shared_publication WHERE singleton=1 AND {id} IS NOT NULL
                     ON CONFLICT(conversation_id) DO UPDATE SET revision=excluded.revision;"
                ).map_err(|_|AppError::internal("format publication trigger"))?;
            }
            let when = when.map_or_else(String::new, |when| {
                let condition = references
                    .iter()
                    .map(|reference| format!("{reference}.{when}"))
                    .collect::<Vec<_>>()
                    .join(" OR ");
                format!("WHEN {condition}")
            });
            write!(
                statements,
                "CREATE TEMP TRIGGER IF NOT EXISTS shared_{table}_{operation}
                 AFTER {operation} ON {schema}.{table} {when} BEGIN
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
              WHERE message_id NOT IN
                (SELECT id FROM objects WHERE origin = 'provider' AND object_role = 'entry');
             INSERT OR IGNORE INTO pending_embeddings(message_id)
             SELECT m.id
               FROM objects m
               LEFT JOIN message_embeddings e ON e.message_id = m.id
              WHERE m.origin = 'provider' AND m.object_role = 'entry'
                AND COALESCE(m.search_projection, m.content) <> ''
                AND e.message_id IS NULL;",
        )
        .map_err(AppError::database)
}
