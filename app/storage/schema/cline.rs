//! Repair stored Cline user projections without replaying or re-embedding other providers.
use super::super::semantic_chunks::{SEMANTIC_CHUNK_ROWS, rebuild_semantic_chunk};
use crate::{AppError, provider::Provider, role::Role};
use rusqlite::{Connection, OptionalExtension, params};

pub(super) fn migrate(connection: &Connection) -> Result<(), AppError> {
    connection
        .execute_batch("CREATE TEMP TABLE cline_projection_chunks(chunk_id INTEGER PRIMARY KEY)")
        .map_err(AppError::database)?;
    let mut next = connection
        .prepare(
            "SELECT m.storage_id, m.id, m.conversation_id,
                COALESCE(m.search_projection,m.content) AS projection
         FROM messages m JOIN conversations c ON c.id=m.conversation_id
         WHERE m.storage_id>?1 AND c.provider=?2 AND m.role=?3
           AND COALESCE(m.search_projection,m.content)<>''
         ORDER BY m.storage_id LIMIT 1",
        )
        .map_err(AppError::database)?;
    let mut cursor = 0;
    let mut changed = false;
    // Keyset traversal retains only one source text. Updating a live query's
    // predicate could otherwise make iteration over changed projections ambiguous.
    while let Some((rowid, id, conversation, before)) = next
        .query_row(params![cursor, Provider::Cline, Role::User], |row| {
            Ok((
                row.get::<_, i64>("storage_id")?,
                row.get::<_, String>("id")?,
                row.get::<_, String>("conversation_id")?,
                row.get::<_, String>("projection")?,
            ))
        })
        .optional()
        .map_err(AppError::database)?
    {
        cursor = rowid;
        let after = crate::ingestion::cline_search_projection(&Role::User, &before);
        if before == after {
            continue;
        }
        connection
            .execute(
                "UPDATE messages SET search_projection=?1, fingerprint='' WHERE id=?2",
                params![after, id],
            )
            .map_err(AppError::database)?;
        connection
            .execute("DELETE FROM message_fts WHERE message_id=?1", [&id])
            .map_err(AppError::database)?;
        if after.is_empty() {
            connection
                .execute("DELETE FROM pending_embeddings WHERE message_id=?1", [&id])
                .map_err(AppError::database)?;
        } else {
            connection
                .execute(
                    "INSERT INTO message_fts(content,message_id,conversation_id) VALUES(?1,?2,?3)",
                    params![after, id, conversation],
                )
                .map_err(AppError::database)?;
            connection
                .execute(
                    "INSERT OR IGNORE INTO pending_embeddings(message_id) VALUES(?1)",
                    [&id],
                )
                .map_err(AppError::database)?;
        }
        connection
            .execute("DELETE FROM message_embeddings WHERE message_id=?1", [&id])
            .map_err(AppError::database)?;
        connection
            .execute(
                "INSERT OR IGNORE INTO cline_projection_chunks VALUES(?1)",
                [(rowid - 1) / SEMANTIC_CHUNK_ROWS],
            )
            .map_err(AppError::database)?;
        changed = true;
    }
    drop(next);
    if changed {
        let mut chunks = connection
            .prepare("SELECT chunk_id FROM cline_projection_chunks ORDER BY chunk_id")
            .map_err(AppError::database)?;
        let mut rows = chunks.query([]).map_err(AppError::database)?;
        while let Some(row) = rows.next().map_err(AppError::database)? {
            rebuild_semantic_chunk(
                connection,
                row.get("chunk_id").map_err(AppError::database)?,
                None,
            )?;
        }
        connection
            .execute(
                "DELETE FROM source_checkpoints WHERE provider=?1",
                [Provider::Cline],
            )
            .map_err(AppError::database)?;
        connection
            .execute(
                "UPDATE derived_state SET semantic_ready_generation=NULL WHERE singleton=1",
                [],
            )
            .map_err(AppError::database)?;
    }
    connection
        .execute_batch("DROP TABLE cline_projection_chunks")
        .map_err(AppError::database)
}
