use crate::provider::Provider;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rusqlite::{OptionalExtension, params};

use super::generated as sql;
use super::{ChatChange, Storage};
use crate::AppError;
use crate::silver::Chat;

struct MessageState {
    fingerprint: String,
    ordinal: i64,
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

    pub(crate) fn replace_chat(&mut self, chat: &Chat) -> Result<ChatChange, AppError> {
        if chat.provider == Provider::ClaudeCode && chat.claude_lineage.is_some() {
            return self.replace_claude_source(chat);
        }
        self.replace_canonical_chat(chat)
    }

    pub(super) fn replace_canonical_chat(&mut self, chat: &Chat) -> Result<ChatChange, AppError> {
        self.require_writer()?;
        let tombstoned = self
            .connection
            .query_row(
                "SELECT 1 FROM tombstones WHERE provider = ?1 AND chat_id = ?2",
                params![chat.provider, chat.id],
                |_| Ok(()),
            )
            .optional()
            .map_err(AppError::database)?
            .is_some();
        if tombstoned {
            return Ok(ChatChange {
                tombstoned: true,
                ..ChatChange::default()
            });
        }

        let repaired_messages = self.repair_claude_source_identity(chat)?;

        let enrichment = crate::silver::enrichment::Evidence::capture(chat)?;
        let source_fingerprint = chat_fingerprint(chat, &enrichment)?;
        let existing_source_fingerprint = sql::ChatSourceFingerprint::builder()
            .id(&chat.id)
            .build()
            .query_opt(&self.connection)
            .map_err(AppError::database)?
            .map(|row| row.source_fingerprint);
        if existing_source_fingerprint.as_deref() == Some(source_fingerprint.as_str()) {
            // A metadata-only migration can lose evidence while retaining the
            // same history fingerprint. Explicit replay must repair that row.
            self.store_enrichment(&chat.id, &enrichment)?;
            return Ok(ChatChange {
                unchanged: true,
                ..ChatChange::default()
            });
        }
        let existing_messages = self.message_states(&chat.id)?;
        let title = chat.title.clone().map(|mut title| {
            crate::redaction::private_text(&mut title);
            title
        });
        sql::UpsertChat::builder()
            .provider(chat.provider.as_str())
            .id(&chat.id)
            .title(title.as_deref())
            .created_at(chat.created_at)
            .updated_at(chat.updated_at)
            .project(chat.project.as_deref())
            .repository(chat.repository.as_deref())
            .build()
            .execute(&self.connection)
            .map_err(AppError::database)?;
        let source_path = chat.source.path.to_string_lossy();
        let source_key = chat.source.key();
        sql::SetChatSource::builder()
            .source_path(&source_path)
            .source_fingerprint(&source_fingerprint)
            .source_key(Some(&source_key))
            .source_session(chat.source.session.as_deref())
            .id(&chat.id)
            .build()
            .execute(&self.connection)
            .map_err(AppError::database)?;

        self.store_enrichment(&chat.id, &enrichment)?;
        let (changed_message_ids, removed_messages) =
            self.reconcile_messages(chat, &existing_messages)?;
        if !changed_message_ids.is_empty() || removed_messages != 0 {
            super::turns::derive(&self.connection, &chat.id)?;
        }
        Ok(ChatChange {
            changed_message_ids,
            removed_messages: removed_messages + repaired_messages,
            ..ChatChange::default()
        })
    }

    pub(super) fn repair_claude_source_identity(&mut self, chat: &Chat) -> Result<u64, AppError> {
        if chat.provider != Provider::ClaudeCode {
            return Ok(0);
        }
        // The old parser could store a subagent under its parent's ID. Repair
        // source ownership within the writer transaction, staging every derived
        // index before deletion so an agent-first replay is also correct.
        let source_key = chat.source.key();
        let displaced = sql::DisplacedChatMessages::builder()
            .provider(chat.provider.as_str())
            .source_key(Some(&source_key))
            .id(&chat.id)
            .build()
            .query_opt(&self.connection)
            .map_err(AppError::database)?
            .map(|row| row.messages);
        if displaced.is_some() {
            self.remove_source(chat.provider, &chat.source)?;
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
        if !self.source_enrichment_is_current(provider, &source.key())? {
            return Ok(false);
        }
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
                    stamp.checkpoint().0,
                    stamp.activity_ns.unwrap_or(stamp.modified_ns),
                    stamp.checkpoint().1,
                    crate::silver::certificate::INTERPRETATION_VERSION
                ],
                |_| Ok(()),
            )
            .optional()
            .map_err(AppError::database)?
            .is_some())
    }

    /// Mark every checkpoint of these providers as read by an older release,
    /// so the next pass reads their sources again. A chat whose content and
    /// metadata come out the same is left as it is.
    pub(crate) fn reread_sources(
        &mut self,
        providers: impl IntoIterator<Item = Provider>,
    ) -> Result<(), AppError> {
        self.require_writer()?;
        for provider in providers {
            self.connection
                .execute(
                    "UPDATE source_checkpoints SET interpretation_version = 0 WHERE provider = ?1",
                    params![provider],
                )
                .map_err(AppError::database)?;
        }
        Ok(())
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
                params![provider, source.key(), stamp.checkpoint().0, stamp.activity_ns.unwrap_or(stamp.modified_ns), source.path.to_string_lossy(), stamp.checkpoint().1, crate::silver::certificate::INTERPRETATION_VERSION],
            )
            .map_err(AppError::database)?;
        self.clear_silver_rejection(provider, &source.key())?;
        Ok(())
    }

    fn reconcile_messages(
        &mut self,
        chat: &Chat,
        existing_messages: &BTreeMap<String, MessageState>,
    ) -> Result<(Vec<String>, u64), AppError> {
        let mut changed = Vec::new();
        let mut upsert_message = self
            .connection
            .prepare_cached(sql::UpsertMessage::QUERY)
            .map_err(AppError::database)?;
        let mut upsert_fingerprint = self
            .connection
            .prepare_cached(sql::SetMessageFingerprint::QUERY)
            .map_err(AppError::database)?;
        let removed = self.remove_missing_messages(chat, existing_messages)?;
        self.park_moved_ordinals(chat, existing_messages)?;

        for message in chat.messages.rows()? {
            let mut message = message?;
            crate::silver::NormalizedMessage::mask_private(&mut message);
            let fingerprint = message_fingerprint(&message);
            if existing_messages
                .get(&message.id)
                .map(|state| &state.fingerprint)
                == Some(&fingerprint)
            {
                continue;
            }
            let attachments = serde_json::to_string(&self.retain_attachments(&message)?)
                .map_err(|_| AppError::internal("serialize attachments"))?;
            let metadata = message.tools.encode()?;
            let upsert = sql::UpsertMessage::builder()
                .id(&message.id)
                .chat_id(&chat.id)
                .ordinal(message.ordinal)
                .role(message.role.as_str())
                .content(&message.content)
                .search_projection(message.dialogue_text.as_deref())
                .created_at(message.created_at)
                .context_parent(message.context.parent())
                .attachments(&attachments)
                .metadata(metadata.as_deref())
                .build();
            upsert_message
                .execute(upsert.as_params())
                .map_err(AppError::database)?;
            let stored = sql::SetMessageFingerprint::builder()
                .fingerprint(&fingerprint)
                .id(&message.id)
                .build();
            upsert_fingerprint
                .execute(stored.as_params())
                .map_err(AppError::database)?;
            changed.push(message.id.clone());
        }
        Ok((changed, removed))
    }

    fn remove_missing_messages(
        &self,
        chat: &Chat,
        existing: &BTreeMap<String, MessageState>,
    ) -> Result<u64, AppError> {
        let incoming = chat
            .messages
            .rows()?
            .map(|message| message.map(|message| message.id.clone()))
            .collect::<Result<BTreeSet<_>, AppError>>()?;
        let mut delete = self
            .connection
            .prepare_cached(sql::DeleteMessage::QUERY)
            .map_err(AppError::database)?;
        let mut removed = 0_u64;
        for id in existing.keys().filter(|id| !incoming.contains(id.as_str())) {
            delete.execute([id]).map_err(AppError::database)?;
            removed += 1;
        }
        Ok(removed)
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
        let existing = sql::ChatsForPurge::builder()
            .provider(provider.as_str())
            .build()
            .query_many(&self.connection)
            .map_err(AppError::database)?
            .into_iter()
            .map(|row| {
                (
                    row.id,
                    row.source_path,
                    row.source_key.unwrap_or_default(),
                    row.modified_ns,
                )
            })
            .collect::<Vec<_>>();
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
            self.delete_chat(&id)?;
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
        let source_key = source.key();
        let id = sql::ChatBySource::builder()
            .provider(provider.as_str())
            .source_key(Some(&source_key))
            .build()
            .query_opt(&self.connection)
            .map_err(AppError::database)?
            .map(|row| row.id);
        let Some(id) = id else {
            return Ok(false);
        };
        self.remove_canonical_chat(&id)
    }

    pub(super) fn remove_canonical_chat(&mut self, id: &str) -> Result<bool, AppError> {
        self.delete_chat(id)?;
        Ok(true)
    }

    fn delete_chat(&self, id: &str) -> Result<(), AppError> {
        sql::DeleteChat::builder()
            .id(id)
            .build()
            .execute(&self.connection)
            .map_err(AppError::database)?;
        Ok(())
    }

    pub(crate) fn forget(&mut self, id: &str) -> Result<bool, AppError> {
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(AppError::database)?;
        let records_removed = super::records::forget_chat(&transaction, id)?;
        let provider = sql::ChatProvider::builder()
            .id(id)
            .build()
            .query_opt(&transaction)
            .map_err(AppError::database)?
            .map(|row| row.provider.parse::<Provider>())
            .transpose()?;
        let Some(provider) = provider else {
            transaction.commit().map_err(AppError::database)?;
            return Ok(records_removed > 0);
        };
        transaction
            .execute(
                "INSERT INTO tombstones(provider, chat_id, forgotten_at)
                 VALUES (?1, ?2, unixepoch())
                 ON CONFLICT(provider, chat_id) DO NOTHING",
                params![provider, id],
            )
            .map_err(AppError::database)?;
        let removed = sql::DeleteChat::builder()
            .id(id)
            .build()
            .execute(&transaction)
            .map_err(AppError::database)?;
        sql::DeleteClaudeChatSources::builder()
            .chat_id(id)
            .build()
            .execute(&transaction)
            .map_err(AppError::database)?;
        transaction.commit().map_err(AppError::database)?;
        Ok(removed > 0)
    }

    fn park_moved_ordinals(
        &self,
        chat: &Chat,
        existing: &BTreeMap<String, MessageState>,
    ) -> Result<(), AppError> {
        // Free occupied destination slots before inserting/updating stable IDs.
        // Branch references use stable IDs and are unaffected by these positions.
        // The writer transaction hides staging from readers.
        let mut ordinal = existing
            .values()
            .map(|state| state.ordinal)
            .max()
            .unwrap_or(-1);
        for message in chat.messages.rows()? {
            ordinal = ordinal.max(message?.ordinal);
        }
        let mut statement = self
            .connection
            .prepare_cached(sql::ParkMessageOrdinal::QUERY)
            .map_err(AppError::database)?;
        for message in chat.messages.rows()? {
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
                    .chat_id(&chat.id)
                    .message_id(&message.id)
                    .build();
                statement
                    .execute(query.as_params())
                    .map_err(AppError::database)?;
            }
        }
        Ok(())
    }

    fn message_states(&self, chat_id: &str) -> Result<BTreeMap<String, MessageState>, AppError> {
        let query = sql::ChatMessageState::builder().chat_id(chat_id).build();
        let mut statement = self
            .connection
            .prepare_cached(query.query_str())
            .map_err(AppError::database)?;
        // Decode and fingerprint one text at a time. Collecting generated rows
        // first would retain another full copy of the chat's text.
        statement
            .query_map(query.as_params(), sql::ChatMessageStateRow::from_row)
            .map_err(AppError::database)?
            .map(|row| {
                let row = row.map_err(AppError::database)?;
                Ok((
                    row.id,
                    MessageState {
                        fingerprint: row.fingerprint,
                        ordinal: row.ordinal,
                    },
                ))
            })
            .collect()
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
fn chat_fingerprint(
    chat: &Chat,
    enrichment: &crate::silver::enrichment::Evidence,
) -> Result<String, AppError> {
    let mut hasher = blake3::Hasher::new();
    update_hash(
        &mut hasher,
        &serde_json::to_string(enrichment)
            .map_err(|_| AppError::internal("serialize silver enrichment evidence"))?,
    );
    update_hash(&mut hasher, chat.provider.as_str());
    update_hash(&mut hasher, &chat.id);
    update_hash(&mut hasher, &chat.source.key());
    update_hash(&mut hasher, chat.project.as_deref().unwrap_or_default());
    update_hash(&mut hasher, chat.repository.as_deref().unwrap_or_default());
    update_hash(&mut hasher, chat.title.as_deref().unwrap_or_default());
    update_hash(
        &mut hasher,
        &chat.created_at.unwrap_or_default().to_string(),
    );
    update_hash(
        &mut hasher,
        &chat.updated_at.unwrap_or_default().to_string(),
    );
    let mut seen = BTreeSet::new();
    let mut previous = -1;
    for message in chat.messages.rows()? {
        let message = message?;
        if message.id.is_empty()
            || message.ordinal <= previous
            || !message.context.valid(&message.id)
            || message
                .context
                .parent()
                .is_some_and(|parent| !parent.is_empty() && !seen.contains(parent))
            || !seen.insert(message.id.clone())
        {
            return Err(AppError::database_data(
                "invalid native message identity, order or parent",
            ));
        }
        previous = message.ordinal;
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
        update_hash(&mut hasher, parent);
    }
    update_hash(&mut hasher, message.role.as_str());
    update_hash(&mut hasher, &message.content);
    for attachment in &message.attachments {
        // Source data is typed and serialized deterministically, including changes to embedded bytes.
        if let Ok(encoded) = serde_json::to_string(attachment) {
            update_hash(&mut hasher, &encoded);
        }
    }
    // Absent activity leaves the fingerprints of entries without tools as they were.
    if let Ok(Some(tools)) = message.tools.encode() {
        update_hash(&mut hasher, "tools");
        update_hash(&mut hasher, &tools);
    }
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

fn update_hash(hasher: &mut blake3::Hasher, value: &str) {
    hasher.update(&(value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}
