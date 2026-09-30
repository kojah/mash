//! Physical Claude contributions outlive canonical source ownership changes.
use std::collections::BTreeSet;
use std::path::PathBuf;

use super::generated as sql;
use super::indexing::source_is_in_reconciliation_scope;
use super::{ConversationChange, Storage};
use crate::AppError;
use crate::ingestion::{Conversation, NormalizedMessage, SourceIdentity, claude};
use crate::provider::Provider;

pub(crate) struct ClaudeSourceRef {
    pub(crate) source: SourceIdentity,
    pub(crate) conversation_id: String,
}

pub(super) fn claude_references(
    connection: &rusqlite::Connection,
) -> Result<Vec<ClaudeSourceRef>, AppError> {
    sql::ClaudeSourceReferences
        .query_many(connection)
        .map_err(AppError::database)
        .map(|rows| {
            rows.into_iter()
                .map(|row| ClaudeSourceRef {
                    source: SourceIdentity::file(row.source_path),
                    conversation_id: row.conversation_id,
                })
                .collect()
        })
}

pub(super) fn claude_thread(
    connection: &rusqlite::Connection,
    id: &str,
    sources: Option<&BTreeSet<String>>,
) -> Result<Option<Conversation>, AppError> {
    let metadata = sql::ClaudeSourceMetadata::builder()
        .conversation_id(id)
        .build()
        .query_many(connection)
        .map_err(AppError::database)?
        .into_iter()
        .find(|source| sources.is_none_or(|keys| keys.contains(&source.source_key)));
    let Some(metadata) = metadata else {
        return Ok(None);
    };
    let rows = sql::ClaudeContributionMessages::builder()
        .conversation_id(id)
        .build()
        .query_many(connection)
        .map_err(AppError::database)?;
    let (messages, lineage) = claude::reconcile(
        rows.into_iter()
            .filter(|row| sources.is_none_or(|keys| keys.contains(&row.source_key)))
            .map(|row| {
                (
                    NormalizedMessage {
                        id: row.message_id,
                        ordinal: 0,
                        context: crate::message_context::MessageContext::Root,
                        role: row.role.into(),
                        content: row.content,
                        search_projection: row.projection,
                        created_at: row.created_at,
                    },
                    row.parent_id,
                )
            }),
    )?;
    let created_at = messages
        .iter()
        .filter_map(|message| message.created_at)
        .min();
    let updated_at = messages
        .iter()
        .filter_map(|message| message.created_at)
        .max();
    Ok(Some(Conversation {
        id: id.to_owned(),
        provider: Provider::ClaudeCode,
        source: SourceIdentity::file(metadata.source_path),
        claude_lineage: Some(lineage),
        title: metadata.title,
        project: metadata.project,
        repository: metadata.repository,
        created_at,
        updated_at,
        messages,
    }))
}

impl Storage {
    #[tracing::instrument(name = "reconcile_claude_source", skip_all)]
    pub(super) fn replace_claude_source(
        &mut self,
        source: &Conversation,
    ) -> Result<ConversationChange, AppError> {
        self.require_writer()?;
        if sql::ClaudeTombstone::builder()
            .conversation_id(&source.id)
            .build()
            .query_opt(&self.connection)
            .map_err(AppError::database)?
            .is_some()
        {
            return Ok(ConversationChange {
                tombstoned: true,
                ..ConversationChange::default()
            });
        }
        self.with_claude_savepoint(|storage| {
            let key = source.source.key();
            let mut moved = ConversationChange::default();
            if let Some(previous) = sql::ClaudeSourceThread::builder()
                .source_key(&key)
                .build()
                .query_opt(&storage.connection)
                .map_err(AppError::database)?
                .filter(|previous| previous.conversation_id != source.id)
            {
                sql::DeleteClaudeSource::builder()
                    .source_key(&key)
                    .build()
                    .execute(&storage.connection)
                    .map_err(AppError::database)?;
                moved = storage.refresh_claude_thread(&previous.conversation_id)?;
            }
            let repaired = storage.repair_claude_source_identity(source)?;
            sql::UpsertClaudeSource::builder()
                .source_key(&key)
                .source_path(&source.source.path.to_string_lossy())
                .conversation_id(&source.id)
                .title(source.title.as_deref())
                .project(source.project.as_deref())
                .repository(source.repository.as_deref())
                .build()
                .execute(&storage.connection)
                .map_err(AppError::database)?;
            sql::DeleteClaudeSourceMessages::builder()
                .source_key(&key)
                .build()
                .execute(&storage.connection)
                .map_err(AppError::database)?;
            let lineage = source
                .claude_lineage
                .as_ref()
                .ok_or_else(|| AppError::internal("Claude contribution has no ancestry"))?;
            let mut statement = storage
                .connection
                .prepare_cached(sql::InsertClaudeSourceMessage::QUERY)
                .map_err(AppError::database)?;
            for message in &source.messages {
                let parent = lineage.get(&message.id).ok_or_else(|| {
                    AppError::internal("Claude contribution has incomplete ancestry")
                })?;
                let query = sql::InsertClaudeSourceMessage::builder()
                    .source_key(&key)
                    .message_id(&message.id)
                    .parent_id(parent.as_deref())
                    .role(message.role.as_str())
                    .content(&message.content)
                    .projection(message.search_projection.as_deref())
                    .created_at(message.created_at)
                    .build();
                statement
                    .execute(query.as_params())
                    .map_err(AppError::database)?;
            }
            drop(statement);
            let mut change = storage.refresh_claude_thread(&source.id)?;
            change.removed_messages += repaired + moved.removed_messages;
            change.changed_message_ids.extend(moved.changed_message_ids);
            Ok(change)
        })
    }

    fn with_claude_savepoint<T>(
        &mut self,
        apply: impl FnOnce(&mut Self) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        self.connection
            .execute_batch("SAVEPOINT mash_claude_source")
            .map_err(AppError::database)?;
        match apply(self) {
            Ok(result) => {
                self.connection
                    .execute_batch("RELEASE mash_claude_source")
                    .map_err(AppError::database)?;
                Ok(result)
            }
            Err(error) => {
                self.connection
                    .execute_batch("ROLLBACK TO mash_claude_source; RELEASE mash_claude_source;")
                    .map_err(AppError::database)?;
                Err(error)
            }
        }
    }

    fn refresh_claude_thread(&mut self, id: &str) -> Result<ConversationChange, AppError> {
        let conversation = claude_thread(&self.connection, id, None)?;
        let Some(conversation) = conversation else {
            let count = sql::CanonicalThreadMessageCount::builder()
                .conversation_id(id)
                .build()
                .query_one(&self.connection)
                .map_err(AppError::database)?
                .messages;
            self.remove_canonical_conversation(id)?;
            return Ok(ConversationChange {
                removed_messages: u64::try_from(count)
                    .map_err(|_| AppError::database_data("negative Claude message count"))?,
                ..ConversationChange::default()
            });
        };
        self.replace_canonical_conversation(&conversation)
    }

    #[tracing::instrument(name = "remove_claude_contribution", skip_all)]
    pub(super) fn remove_claude_source(&mut self, key: &str) -> Result<Option<bool>, AppError> {
        let previous = sql::ClaudeSourceThread::builder()
            .source_key(key)
            .build()
            .query_opt(&self.connection)
            .map_err(AppError::database)?;
        let Some(previous) = previous else {
            return Ok(None);
        };
        self.with_claude_savepoint(|storage| {
            sql::DeleteClaudeSource::builder()
                .source_key(key)
                .build()
                .execute(&storage.connection)
                .map_err(AppError::database)?;
            storage.refresh_claude_thread(&previous.conversation_id)?;
            let retained = sql::ClaudeSourceMetadata::builder()
                .conversation_id(&previous.conversation_id)
                .build()
                .query_many(&storage.connection)
                .map_err(AppError::database)?;
            Ok(Some(retained.is_empty()))
        })
    }

    pub(super) fn purge_claude_sources(
        &mut self,
        observed: &BTreeSet<String>,
        roots: &[PathBuf],
        cutoff: Option<i64>,
    ) -> Result<u64, AppError> {
        let sources = sql::ClaudeSourceScope
            .query_many(&self.connection)
            .map_err(AppError::database)?;
        let mut removed = 0;
        for source in sources {
            if !observed.contains(&source.source_key)
                && source_is_in_reconciliation_scope(
                    &source.source_path,
                    source.modified_ns,
                    roots,
                    cutoff,
                )
            {
                removed += u64::from(
                    self.remove_claude_source(&source.source_key)?
                        .unwrap_or(false),
                );
            }
        }
        Ok(removed)
    }
}
