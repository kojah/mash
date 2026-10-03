use rusqlite::Connection;

use super::semantic_chunks::rebuild_all_semantic_chunks;
use crate::AppError;

pub(super) const SCHEMA_VERSION: i64 = 36;
mod cline;

const SCHEMA: &str = include_str!("schema.sql");

pub(super) fn initialize(connection: &Connection) -> Result<(), AppError> {
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(AppError::database)?;
    if version > SCHEMA_VERSION {
        return Err(AppError::schema(format!(
            "database schema version {version} is newer than supported version {SCHEMA_VERSION}"
        )));
    }

    connection
        .execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")
        .map_err(AppError::database)?;
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    if matches!(version, 32..=35) {
        return migrate_current_projections(connection, version);
    }

    connection
        .pragma_update(None, "foreign_keys", false)
        .map_err(AppError::database)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(AppError::database)?;
    transaction
        .execute_batch(SCHEMA)
        .map_err(AppError::database)?;
    add_column_if_missing(
        &transaction,
        "conversations",
        "source_fingerprint",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    prepare_message_columns(&transaction)?;
    add_column_if_missing(
        &transaction,
        "message_embeddings",
        "generation",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column_if_missing(
        &transaction,
        "message_embeddings",
        "norm",
        "REAL NOT NULL DEFAULT 0",
    )?;
    add_column_if_missing(
        &transaction,
        "derived_state",
        "semantic_ready_generation",
        "TEXT",
    )?;
    purge_unsupported_providers(&transaction)?;
    add_column_if_missing(&transaction, "conversations", "project", "TEXT")?;
    add_column_if_missing(&transaction, "conversations", "repository", "TEXT")?;
    migrate_source_identities(&transaction)?;
    install_silver_contract(&transaction)?;
    transaction.execute_batch("CREATE INDEX IF NOT EXISTS messages_context_parent ON messages(conversation_id, context_parent, ordinal) WHERE context_parent IS NOT NULL").map_err(AppError::database)?;
    if version < 8 {
        transaction
            .execute_batch(
                "DELETE FROM message_fts;
                 DELETE FROM message_embeddings;
                 DELETE FROM source_checkpoints;
                 UPDATE derived_state SET search_dirty = 1 WHERE singleton = 1;",
            )
            .map_err(AppError::database)?;
    }
    if version < 10 {
        backfill_semantic_readiness(&transaction)?;
    }
    if version < 11 {
        rebuild_all_semantic_chunks(&transaction)?;
    }
    if version < 12 {
        backfill_pending_embeddings(&transaction)?;
    }
    install_publication_triggers(&transaction)?;
    cline::migrate(&transaction)?;
    // Every earlier schema may retain the old projection. Invalidate after
    // legacy backfills so no migration can accidentally restore readiness.
    transaction.execute_batch(
        "DELETE FROM source_checkpoints;
         UPDATE derived_state SET search_dirty = 1, semantic_ready_generation = NULL WHERE singleton = 1;"
    ).map_err(AppError::database)?;
    transaction
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(AppError::database)?;
    transaction.commit().map_err(AppError::database)?;
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(AppError::database)
}

fn migrate_current_projections(connection: &Connection, version: i64) -> Result<(), AppError> {
    // Claude provenance requires explicit replay, not rebuilding unrelated text.
    let transaction = connection
        .unchecked_transaction()
        .map_err(AppError::database)?;
    transaction
        .execute_batch(SCHEMA)
        .map_err(AppError::database)?;
    install_silver_contract(&transaction)?;
    if version < 35 {
        cline::migrate(&transaction)?;
    }
    if version < 34 {
        transaction
            .execute(
                "DELETE FROM source_checkpoints WHERE provider = ?1",
                [crate::provider::Provider::ClaudeCode],
            )
            .map_err(AppError::database)?;
    }
    let certificate = super::generated::SilverCertificate::builder()
        .current_version(i64::from(
            crate::silver::certificate::INTERPRETATION_VERSION,
        ))
        .build()
        .query_one(&transaction)
        .map_err(AppError::database)?;
    if certificate.unverified != 0
        || certificate.pending != 0
        || certificate.incomplete != 0
        || certificate.rejected_sources != 0
    {
        // Preserve vectors and canonical history, but never invent a completed
        // ingestion certificate for a database predating this contract.
        transaction
            .execute(
                "UPDATE derived_state SET semantic_ready_generation=NULL,resume_generation=NULL,resume_silver_revision=NULL,resume_interpretation_version=NULL WHERE singleton=1",
                [],
            )
            .map_err(AppError::database)?;
    }
    transaction
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(AppError::database)?;
    transaction.commit().map_err(AppError::database)
}

fn prepare_message_columns(connection: &Connection) -> Result<(), AppError> {
    add_column_if_missing(
        connection,
        "messages",
        "fingerprint",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column_if_missing(connection, "messages", "search_projection", "TEXT")?;
    add_column_if_missing(
        connection,
        "messages",
        "context_parent",
        "INTEGER CHECK (context_parent IS NULL OR (context_parent >= -1 AND context_parent < ordinal))",
    )?;
    Ok(())
}

fn install_silver_contract(connection: &Connection) -> Result<(), AppError> {
    for (column, declaration) in [
        ("resume_generation", "TEXT"),
        ("resume_silver_revision", "INTEGER"),
        ("resume_interpretation_version", "INTEGER"),
    ] {
        add_column_if_missing(connection, "derived_state", column, declaration)?;
    }
    add_column_if_missing(
        connection,
        "source_checkpoints",
        "interpretation_version",
        "INTEGER NOT NULL DEFAULT 0 CHECK (interpretation_version >= 0)",
    )?;
    // Gold writes and scan bookkeeping do not change the canonical revision.
    for table in [
        "conversations",
        "messages",
        "tombstones",
        "claude_sources",
        "claude_source_messages",
    ] {
        for operation in ["INSERT", "UPDATE", "DELETE"] {
            connection
                .execute_batch(&format!(
                    "CREATE TRIGGER IF NOT EXISTS silver_{table}_{operation}
                 AFTER {operation} ON {table} BEGIN
                 UPDATE silver_state SET revision = revision + 1 WHERE singleton = 1;
                 END;"
                ))
                .map_err(AppError::database)?;
        }
    }
    Ok(())
}

fn install_publication_triggers(connection: &Connection) -> Result<(), AppError> {
    for table in [
        "conversations",
        "messages",
        "message_embeddings",
        "tombstones",
    ] {
        for operation in ["INSERT", "UPDATE", "DELETE"] {
            connection
                .execute_batch(&format!(
                    "CREATE TRIGGER IF NOT EXISTS shared_{table}_{operation}
                 AFTER {operation} ON {table} BEGIN
                 UPDATE shared_publication SET revision = revision + 1 WHERE singleton = 1;
                 END;"
                ))
                .map_err(AppError::database)?;
        }
    }
    Ok(())
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

fn backfill_semantic_readiness(connection: &Connection) -> Result<(), AppError> {
    connection
        .execute_batch(
            "UPDATE derived_state
                SET semantic_ready_generation = CASE
                    WHEN (SELECT count(DISTINCT generation)
                            FROM message_embeddings) = 1
                     AND NOT EXISTS (
                        SELECT 1
                          FROM messages m
                          LEFT JOIN message_embeddings e ON e.message_id = m.id
                         WHERE COALESCE(m.search_projection, m.content) <> ''
                           AND e.message_id IS NULL
                        UNION ALL
                        SELECT 1
                          FROM message_embeddings e
                          JOIN messages m ON m.id = e.message_id
                         WHERE COALESCE(m.search_projection, m.content) = ''
                     )
                    THEN (SELECT min(generation) FROM message_embeddings)
                    ELSE NULL
                END
              WHERE singleton = 1;",
        )
        .map_err(AppError::database)
}

fn migrate_source_identities(connection: &Connection) -> Result<(), AppError> {
    add_column_if_missing(
        connection,
        "conversations",
        "source_key",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column_if_missing(connection, "conversations", "source_session", "TEXT")?;
    connection
        .execute(
            "UPDATE conversations SET source_key = 'file:' || source_path WHERE source_key = ''",
            [],
        )
        .map_err(AppError::database)?;
    rebuild_history_schema(connection)?;
    migrate_source_checkpoints(connection)
}

fn rebuild_history_schema(connection: &Connection) -> Result<(), AppError> {
    connection
        .execute_batch(
            "CREATE TABLE conversations_next (
                id TEXT PRIMARY KEY,
                provider TEXT NOT NULL CHECK (provider IN (
                    'claude-code', 'codex', 'aider', 'hermes', 'opencode', 'openclaw', 'pi', 'copilot', 'gemini-cli', 'cline', 'cursor', 'windsurf', 'antigravity'
                )),
                source_path TEXT NOT NULL,
                title TEXT, created_at INTEGER, updated_at INTEGER,
                source_fingerprint TEXT NOT NULL DEFAULT '', project TEXT, repository TEXT,
                source_key TEXT NOT NULL, source_session TEXT,
                UNIQUE(provider, source_key)
             );
             CREATE TABLE messages_next (
                storage_id INTEGER PRIMARY KEY AUTOINCREMENT,
                id TEXT NOT NULL UNIQUE,
                conversation_id TEXT NOT NULL REFERENCES conversations_next(id) ON DELETE CASCADE,
                ordinal INTEGER NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL,
                search_projection TEXT, created_at INTEGER,
                context_parent INTEGER CHECK (context_parent IS NULL OR (context_parent >= -1 AND context_parent < ordinal)),
                fingerprint TEXT NOT NULL DEFAULT '',
                UNIQUE(conversation_id, ordinal)
             );
             CREATE TABLE message_embeddings_next (
                message_id TEXT PRIMARY KEY REFERENCES messages_next(id) ON DELETE CASCADE,
                generation TEXT NOT NULL DEFAULT '',
                dimensions INTEGER NOT NULL, norm REAL NOT NULL, vector BLOB NOT NULL
             );
             INSERT INTO conversations_next
                SELECT id, provider, source_path, title, created_at, updated_at, source_fingerprint, project, repository,
                       source_key, source_session
                FROM conversations;
             INSERT INTO messages_next
                SELECT rowid, id, conversation_id, ordinal, role, content, search_projection,
                       created_at, context_parent, fingerprint
                FROM messages;
             INSERT INTO message_embeddings_next(
                message_id, generation, dimensions, norm, vector
             ) SELECT message_id, generation, dimensions, norm, vector FROM message_embeddings;
             DROP TABLE message_fts;
             DROP TABLE message_embeddings;
             DROP TABLE messages;
             DROP TABLE conversations;
             ALTER TABLE conversations_next RENAME TO conversations;
             ALTER TABLE messages_next RENAME TO messages;
             ALTER TABLE message_embeddings_next RENAME TO message_embeddings;
             CREATE VIRTUAL TABLE message_fts USING fts5(
                content, message_id UNINDEXED, conversation_id UNINDEXED,
                tokenize = 'unicode61'
             );
             INSERT INTO message_fts(content, message_id, conversation_id)
                SELECT COALESCE(search_projection, content), id, conversation_id
                  FROM messages
                 WHERE COALESCE(search_projection, content) <> '';",
        )
        .map_err(AppError::database)
}

fn migrate_source_checkpoints(connection: &Connection) -> Result<(), AppError> {
    add_column_if_missing(
        connection,
        "source_checkpoints",
        "fingerprint",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column_if_missing(
        connection,
        "source_checkpoints",
        "source_key",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    connection.execute_batch(
        "CREATE TABLE source_checkpoints_next (
            provider TEXT NOT NULL, source_key TEXT NOT NULL, source_path TEXT NOT NULL,
            size_bytes INTEGER NOT NULL, modified_ns INTEGER NOT NULL, fingerprint TEXT NOT NULL DEFAULT '',
            PRIMARY KEY(provider, source_key)
         );
         INSERT INTO source_checkpoints_next
            SELECT provider, CASE WHEN source_key = '' THEN 'file:' || source_path ELSE source_key END,
                   source_path, size_bytes, modified_ns, fingerprint
              FROM source_checkpoints;
         DROP TABLE source_checkpoints;
         ALTER TABLE source_checkpoints_next RENAME TO source_checkpoints;"
    ).map_err(AppError::database)
}

fn add_column_if_missing(
    connection: &Connection,
    table: &str,
    column: &str,
    declaration: &str,
) -> Result<(), AppError> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(AppError::database)?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(AppError::database)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::database)?;
    if !names.iter().any(|name| name == column) {
        connection
            .execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} {declaration}"
            ))
            .map_err(AppError::database)?;
    }
    Ok(())
}

fn purge_unsupported_providers(connection: &Connection) -> Result<(), AppError> {
    connection
        .execute_batch(
            "DELETE FROM message_fts
              WHERE conversation_id IN (
                    SELECT id FROM conversations
                     WHERE provider NOT IN ('claude-code', 'codex', 'aider', 'hermes', 'opencode', 'openclaw', 'pi', 'copilot', 'gemini-cli', 'cline', 'cursor', 'windsurf', 'antigravity')
              );
             DELETE FROM message_embeddings
              WHERE message_id IN (
                    SELECT messages.id
                      FROM messages
                      JOIN conversations ON conversations.id = messages.conversation_id
                     WHERE conversations.provider NOT IN ('claude-code', 'codex', 'aider', 'hermes', 'opencode', 'openclaw', 'pi', 'copilot', 'gemini-cli', 'cline', 'cursor', 'windsurf', 'antigravity')
              );
             DELETE FROM pending_embeddings
              WHERE message_id IN (
                    SELECT messages.id
                      FROM messages
                      JOIN conversations ON conversations.id = messages.conversation_id
                     WHERE conversations.provider NOT IN ('claude-code', 'codex', 'aider', 'hermes', 'opencode', 'openclaw', 'pi', 'copilot', 'gemini-cli', 'cline', 'cursor', 'windsurf', 'antigravity')
              );
             DELETE FROM messages
              WHERE conversation_id IN (
                    SELECT id FROM conversations
                     WHERE provider NOT IN ('claude-code', 'codex', 'aider', 'hermes', 'opencode', 'openclaw', 'pi', 'copilot', 'gemini-cli', 'cline', 'cursor', 'windsurf', 'antigravity')
              );
             DELETE FROM conversations
              WHERE provider NOT IN ('claude-code', 'codex', 'aider', 'hermes', 'opencode', 'openclaw', 'pi', 'copilot', 'gemini-cli', 'cline', 'cursor', 'windsurf', 'antigravity');
             DELETE FROM source_checkpoints
              WHERE provider NOT IN ('claude-code', 'codex', 'aider', 'hermes', 'opencode', 'openclaw', 'pi', 'copilot', 'gemini-cli', 'cline', 'cursor', 'windsurf', 'antigravity');
             DELETE FROM tombstones
              WHERE provider NOT IN ('claude-code', 'codex', 'aider', 'hermes', 'opencode', 'openclaw', 'pi', 'copilot', 'gemini-cli', 'cline', 'cursor', 'windsurf', 'antigravity');",
        )
        .map_err(AppError::database)
}
