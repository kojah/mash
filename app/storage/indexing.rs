use crate::provider::Provider;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rusqlite::{OptionalExtension, params};

use super::generated as sql;
use super::semantic_chunks::{SEMANTIC_CHUNK_ROWS, rebuild_semantic_chunk};
use super::{ConversationChange, FtsRefreshStrategy, SearchUpdates, Storage};
use crate::AppError;
use crate::silver::Conversation;

const FTS_BULK_REBUILD_PERCENT: u64 = 90;

#[cfg(test)]
mod memory;

struct MessageState {
    fingerprint: String,
    ordinal: i64,
    search_text_hash: blake3::Hash,
}

impl Storage {
    pub(crate) fn begin_provider_scan(&self) -> Result<(), AppError> {
        self.require_writer()?;
        self.connection
            .execute_batch("SAVEPOINT mash_provider_scan")
            .map_err(AppError::database)
    }

    pub(crate) fn finish_provider_scan(&self) -> Result<(), AppError> {
        self.connection
            .execute_batch("RELEASE mash_provider_scan")
            .map_err(AppError::database)
    }

    pub(crate) fn rollback_provider_scan(&self) -> Result<(), AppError> {
        self.connection
            .execute_batch(
                "ROLLBACK TO mash_provider_scan;
                 RELEASE mash_provider_scan;",
            )
            .map_err(AppError::database)
    }

    pub(super) fn measured_fts_bulk_threshold(&self) -> Result<u64, AppError> {
        let messages = self.message_count()?.max(self.transaction_base_messages);
        Ok(messages
            .saturating_mul(FTS_BULK_REBUILD_PERCENT)
            .saturating_add(99)
            / 100)
    }

    pub(super) fn message_count(&self) -> Result<u64, AppError> {
        let messages: i64 = self
            .connection
            .query_row("SELECT count(*) FROM messages", [], |row| row.get(0))
            .map_err(AppError::database)?;
        u64::try_from(messages).map_err(|_| AppError::internal("negative message count"))
    }

    pub(super) fn finalize_pending_fts_updates(
        &mut self,
        bulk_rebuild_threshold: u64,
    ) -> Result<FtsRefreshStrategy, AppError> {
        self.require_writer()?;
        if self.search_updates != SearchUpdates::Immediate {
            return Ok(FtsRefreshStrategy::Deferred);
        }
        let changed: i64 = self
            .connection
            .query_row("SELECT count(*) FROM pending_fts_messages", [], |row| {
                row.get(0)
            })
            .map_err(AppError::database)?;
        let changed = u64::try_from(changed)
            .map_err(|_| AppError::internal("negative pending FTS message count"))?;
        if changed == 0 {
            return Ok(FtsRefreshStrategy::None);
        }
        let strategy = if changed >= bulk_rebuild_threshold.max(1) {
            self.rebuild_fts(false)?;
            FtsRefreshStrategy::Bulk
        } else {
            self.connection
                .execute_batch(
                    "DELETE FROM message_fts
                      WHERE message_id IN (SELECT message_id FROM pending_fts_messages);
                     INSERT INTO message_fts(content, message_id, conversation_id)
                     SELECT COALESCE(search_projection, content), id, conversation_id
                       FROM messages
                      WHERE id IN (SELECT message_id FROM pending_fts_messages)
                        AND COALESCE(search_projection, content) <> '';
                     DELETE FROM pending_fts_messages;",
                )
                .map_err(AppError::database)?;
            FtsRefreshStrategy::Incremental
        };
        Ok(strategy)
    }

    fn stage_semantic_message(
        &self,
        message_id: &str,
        generation: Option<&str>,
    ) -> Result<(), AppError> {
        self.connection
            .execute(
                "INSERT INTO dirty_semantic_chunks(chunk_id, generation)
                 SELECT (m.rowid - 1) / ?2, COALESCE(?3, e.generation)
                   FROM messages m
                   LEFT JOIN message_embeddings e ON e.message_id = m.id
                  WHERE m.id = ?1
                 ON CONFLICT(chunk_id) DO UPDATE SET
                    generation = COALESCE(excluded.generation, generation)",
                params![message_id, SEMANTIC_CHUNK_ROWS, generation],
            )
            .map_err(AppError::database)?;
        Ok(())
    }

    fn stage_semantic_conversation(&self, conversation_id: &str) -> Result<(), AppError> {
        self.connection
            .execute(
                "INSERT INTO dirty_semantic_chunks(chunk_id, generation)
                 SELECT (m.rowid - 1) / ?2, min(e.generation)
                   FROM messages m
                   LEFT JOIN message_embeddings e ON e.message_id = m.id
                  WHERE m.conversation_id = ?1
                  GROUP BY (m.rowid - 1) / ?2
                 ON CONFLICT(chunk_id) DO UPDATE SET
                    generation = COALESCE(excluded.generation, generation)",
                params![conversation_id, SEMANTIC_CHUNK_ROWS],
            )
            .map_err(AppError::database)?;
        Ok(())
    }

    pub(super) fn finalize_semantic_chunks(&self) -> Result<(), AppError> {
        if self.search_updates != SearchUpdates::Immediate {
            return Ok(());
        }
        let mut statement = self
            .connection
            .prepare("SELECT chunk_id, generation FROM dirty_semantic_chunks ORDER BY chunk_id")
            .map_err(AppError::database)?;
        let dirty = statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .map_err(AppError::database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::database)?;
        drop(statement);
        for (chunk_id, generation) in dirty {
            rebuild_semantic_chunk(&self.connection, chunk_id, generation.as_deref())?;
        }
        self.connection
            .execute("DELETE FROM dirty_semantic_chunks", [])
            .map_err(AppError::database)?;
        Ok(())
    }

    pub(crate) fn defer_search_updates(&mut self) -> Result<(), AppError> {
        self.require_writer()?;
        self.search_updates = SearchUpdates::FullRebuild;
        Ok(())
    }

    /// Keep delta invalidation atomic with canonical writes, but build views
    /// only in the gold stage. A persisted dirty marker makes lost temporary
    /// delta queues recoverable through a full rebuild after failure/restart.
    pub(crate) fn defer_derived_updates(&mut self) -> Result<(), AppError> {
        self.require_writer()?;
        if self.search_updates == SearchUpdates::Immediate {
            self.search_updates = SearchUpdates::Deferred;
        }
        Ok(())
    }

    pub(crate) fn refresh_derived_search_state(&mut self) -> Result<(), AppError> {
        self.require_writer()?;
        if self.search_updates == SearchUpdates::Deferred {
            self.search_updates = SearchUpdates::Immediate;
            let threshold = self.measured_fts_bulk_threshold()?;
            self.finalize_pending_fts_updates(threshold)?;
            self.finalize_semantic_chunks()?;
            self.connection
                .execute(
                    "UPDATE derived_state SET search_dirty = 0 WHERE singleton = 1",
                    [],
                )
                .map_err(AppError::database)?;
        } else if self.derived_search_is_dirty()? {
            self.rebuild_derived_search_state()?;
        } else {
            self.search_updates = SearchUpdates::Immediate;
        }
        Ok(())
    }

    pub(crate) fn mark_derived_search_dirty(&self) -> Result<(), AppError> {
        self.require_writer()?;
        self.connection
            .execute(
                "UPDATE derived_state SET search_dirty = 1 WHERE singleton = 1",
                [],
            )
            .map_err(AppError::database)?;
        Ok(())
    }

    pub(crate) fn derived_search_is_dirty(&self) -> Result<bool, AppError> {
        self.connection
            .query_row(
                "SELECT search_dirty FROM derived_state WHERE singleton = 1",
                [],
                |row| row.get::<_, bool>(0),
            )
            .map_err(AppError::database)
    }

    pub(crate) fn replace_conversation(
        &mut self,
        conversation: &Conversation,
    ) -> Result<ConversationChange, AppError> {
        if conversation.provider == Provider::ClaudeCode && conversation.claude_lineage.is_some() {
            return self.replace_claude_source(conversation);
        }
        self.replace_canonical_conversation(conversation)
    }

    pub(super) fn replace_canonical_conversation(
        &mut self,
        conversation: &Conversation,
    ) -> Result<ConversationChange, AppError> {
        self.require_writer()?;
        let tombstoned = self
            .connection
            .query_row(
                "SELECT 1 FROM tombstones WHERE provider = ?1 AND conversation_id = ?2",
                params![conversation.provider, conversation.id],
                |_| Ok(()),
            )
            .optional()
            .map_err(AppError::database)?
            .is_some();
        if tombstoned {
            return Ok(ConversationChange {
                tombstoned: true,
                ..ConversationChange::default()
            });
        }

        let repaired_messages = self.repair_claude_source_identity(conversation)?;

        let source_fingerprint = conversation_fingerprint(conversation)?;
        let existing_source_fingerprint: Option<String> = self
            .connection
            .query_row(
                "SELECT source_fingerprint FROM conversations WHERE id = ?1",
                [&conversation.id],
                |row| row.get(0),
            )
            .optional()
            .map_err(AppError::database)?;
        if existing_source_fingerprint.as_deref() == Some(source_fingerprint.as_str()) {
            return Ok(ConversationChange {
                unchanged: true,
                ..ConversationChange::default()
            });
        }
        if existing_source_fingerprint.is_some()
            && self.search_updates != SearchUpdates::FullRebuild
        {
            self.stage_semantic_conversation(&conversation.id)?;
            self.mark_semantic_index_incomplete()?;
        }

        let existing_messages = self.message_states(&conversation.id)?;
        self.connection
            .execute(
                "INSERT INTO conversations(
                    id, provider, source_path, title, created_at, updated_at, source_fingerprint, project, repository,
                    source_key, source_session
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(id) DO UPDATE SET
                    provider = excluded.provider,
                    source_path = excluded.source_path,
                    source_key = excluded.source_key,
                    source_session = excluded.source_session,
                    title = excluded.title,
                    created_at = excluded.created_at,
                    updated_at = excluded.updated_at,
                    source_fingerprint = excluded.source_fingerprint,
                    project = excluded.project, repository = excluded.repository",
                params![
                    conversation.id,
                    conversation.provider,
                    conversation.source.path.to_string_lossy(),
                    conversation.title,
                    conversation.created_at,
                    conversation.updated_at,
                    source_fingerprint,
                    conversation.project,
                    conversation.repository,
                    conversation.source.key(),
                    conversation.source.session,
                ],
            )
            .map_err(AppError::database)?;

        let (changed_message_ids, removed_messages) =
            self.reconcile_messages(conversation, &existing_messages)?;
        if !changed_message_ids.is_empty() {
            self.mark_semantic_index_incomplete()?;
        }
        if self.search_updates != SearchUpdates::Immediate
            && (!changed_message_ids.is_empty() || removed_messages != 0)
        {
            self.mark_derived_search_dirty()?;
        }
        Ok(ConversationChange {
            changed_message_ids,
            removed_messages: removed_messages + repaired_messages,
            ..ConversationChange::default()
        })
    }

    pub(super) fn repair_claude_source_identity(
        &mut self,
        conversation: &Conversation,
    ) -> Result<u64, AppError> {
        if conversation.provider != Provider::ClaudeCode {
            return Ok(0);
        }
        // The old parser could store a subagent under its parent's ID. Repair
        // source ownership within the writer transaction, staging every derived
        // index before deletion so an agent-first replay is also correct.
        let displaced: Option<i64> = self
            .connection
            .query_row(
                "SELECT (SELECT COUNT(*) FROM messages WHERE conversation_id = conversations.id)
             FROM conversations WHERE provider = ?1 AND source_key = ?2 AND id != ?3",
                params![
                    conversation.provider,
                    conversation.source.key(),
                    conversation.id
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(AppError::database)?;
        if displaced.is_some() {
            self.remove_source(conversation.provider, &conversation.source)?;
        }
        u64::try_from(displaced.unwrap_or_default())
            .map_err(|_| AppError::internal("invalid displaced message count"))
    }

    pub(crate) fn source_checkpoint_matches(
        &self,
        provider: Provider,
        source: &crate::silver::SourceIdentity,
        stamp: &crate::silver::SourceStamp,
    ) -> Result<bool, AppError> {
        Ok(self
            .connection
            .query_row(
                "SELECT 1 FROM source_checkpoints
                  WHERE provider = ?1 AND source_key = ?2
                    AND size_bytes = ?3 AND modified_ns = ?4 AND fingerprint = ?5
                    AND interpretation_version = ?6",
                params![
                    provider,
                    source.key(),
                    stamp.size_bytes,
                    stamp.activity_ns.unwrap_or(stamp.modified_ns),
                    stamp.fingerprint,
                    crate::silver::certificate::INTERPRETATION_VERSION
                ],
                |_| Ok(()),
            )
            .optional()
            .map_err(AppError::database)?
            .is_some())
    }

    pub(crate) fn record_source_checkpoint(
        &mut self,
        provider: Provider,
        source: &crate::silver::SourceIdentity,
        stamp: &crate::silver::SourceStamp,
    ) -> Result<(), AppError> {
        self.require_writer()?;
        self.connection
            .execute(
                "INSERT INTO source_checkpoints(provider, source_key, size_bytes, modified_ns, source_path, fingerprint, interpretation_version)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(provider, source_key) DO UPDATE SET
                    size_bytes = excluded.size_bytes,
                    modified_ns = excluded.modified_ns,
                    source_path = excluded.source_path, fingerprint = excluded.fingerprint,
                    interpretation_version = excluded.interpretation_version",
                params![provider, source.key(), stamp.size_bytes, stamp.activity_ns.unwrap_or(stamp.modified_ns), source.path.to_string_lossy(), stamp.fingerprint, crate::silver::certificate::INTERPRETATION_VERSION],
            )
            .map_err(AppError::database)?;
        self.clear_silver_rejection(provider, &source.key())?;
        Ok(())
    }

    fn reconcile_messages(
        &mut self,
        conversation: &Conversation,
        existing_messages: &BTreeMap<String, MessageState>,
    ) -> Result<(Vec<String>, u64), AppError> {
        let mut changed = Vec::new();
        let mut upsert_message = self
            .connection
            .prepare_cached(
                "INSERT INTO messages(
                    id, conversation_id, ordinal, role, content, search_projection,
                    created_at, fingerprint, context_parent
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO UPDATE SET
                    conversation_id = excluded.conversation_id,
                    ordinal = excluded.ordinal,
                    role = excluded.role,
                    content = excluded.content,
                    search_projection = excluded.search_projection,
                    created_at = excluded.created_at,
                    fingerprint = excluded.fingerprint,
                    context_parent = excluded.context_parent",
            )
            .map_err(AppError::database)?;
        let mut stage_fts = self
            .connection
            .prepare_cached("INSERT OR IGNORE INTO pending_fts_messages(message_id) VALUES (?1)")
            .map_err(AppError::database)?;
        let mut delete_embedding = self
            .connection
            .prepare_cached("DELETE FROM message_embeddings WHERE message_id = ?1")
            .map_err(AppError::database)?;
        let mut queue_embedding = self
            .connection
            .prepare_cached("INSERT OR IGNORE INTO pending_embeddings(message_id) VALUES (?1)")
            .map_err(AppError::database)?;
        let mut unqueue_embedding = self
            .connection
            .prepare_cached("DELETE FROM pending_embeddings WHERE message_id = ?1")
            .map_err(AppError::database)?;
        let removed = self.remove_missing_messages(conversation, existing_messages)?;
        self.park_moved_ordinals(conversation, existing_messages)?;

        for message in conversation.messages.rows()? {
            let message = message?;
            let fingerprint = message_fingerprint(&message);
            if existing_messages
                .get(&message.id)
                .map(|state| &state.fingerprint)
                == Some(&fingerprint)
            {
                continue;
            }
            if self.search_updates != SearchUpdates::FullRebuild {
                self.stage_semantic_message(&message.id, None)?;
            }
            upsert_message
                .execute(params![
                    message.id,
                    conversation.id,
                    message.ordinal,
                    message.role,
                    message.content,
                    message.dialogue_text,
                    message.created_at,
                    fingerprint,
                    message.context.parent(),
                ])
                .map_err(AppError::database)?;
            if self.search_updates != SearchUpdates::FullRebuild {
                stage_fts
                    .execute([&message.id])
                    .map_err(AppError::database)?;
                if existing_messages
                    .get(&message.id)
                    .map(|state| state.search_text_hash)
                    != Some(blake3::hash(message_search_text(&message).as_bytes()))
                {
                    delete_embedding
                        .execute([&message.id])
                        .map_err(AppError::database)?;
                    if message_search_text(&message).is_empty() {
                        unqueue_embedding
                            .execute([&message.id])
                            .map_err(AppError::database)?;
                    } else {
                        queue_embedding
                            .execute([&message.id])
                            .map_err(AppError::database)?;
                    }
                }
                self.stage_semantic_message(&message.id, None)?;
            }
            changed.push(message.id.clone());
        }
        Ok((changed, removed))
    }

    fn remove_missing_messages(
        &self,
        conversation: &Conversation,
        existing: &BTreeMap<String, MessageState>,
    ) -> Result<u64, AppError> {
        let incoming = conversation
            .messages
            .rows()?
            .map(|message| message.map(|message| message.id.clone()))
            .collect::<Result<BTreeSet<_>, AppError>>()?;
        let mut delete = self
            .connection
            .prepare_cached("DELETE FROM messages WHERE id = ?1")
            .map_err(AppError::database)?;
        let mut stage = self
            .connection
            .prepare_cached("INSERT OR IGNORE INTO pending_fts_messages(message_id) VALUES (?1)")
            .map_err(AppError::database)?;
        let mut removed = 0_u64;
        for id in existing.keys().filter(|id| !incoming.contains(id.as_str())) {
            if self.search_updates != SearchUpdates::FullRebuild {
                stage.execute([id]).map_err(AppError::database)?;
                self.stage_semantic_message(id, None)?;
            }
            delete.execute([id]).map_err(AppError::database)?;
            removed += 1;
        }
        Ok(removed)
    }

    pub(crate) fn rebuild_derived_search_state(&mut self) -> Result<(), AppError> {
        self.rebuild_fts(true)?;
        self.connection
            .execute_batch(
                "DELETE FROM message_embeddings;
                 DELETE FROM semantic_chunks;
                 DELETE FROM dirty_semantic_chunks;
                 DELETE FROM pending_embeddings;
                 INSERT INTO pending_embeddings(message_id)
                    SELECT id FROM messages
                     WHERE COALESCE(search_projection, content) <> '';
                 UPDATE derived_state
                    SET search_dirty = 0,
                        semantic_ready_generation = NULL
                  WHERE singleton = 1;",
            )
            .map_err(AppError::database)?;
        self.search_updates = SearchUpdates::Immediate;
        Ok(())
    }

    fn rebuild_fts(&mut self, optimize: bool) -> Result<(), AppError> {
        self.connection
            .execute_batch(
                "DELETE FROM message_fts;
                 INSERT INTO message_fts(content, message_id, conversation_id)
                 SELECT COALESCE(search_projection, content), id, conversation_id
                   FROM messages
                  WHERE COALESCE(search_projection, content) <> '';
                 DELETE FROM pending_fts_messages;",
            )
            .map_err(AppError::database)?;
        if optimize {
            self.connection
                .execute(
                    "INSERT INTO message_fts(message_fts) VALUES ('optimize')",
                    [],
                )
                .map_err(AppError::database)?;
        }
        Ok(())
    }

    pub(crate) fn purge_missing_sources(
        &mut self,
        provider: Provider,
        observed_keys: &BTreeSet<String>,
        authoritative_roots: &[std::path::PathBuf],
        cutoff_modified_ns: Option<i64>,
    ) -> Result<u64, AppError> {
        self.require_writer()?;
        self.purge_missing_silver_rejections(
            provider,
            observed_keys,
            authoritative_roots,
            cutoff_modified_ns,
        )?;
        let mut removed = if provider == Provider::ClaudeCode {
            self.purge_claude_sources(observed_keys, authoritative_roots, cutoff_modified_ns)?
        } else {
            0
        };
        let mut statement = self
            .connection
            .prepare(
                "SELECT conversations.id, conversations.source_path, conversations.source_key,
                        source_checkpoints.modified_ns
                   FROM conversations
                   LEFT JOIN source_checkpoints
                     ON source_checkpoints.provider = conversations.provider
                    AND source_checkpoints.source_key = conversations.source_key
                  WHERE conversations.provider = ?1",
            )
            .map_err(AppError::database)?;
        let rows = statement
            .query_map([provider], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            })
            .map_err(AppError::database)?;
        let existing = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::database)?;
        drop(statement);
        for (id, source_path, source_key, modified_ns) in existing {
            if observed_keys.contains(&source_key)
                || !source_is_in_reconciliation_scope(
                    &source_path,
                    modified_ns,
                    authoritative_roots,
                    cutoff_modified_ns,
                )
            {
                continue;
            }
            if self.search_updates != SearchUpdates::FullRebuild {
                self.stage_conversation_fts(&id)?;
                self.stage_semantic_conversation(&id)?;
                self.mark_semantic_index_incomplete()?;
            }
            self.connection
                .execute("DELETE FROM conversations WHERE id = ?1", [&id])
                .map_err(AppError::database)?;
            if self.search_updates != SearchUpdates::Immediate {
                self.mark_derived_search_dirty()?;
            }
            removed += 1;
        }
        let mut statement = self
            .connection
            .prepare(
                "SELECT source_path, source_key, modified_ns
                   FROM source_checkpoints WHERE provider = ?1",
            )
            .map_err(AppError::database)?;
        let checkpoint_paths = statement
            .query_map([provider], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(AppError::database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::database)?;
        drop(statement);
        for (source_path, source_key, modified_ns) in checkpoint_paths {
            if !observed_keys.contains(&source_key)
                && source_is_in_reconciliation_scope(
                    &source_path,
                    Some(modified_ns),
                    authoritative_roots,
                    cutoff_modified_ns,
                )
            {
                self.connection
                    .execute(
                        "DELETE FROM source_checkpoints
                          WHERE provider = ?1 AND source_key = ?2",
                        params![provider, source_key],
                    )
                    .map_err(AppError::database)?;
            }
        }
        Ok(removed)
    }

    pub(crate) fn remove_source(
        &mut self,
        provider: Provider,
        source: &crate::silver::SourceIdentity,
    ) -> Result<bool, AppError> {
        self.require_writer()?;
        if provider == Provider::ClaudeCode
            && let Some(removed) = self.remove_claude_source(&source.key())?
        {
            return Ok(removed);
        }
        let id: Option<String> = self
            .connection
            .query_row(
                "SELECT id FROM conversations WHERE provider = ?1 AND source_key = ?2",
                params![provider, source.key()],
                |row| row.get(0),
            )
            .optional()
            .map_err(AppError::database)?;
        let Some(id) = id else {
            return Ok(false);
        };
        self.remove_canonical_conversation(&id)
    }

    pub(super) fn remove_canonical_conversation(&mut self, id: &str) -> Result<bool, AppError> {
        if self.search_updates != SearchUpdates::FullRebuild {
            self.stage_conversation_fts(id)?;
            self.stage_semantic_conversation(id)?;
            self.mark_semantic_index_incomplete()?;
        }
        if self.search_updates != SearchUpdates::Immediate {
            self.mark_derived_search_dirty()?;
        }
        self.connection
            .execute("DELETE FROM conversations WHERE id = ?1", [id])
            .map_err(AppError::database)?;
        Ok(true)
    }

    fn stage_conversation_fts(&self, conversation_id: &str) -> Result<(), AppError> {
        self.connection
            .execute(
                "INSERT OR IGNORE INTO pending_fts_messages(message_id)
                 SELECT id FROM messages WHERE conversation_id = ?1",
                [conversation_id],
            )
            .map_err(AppError::database)?;
        Ok(())
    }

    pub(crate) fn forget(&mut self, id: &str) -> Result<bool, AppError> {
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(AppError::database)?;
        let silver = self.silver_certificate()?;
        let mut manifest = self
            .gold_manifest()?
            .filter(|m| m.matches(&m.build, &m.model_generation, &silver));
        if let Some(m) = &manifest
            && !self.semantic_index_is_ready(&m.build.generation(&m.model_generation))?
        {
            manifest = None;
        }
        let provider: Option<Provider> = transaction
            .query_row(
                "SELECT provider FROM conversations WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()
            .map_err(AppError::database)?;
        let Some(provider) = provider else {
            transaction.commit().map_err(AppError::database)?;
            return Ok(false);
        };
        let semantic_generation = transaction
            .query_row(
                "SELECT semantic_ready_generation FROM derived_state WHERE singleton = 1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
            .map_err(AppError::database)?;
        let mut statement = transaction
            .prepare(
                "SELECT DISTINCT (rowid - 1) / ?2
                   FROM messages WHERE conversation_id = ?1 ORDER BY 1",
            )
            .map_err(AppError::database)?;
        let semantic_chunk_ids = statement
            .query_map(params![id, SEMANTIC_CHUNK_ROWS], |row| row.get(0))
            .map_err(AppError::database)?
            .collect::<Result<Vec<i64>, _>>()
            .map_err(AppError::database)?;
        drop(statement);
        transaction
            .execute(
                "INSERT INTO tombstones(provider, conversation_id, forgotten_at)
                 VALUES (?1, ?2, unixepoch())
                 ON CONFLICT(provider, conversation_id) DO NOTHING",
                params![provider, id],
            )
            .map_err(AppError::database)?;
        transaction
            .execute("DELETE FROM message_fts WHERE conversation_id = ?1", [id])
            .map_err(AppError::database)?;
        let removed = transaction
            .execute("DELETE FROM conversations WHERE id = ?1", [id])
            .map_err(AppError::database)?;
        sql::DeleteClaudeThreadSources::builder()
            .conversation_id(id)
            .build()
            .execute(&transaction)
            .map_err(AppError::database)?;
        for chunk_id in semantic_chunk_ids {
            rebuild_semantic_chunk(&transaction, chunk_id, semantic_generation.as_deref())?;
        }
        if let Some(mut manifest) = manifest {
            // This was a certified, fully covered build. Deletion maintains
            // FTS and packed vectors above and cannot introduce missing vectors.
            // Advance its revision atomically with the deletion.
            let revision: i64 = transaction
                .query_row(
                    "SELECT revision FROM silver_state WHERE singleton=1",
                    [],
                    |r| r.get(0),
                )
                .map_err(AppError::database)?;
            manifest.silver.revision = u64::try_from(revision)
                .map_err(|_| AppError::database_data("invalid silver revision"))?;
            let json = serde_json::to_string(&manifest)
                .map_err(|_| AppError::internal("serialize gold build manifest"))?;
            transaction
                .execute(
                    "UPDATE gold_manifest SET manifest=?1 WHERE singleton=1",
                    [json],
                )
                .map_err(AppError::database)?;
        }
        transaction.commit().map_err(AppError::database)?;
        Ok(removed > 0)
    }

    fn park_moved_ordinals(
        &self,
        conversation: &Conversation,
        existing: &BTreeMap<String, MessageState>,
    ) -> Result<(), AppError> {
        // Free occupied destination slots before inserting/updating stable IDs.
        // Positive temporary ordinals preserve the parent < ordinal CHECK for
        // tree histories. The writer transaction hides staging from readers.
        let mut ordinal = existing
            .values()
            .map(|state| state.ordinal)
            .max()
            .unwrap_or(-1);
        for message in conversation.messages.rows()? {
            ordinal = ordinal.max(message?.ordinal);
        }
        let mut statement = self
            .connection
            .prepare_cached(sql::ParkMessageOrdinal::QUERY)
            .map_err(AppError::database)?;
        for message in conversation.messages.rows()? {
            let message = message?;
            if existing
                .get(&message.id)
                .is_some_and(|state| state.ordinal != message.ordinal)
            {
                ordinal = ordinal
                    .checked_add(1)
                    .ok_or_else(|| AppError::database_data("message ordinal staging overflow"))?;
                let query = sql::ParkMessageOrdinal::builder()
                    .ordinal(ordinal)
                    .conversation_id(&conversation.id)
                    .message_id(&message.id)
                    .build();
                statement
                    .execute(query.as_params())
                    .map_err(AppError::database)?;
            }
        }
        Ok(())
    }

    fn message_states(
        &self,
        conversation_id: &str,
    ) -> Result<BTreeMap<String, MessageState>, AppError> {
        let query = sql::ConversationMessageState::builder()
            .conversation_id(conversation_id)
            .build();
        let mut statement = self
            .connection
            .prepare_cached(query.query_str())
            .map_err(AppError::database)?;
        // Decode and fingerprint one text at a time. Collecting generated rows
        // first would retain another full copy of the conversation's text.
        statement
            .query_map(
                query.as_params(),
                sql::ConversationMessageStateRow::from_row,
            )
            .map_err(AppError::database)?
            .map(|row| {
                let row = row?;
                Ok((
                    row.id,
                    MessageState {
                        fingerprint: row.fingerprint,
                        ordinal: row.ordinal,
                        search_text_hash: blake3::hash(row.search_text.as_bytes()),
                    },
                ))
            })
            .collect::<rusqlite::Result<_>>()
            .map_err(AppError::database)
    }
}

pub(super) fn source_is_in_reconciliation_scope(
    source_path: &str,
    modified_ns: Option<i64>,
    authoritative_roots: &[std::path::PathBuf],
    cutoff_modified_ns: Option<i64>,
) -> bool {
    let source_path = Path::new(source_path);
    if !authoritative_roots
        .iter()
        .any(|root| source_path.starts_with(root))
    {
        return false;
    }
    cutoff_modified_ns.is_none_or(|cutoff| modified_ns.is_some_and(|value| value >= cutoff))
}
fn conversation_fingerprint(conversation: &Conversation) -> Result<String, AppError> {
    let mut hasher = blake3::Hasher::new();
    update_hash(&mut hasher, conversation.provider.as_str());
    update_hash(&mut hasher, &conversation.id);
    update_hash(&mut hasher, &conversation.source.key());
    update_hash(
        &mut hasher,
        conversation.project.as_deref().unwrap_or_default(),
    );
    update_hash(
        &mut hasher,
        conversation.repository.as_deref().unwrap_or_default(),
    );
    update_hash(
        &mut hasher,
        conversation.title.as_deref().unwrap_or_default(),
    );
    update_hash(
        &mut hasher,
        &conversation.created_at.unwrap_or_default().to_string(),
    );
    update_hash(
        &mut hasher,
        &conversation.updated_at.unwrap_or_default().to_string(),
    );
    for message in conversation.messages.rows()? {
        let message = message?;
        update_hash(&mut hasher, &message_fingerprint(&message));
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn message_fingerprint(message: &crate::silver::NormalizedMessage) -> String {
    let mut hasher = blake3::Hasher::new();
    update_hash(&mut hasher, &message.id);
    update_hash(&mut hasher, &message.ordinal.to_string());
    if let Some(parent) = message.context.parent() {
        update_hash(&mut hasher, "tree-context");
        update_hash(&mut hasher, &parent.to_string());
    }
    update_hash(&mut hasher, message.role.as_str());
    update_hash(&mut hasher, &message.content);
    match &message.dialogue_text {
        None => update_hash(&mut hasher, "canonical"),
        Some(projection) => {
            update_hash(&mut hasher, "projected");
            update_hash(&mut hasher, projection);
        }
    }
    update_hash(
        &mut hasher,
        &message.created_at.unwrap_or_default().to_string(),
    );
    hasher.finalize().to_hex().to_string()
}

fn message_search_text(message: &crate::silver::NormalizedMessage) -> &str {
    message.dialogue()
}

fn update_hash(hasher: &mut blake3::Hasher, value: &str) {
    hasher.update(&(value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}
