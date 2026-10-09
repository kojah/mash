use rusqlite::{OptionalExtension, params};

#[cfg(test)]
use super::SearchableMessage;
use super::semantic_chunks::{
    MISSING_CREATED_AT, SEMANTIC_CHUNK_ROWS, decode_f32_blob, decode_i64_blob, provider_code,
};
use super::{EmbeddingWrite, SemanticChunk, SemanticChunks, Storage, cutoff_timestamp};
use crate::AppError;

impl Storage {
    pub(crate) fn clear_dense_gold(&self) -> Result<(), AppError> {
        self.require_writer()?;
        self.connection.execute_batch("DELETE FROM connector_embeddings; DELETE FROM message_embeddings; DELETE FROM semantic_chunks; DELETE FROM dirty_semantic_chunks; DELETE FROM pending_embeddings; UPDATE derived_state SET semantic_ready_generation=NULL,resume_generation=NULL,resume_silver_revision=NULL,resume_interpretation_version=NULL WHERE singleton=1;")
            .map_err(AppError::database)
    }
    pub(crate) fn resume_unchanged_semantic_snapshot(
        &mut self,
        generation: &str,
    ) -> Result<(), AppError> {
        self.require_writer()?;
        if self.has_pending_embeddings()? || !self.semantic_coverage_is_complete(generation)? {
            return Ok(());
        }
        self.connection.execute(
            "UPDATE derived_state SET semantic_ready_generation=resume_generation
             WHERE singleton=1 AND search_dirty=0 AND resume_generation=?1
               AND resume_silver_revision=(SELECT revision FROM silver_state WHERE singleton=1)
               AND resume_interpretation_version=(SELECT interpretation_version FROM silver_state WHERE singleton=1)
               AND resume_interpretation_version=?2",
            params![generation, crate::silver::certificate::INTERPRETATION_VERSION],
        ).map_err(AppError::database)?;
        Ok(())
    }

    pub(crate) fn embedding_count(&self, generation: &str) -> Result<u64, AppError> {
        let count: i64 = self
            .connection
            .query_row(
                "SELECT count(*) FROM message_embeddings WHERE generation = ?1",
                [generation],
                |row| row.get(0),
            )
            .map_err(AppError::database)?;
        u64::try_from(count).map_err(|_| AppError::internal("negative embedding count"))
    }

    pub(crate) fn semantic_coverage_is_complete(&self, generation: &str) -> Result<bool, AppError> {
        self.semantic_coverage_for_selection(generation, self.dense_users_only()?)
    }

    fn dense_users_only(&self) -> Result<bool, AppError> {
        Ok(self.gold_manifest()?.is_some_and(|m| m.build.users_only()))
    }

    fn semantic_coverage_for_selection(
        &self,
        generation: &str,
        users_only: bool,
    ) -> Result<bool, AppError> {
        self.connection
            .query_row(
                // Unique message IDs make equal cardinality plus inclusion an
                // exact coverage check, including surplus/hidden vectors. The
                // partial identity index keeps both checks off message bodies.
                "SELECT
                 (SELECT count(*) FROM message_embeddings WHERE generation=?1) =
                 (SELECT count(*) FROM messages
                  WHERE COALESCE(search_projection, content) <> '' AND (?2=0 OR role='user'))
                 AND NOT EXISTS (
                    SELECT 1
                      FROM messages m INDEXED BY messages_searchable_identity
                      LEFT JOIN message_embeddings e
                        ON e.message_id = m.id AND e.generation = ?1
                     WHERE COALESCE(m.search_projection, m.content) <> ''
                       AND (?2 = 0 OR m.role = 'user')
                       AND e.message_id IS NULL
                 )",
                params![generation, users_only],
                |row| row.get(0),
            )
            .map_err(AppError::database)
    }

    pub(crate) fn semantic_index_is_ready(&self, generation: &str) -> Result<bool, AppError> {
        if !self.connector_embeddings_complete(generation)? {
            return Ok(false);
        }
        let ready_generation = self
            .connection
            .query_row(
                "SELECT semantic_ready_generation
                   FROM derived_state
                  WHERE singleton = 1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
            .map_err(AppError::database)?;
        if ready_generation.as_deref() == Some(generation) {
            let users_only = self.dense_users_only()?;
            return self
                .connection
                .query_row(
                    "SELECT
                        (SELECT count(*) FROM messages
                          WHERE COALESCE(search_projection, content) <> '' AND (?2=0 OR role='user')) =
                        (SELECT COALESCE(sum(vector_count), 0)
                           FROM semantic_chunks WHERE generation = ?1)",
                    params![generation, users_only],
                    |row| row.get(0),
                )
                .map_err(AppError::database);
        }
        Ok(false)
    }

    pub(crate) fn semantic_generation_is_published(
        &self,
        generation: &str,
    ) -> Result<bool, AppError> {
        self.connection
            .query_row(
                "SELECT semantic_ready_generation IS ?1
                   FROM derived_state
                  WHERE singleton = 1",
                [generation],
                |row| row.get(0),
            )
            .map_err(AppError::database)
    }

    #[cfg(test)]
    pub(crate) fn mark_semantic_index_ready(&mut self, generation: &str) -> Result<(), AppError> {
        self.mark_semantic_build_ready(generation, &crate::gold::config::Build::default())
    }

    pub(crate) fn restrict_dense_queue(
        &self,
        build: &crate::gold::config::Build,
    ) -> Result<(), AppError> {
        self.require_writer()?;
        if build.users_only() {
            // Drive the lookup from the queue: an incremental refresh may have
            // only a handful of entries in a large message archive.
            self.connection
                .execute(
                    "DELETE FROM pending_embeddings
                     WHERE EXISTS (
                         SELECT 1 FROM messages
                         WHERE messages.id = pending_embeddings.message_id
                           AND messages.role <> 'user'
                     )",
                    [],
                )
                .map_err(AppError::database)?;
        }
        Ok(())
    }

    pub(crate) fn mark_semantic_build_ready(
        &mut self,
        generation: &str,
        build: &crate::gold::config::Build,
    ) -> Result<(), AppError> {
        self.require_writer()?;
        if !self.semantic_coverage_for_selection(generation, build.users_only())? {
            return Err(AppError::search_not_ready(
                "semantic embedding coverage is incomplete",
            ));
        }
        self.finalize_semantic_chunks()?;
        let expected: i64 = self
            .connection
            .query_row(
                "SELECT count(*) FROM messages
                  WHERE COALESCE(search_projection, content) <> '' AND (?1=0 OR role='user')",
                [build.users_only()],
                |row| row.get(0),
            )
            .map_err(AppError::database)?;
        let packed: i64 = self
            .connection
            .query_row(
                "SELECT COALESCE(sum(vector_count), 0)
                   FROM semantic_chunks WHERE generation = ?1",
                [generation],
                |row| row.get(0),
            )
            .map_err(AppError::database)?;
        if packed != expected {
            return Err(AppError::search_not_ready(
                "packed semantic index coverage is incomplete",
            ));
        }
        self.connection
            .execute(
                "UPDATE derived_state
                    SET semantic_ready_generation = ?1
                  WHERE singleton = 1",
                [generation],
            )
            .map_err(AppError::database)?;
        Ok(())
    }

    pub(crate) fn mark_semantic_index_incomplete(&self) -> Result<(), AppError> {
        self.require_writer()?;
        self.connection
            .execute(
                "UPDATE derived_state
                    SET semantic_ready_generation = NULL
                  WHERE singleton = 1",
                [],
            )
            .map_err(AppError::database)?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn messages_needing_embeddings(&self) -> Result<Vec<SearchableMessage>, AppError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT messages.id, COALESCE(messages.search_projection, messages.content)
                   FROM pending_embeddings
                   JOIN messages ON messages.id = pending_embeddings.message_id
                  ORDER BY pending_embeddings.message_id",
            )
            .map_err(AppError::database)?;
        let rows = statement
            .query_map([], |row| {
                Ok(SearchableMessage {
                    id: row.get(0)?,
                    content: row.get(1)?,
                })
            })
            .map_err(AppError::database)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(AppError::database)
    }

    pub(crate) fn has_pending_embeddings(&self) -> Result<bool, AppError> {
        self.connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pending_embeddings LIMIT 1)",
                [],
                |row| row.get(0),
            )
            .map_err(AppError::database)
    }

    #[tracing::instrument(name = "write_embeddings", skip_all, fields(vectors = embeddings.len()))]
    pub(crate) fn replace_embeddings(
        &mut self,
        generation: &str,
        embeddings: &[EmbeddingWrite<'_>],
    ) -> Result<(), AppError> {
        let mut upsert = self
            .connection
            .prepare_cached(
                "INSERT INTO message_embeddings(
                    message_id, generation, dimensions, norm, vector
                 ) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(message_id) DO UPDATE SET
                    generation = excluded.generation,
                    dimensions = excluded.dimensions,
                    norm = excluded.norm,
                    vector = excluded.vector",
            )
            .map_err(AppError::database)?;
        let mut stage_chunk = self
            .connection
            .prepare_cached(
                "INSERT INTO dirty_semantic_chunks(chunk_id, generation)
                 SELECT (rowid - 1) / ?2, ?3 FROM messages WHERE id = ?1
                 ON CONFLICT(chunk_id) DO UPDATE SET generation = excluded.generation",
            )
            .map_err(AppError::database)?;
        let mut dequeue = self
            .connection
            .prepare_cached("DELETE FROM pending_embeddings WHERE message_id = ?1")
            .map_err(AppError::database)?;
        for embedding in embeddings {
            let dimensions = i64::try_from(embedding.vector.len())
                .map_err(|_| AppError::internal("embedding has too many dimensions"))?;
            upsert
                .execute(params![
                    embedding.message_id,
                    generation,
                    dimensions,
                    embedding.norm,
                    embedding.vector,
                ])
                .map_err(AppError::database)?;
            stage_chunk
                .execute(params![
                    embedding.message_id,
                    SEMANTIC_CHUNK_ROWS,
                    generation
                ])
                .map_err(AppError::database)?;
            dequeue
                .execute([embedding.message_id])
                .map_err(AppError::database)?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn semantic_chunks(
        &self,
        generation: &str,
        provider: Option<crate::provider::Provider>,
        days: Option<u32>,
    ) -> Result<SemanticChunks, AppError> {
        self.semantic_chunks_filtered(generation, provider, days, None)
    }

    pub(crate) fn semantic_chunks_filtered(
        &self,
        generation: &str,
        provider: Option<crate::provider::Provider>,
        days: Option<u32>,
        filters: Option<&crate::search_filter::Scope>,
    ) -> Result<SemanticChunks, AppError> {
        let filters = filters.filter(|f| !matches!(&f.filter, crate::search_filter::Filter::All(children) if children.is_empty()));
        let allowed = filters
            .map(|scope| self.filtered_semantic_rowids(scope))
            .transpose()?;
        let cutoff = cutoff_timestamp(days)?;
        let requested_provider = provider.map(provider_code);
        let mut statement = self
            .connection
            .prepare(
                "SELECT dimensions, vector_count, message_rowids, norms,
                        providers, created_ats, vectors
                   FROM semantic_chunks
                  WHERE generation = ?1
                  ORDER BY chunk_id",
            )
            .map_err(AppError::database)?;
        let mut rows = statement.query([generation]).map_err(AppError::database)?;
        let mut result = SemanticChunks {
            chunks: Vec::new(),
            dimensions: 0,
        };
        while let Some(row) = rows.next().map_err(AppError::database)? {
            let dimensions: i64 = row.get(0).map_err(AppError::database)?;
            let dimensions = usize::try_from(dimensions)
                .map_err(|_| AppError::database_data("negative embedding dimensions"))?;
            let vector_count: i64 = row.get(1).map_err(AppError::database)?;
            let vector_count = usize::try_from(vector_count)
                .map_err(|_| AppError::database_data("negative semantic chunk size"))?;
            let message_rowids = decode_i64_blob(
                &row.get::<_, Vec<u8>>(2).map_err(AppError::database)?,
                vector_count,
                "semantic chunk rowid",
            )?;
            let norms = decode_f32_blob(
                &row.get::<_, Vec<u8>>(3).map_err(AppError::database)?,
                vector_count,
                "semantic chunk norm",
            )?;
            if norms.iter().any(|norm| !norm.is_finite() || *norm < 0.0) {
                return Err(AppError::database_data(
                    "semantic chunk contains an invalid norm",
                ));
            }
            let providers: Vec<u8> = row.get(4).map_err(AppError::database)?;
            if providers.len() != vector_count
                || providers.iter().any(|code| {
                    !crate::provider::Provider::ALL
                        .iter()
                        .any(|provider| provider.storage_code() == *code)
                })
            {
                return Err(AppError::database_data(
                    "semantic chunk contains invalid provider metadata",
                ));
            }
            let created_ats = decode_i64_blob(
                &row.get::<_, Vec<u8>>(5).map_err(AppError::database)?,
                vector_count,
                "semantic chunk timestamp",
            )?;
            let values: Vec<u8> = row.get(6).map_err(AppError::database)?;
            let expected_values = vector_count
                .checked_mul(dimensions)
                .ok_or_else(|| AppError::database_data("semantic chunk size overflows"))?;
            if values.len() != expected_values {
                return Err(AppError::database_data(
                    "semantic chunk vector bytes do not match its dimensions",
                ));
            }
            if result.dimensions == 0 {
                result.dimensions = dimensions;
            } else if result.dimensions != dimensions {
                return Err(AppError::database_data(
                    "embedding rows have inconsistent dimensions",
                ));
            }
            let eligible = providers
                .iter()
                .zip(&created_ats)
                .zip(&message_rowids)
                .map(|((stored_provider, created_at), rowid)| {
                    allowed.as_ref().is_none_or(|ids| ids.contains(rowid))
                        && requested_provider.is_none_or(|wanted| wanted == *stored_provider)
                        && cutoff.is_none_or(|minimum| {
                            *created_at != MISSING_CREATED_AT && *created_at >= minimum
                        })
                })
                .collect();
            result.chunks.push(SemanticChunk {
                message_rowids,
                values,
                norms,
                eligible,
                dimensions,
            });
        }
        Ok(result)
    }

    fn filtered_semantic_rowids(
        &self,
        scope: &crate::search_filter::Scope,
    ) -> Result<std::collections::HashSet<i64>, AppError> {
        self.connection
            .prepare(&semantic_filter_query(scope))
            .map_err(AppError::database)?
            .query_map([], |row| row.get(0))
            .map_err(AppError::database)?
            .collect::<Result<_, _>>()
            .map_err(AppError::database)
    }

    pub(crate) fn invalidate_embedding_generation(
        &mut self,
        generation: &str,
    ) -> Result<u64, AppError> {
        self.require_writer()?;
        let ready_generation = self
            .connection
            .query_row(
                "SELECT semantic_ready_generation FROM derived_state WHERE singleton = 1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
            .map_err(AppError::database)?;
        if ready_generation.as_deref() == Some(generation) && !self.has_pending_embeddings()? {
            return Ok(0);
        }
        self.mark_semantic_index_incomplete()?;
        let stored_generation = self
            .connection
            .query_row(
                "SELECT generation FROM message_embeddings LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(AppError::database)?;
        if stored_generation
            .as_deref()
            .is_none_or(|stored| stored == generation)
        {
            return Ok(0);
        }
        let removed = self
            .connection
            .execute("DELETE FROM message_embeddings", [])
            .map_err(AppError::database)?;
        self.connection
            .execute_batch(
                "DELETE FROM semantic_chunks;
                 DELETE FROM dirty_semantic_chunks;
                 DELETE FROM pending_embeddings;
                 INSERT INTO pending_embeddings(message_id)
                    SELECT id FROM messages
                     WHERE COALESCE(search_projection, content) <> '';",
            )
            .map_err(AppError::database)?;
        u64::try_from(removed).map_err(|_| AppError::internal("too many stale embeddings"))
    }
}

pub(super) fn semantic_filter_query(scope: &crate::search_filter::Scope) -> String {
    let predicate = scope.filter.sql(Some(&scope.node));
    // Packed vectors contain only searchable messages. Match that eligibility
    // here. Require the metadata index to avoid scattered canonical-body reads
    // when applying time, role and conversation filters to a cold database.
    format!(
        "SELECT m.rowid FROM messages m INDEXED BY messages_searchable_metadata
         JOIN conversations c ON c.id=m.conversation_id
         WHERE COALESCE(m.search_projection, m.content) <> '' AND {predicate}"
    )
}
