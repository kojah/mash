use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use super::schema::SCHEMA_VERSION;
use super::{Counts, Message, SearchHit, StatusSnapshot, Storage, cutoff_timestamp};
use crate::AppError;

impl Storage {
    pub(crate) fn open_search(path: &Path, generation: &str) -> Result<Self, AppError> {
        if !path.is_file() {
            return Err(AppError::missing_database(path));
        }
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(AppError::database)?;
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(AppError::database)?;
        if version > SCHEMA_VERSION {
            return Err(AppError::schema(
                "index schema is newer than this version of mash",
            ));
        }
        if version < SCHEMA_VERSION {
            return Err(AppError::search_not_ready(
                "index requires an explicit rebuild; run `mash index`",
            ));
        }
        let storage = Self {
            connection,
            writer_active: false,
            defer_search_updates: false,
            transaction_base_messages: 0,
        };
        if storage.derived_search_is_dirty()? || !storage.semantic_index_is_ready(generation)? {
            return Err(AppError::search_not_ready(
                "initial indexing has not completed or the index needs updating; run `mash index`",
            ));
        }
        Ok(storage)
    }

    pub(crate) fn open_existing(path: &Path) -> Result<Self, AppError> {
        if !path.is_file() {
            return Err(AppError::missing_database(path));
        }
        Self::open(path)
    }

    pub(crate) fn status_snapshot(
        path: &Path,
        generation: &str,
    ) -> Result<StatusSnapshot, AppError> {
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(AppError::database)?;
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(AppError::database)?;
        if version > SCHEMA_VERSION {
            return Err(AppError::schema(format!(
                "database schema version {version} is newer than supported version {SCHEMA_VERSION}"
            )));
        }
        if version < SCHEMA_VERSION {
            return Ok(StatusSnapshot {
                counts: Counts {
                    conversations: readonly_table_count(&connection, "conversations")?,
                    messages: readonly_table_count(&connection, "messages")?,
                    searchable_messages: readonly_table_count(&connection, "messages")?,
                    embeddings: readonly_table_count(&connection, "message_embeddings")?,
                },
                current_embeddings: 0,
                derived_clean: false,
                exact_semantic_coverage: false,
            });
        }
        let storage = Self {
            connection,
            writer_active: false,
            defer_search_updates: false,
            transaction_base_messages: 0,
        };
        Ok(StatusSnapshot {
            counts: storage.counts()?,
            current_embeddings: storage.embedding_count(generation)?,
            derived_clean: !storage.derived_search_is_dirty()?,
            exact_semantic_coverage: storage.semantic_index_is_ready(generation)?,
        })
    }

    pub(crate) fn counts(&self) -> Result<Counts, AppError> {
        let conversations: i64 = self
            .connection
            .query_row("SELECT count(*) FROM conversations", [], |row| row.get(0))
            .map_err(AppError::database)?;
        let messages: i64 = self
            .connection
            .query_row("SELECT count(*) FROM messages", [], |row| row.get(0))
            .map_err(AppError::database)?;
        let embeddings: i64 = self
            .connection
            .query_row("SELECT count(*) FROM message_embeddings", [], |row| {
                row.get(0)
            })
            .map_err(AppError::database)?;
        let searchable_messages: i64 = self
            .connection
            .query_row(
                "SELECT count(*) FROM messages
                  WHERE COALESCE(search_projection, content) <> ''",
                [],
                |row| row.get(0),
            )
            .map_err(AppError::database)?;
        Ok(Counts {
            conversations: u64::try_from(conversations)
                .map_err(|_| AppError::internal("negative conversation count"))?,
            messages: u64::try_from(messages)
                .map_err(|_| AppError::internal("negative message count"))?,
            searchable_messages: u64::try_from(searchable_messages)
                .map_err(|_| AppError::internal("negative searchable message count"))?,
            embeddings: u64::try_from(embeddings)
                .map_err(|_| AppError::internal("negative embedding count"))?,
        })
    }

    #[cfg(test)]
    pub(crate) fn search(
        &self,
        query: &str,
        limit: usize,
        provider: Option<crate::provider::Provider>,
        days: Option<u32>,
    ) -> Result<Vec<SearchHit>, AppError> {
        self.search_filtered(query, limit, provider, days, None)
    }

    pub(crate) fn search_filtered(
        &self,
        query: &str,
        limit: usize,
        provider: Option<crate::provider::Provider>,
        days: Option<u32>,
        filters: Option<&crate::search_filter::Scope>,
    ) -> Result<Vec<SearchHit>, AppError> {
        if query.trim().is_empty() {
            return Err(AppError::usage("search query must not be empty"));
        }
        let limit = i64::try_from(limit.min(1_000)).unwrap_or(1_000);
        let cutoff = cutoff_timestamp(days)?;
        let predicate = filters.map_or_else(|| "TRUE".to_owned(), |f| f.filter.sql(Some(&f.node)));
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT m.id, m.conversation_id, c.provider, m.role, m.content,
                        bm25(message_fts) AS lexical_score
                   FROM message_fts
                   JOIN messages m ON m.id = message_fts.message_id
                   JOIN conversations c ON c.id = m.conversation_id
                  WHERE message_fts MATCH ?1
                    AND (?2 IS NULL OR c.provider = ?2)
                    AND (?3 IS NULL OR m.created_at >= ?3)
                    AND ({predicate})
                  ORDER BY lexical_score, m.id
                  LIMIT ?4"
            ))
            .map_err(AppError::database)?;
        let rows = statement
            .query_map(params![query, provider, cutoff, limit], |row| {
                Ok(SearchHit {
                    id: row.get(0)?,
                    conversation_id: row.get(1)?,
                    provider: row.get(2)?,
                    role: row.get(3)?,
                    content: row.get(4)?,
                    lexical_score: Some(row.get(5)?),
                    semantic_score: None,
                    fusion_score: 0.0,
                    rerank_score: None,
                    node_id: None,
                })
            })
            .map_err(AppError::database)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(AppError::database)
    }

    pub(crate) fn search_hits(&self, message_rowids: &[i64]) -> Result<Vec<SearchHit>, AppError> {
        let mut statement = self
            .connection
            .prepare_cached(
                "SELECT m.id, m.conversation_id, c.provider, m.role, m.content
                   FROM messages m
                   JOIN conversations c ON c.id = m.conversation_id
                  WHERE m.rowid = ?1",
            )
            .map_err(AppError::database)?;
        let mut hits = Vec::with_capacity(message_rowids.len());
        for message_rowid in message_rowids {
            let hit = statement
                .query_row([message_rowid], |row| {
                    Ok(SearchHit {
                        id: row.get(0)?,
                        conversation_id: row.get(1)?,
                        provider: row.get(2)?,
                        role: row.get(3)?,
                        content: row.get(4)?,
                        lexical_score: None,
                        semantic_score: None,
                        fusion_score: 0.0,
                        rerank_score: None,
                        node_id: None,
                    })
                })
                .optional()
                .map_err(AppError::database)?;
            let hit = hit.ok_or_else(|| {
                AppError::database_data("semantic index references a missing message")
            })?;
            hits.push(hit);
        }
        Ok(hits)
    }

    pub(crate) fn search_documents(&self, message_ids: &[&str]) -> Result<Vec<String>, AppError> {
        let mut statement = self
            .connection
            .prepare_cached(
                "SELECT COALESCE(search_projection, content)
                   FROM messages
                  WHERE id = ?1",
            )
            .map_err(AppError::database)?;
        message_ids
            .iter()
            .map(|message_id| {
                statement
                    .query_row([message_id], |row| row.get(0))
                    .map_err(AppError::database)
            })
            .collect()
    }

    pub(crate) fn view(&self, id: &str, context: u32) -> Result<Vec<Message>, AppError> {
        let anchor: Option<(String, i64)> = self
            .connection
            .query_row(
                "SELECT conversation_id, ordinal FROM messages WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(AppError::database)?;
        let Some((conversation_id, ordinal)) = anchor else {
            return Ok(Vec::new());
        };
        let context = i64::from(context);
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, ordinal, role, content, created_at
                   FROM messages
                  WHERE conversation_id = ?1
                    AND ordinal BETWEEN ?2 AND ?3
                  ORDER BY ordinal",
            )
            .map_err(AppError::database)?;
        let rows = statement
            .query_map(
                params![
                    conversation_id,
                    ordinal.saturating_sub(context),
                    ordinal.saturating_add(context)
                ],
                |row| {
                    Ok(Message {
                        id: row.get(0)?,
                        ordinal: row.get(1)?,
                        role: row.get(2)?,
                        content: row.get(3)?,
                        created_at: row.get(4)?,
                    })
                },
            )
            .map_err(AppError::database)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(AppError::database)
    }
}

fn readonly_table_count(connection: &Connection, table: &str) -> Result<u64, AppError> {
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1)",
            [table],
            |row| row.get(0),
        )
        .map_err(AppError::database)?;
    if !exists {
        return Ok(0);
    }
    let sql = format!("SELECT count(*) FROM {table}");
    let count: i64 = connection
        .query_row(&sql, [], |row| row.get(0))
        .map_err(AppError::database)?;
    u64::try_from(count).map_err(|_| AppError::internal("negative table count"))
}
