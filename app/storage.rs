//! Shared SQLite infrastructure for silver history and views retrieval tables.
//! Canonical mutation also invalidates dependent views in the same transaction;
//! views/local owns rebuilding. See silver.sql and views.sql for the two databases.
use std::fs;
use std::path::Path;
use std::time::Duration;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::AppError;

// View views and materialized retrieval data.
mod catalog;
pub(crate) mod connector_views;
mod coverage;
pub(crate) mod embedding_plan;
mod exact;
mod semantic;
mod semantic_chunks;
mod views_manifest;

// Silver mutation/provenance plus atomic invalidation and publication ownership.
mod attachments;
mod claude_sources;
mod enrichment;
pub(crate) mod export;
mod indexing;
pub(crate) mod objects;
pub(crate) mod publication;
mod silver_state;
pub(crate) mod turns;

// Connection/schema and generated query infrastructure for both layers.
mod queries;
pub(crate) mod schema;

#[cfg(test)]
use schema::SCHEMA_VERSION;
// sqlc-gen-rust emits an unused row parameter for :exec result models.
#[allow(dead_code, unused_variables, clippy::all, clippy::pedantic)]
#[path = "storage/generated/queries.rs"]
pub(crate) mod generated;
use schema::initialize;
#[cfg(test)]
use semantic_chunks::validate_quantized_vector;

pub(crate) struct Storage {
    attachments_config: crate::attachments::Configuration,
    /// Match evidence is computed on text masked under the output policy.
    output_redaction: crate::redaction::Policy,
    connection: Connection,
    writer_active: bool,
    search_updates: SearchUpdates,
    transaction_base_messages: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchUpdates {
    Immediate,
    Deferred,
    FullRebuild,
}

#[derive(Debug, Eq, PartialEq)]
enum FtsRefreshStrategy {
    None,
    Incremental,
    Bulk,
    Deferred,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub(crate) struct Counts {
    pub(crate) conversations: u64,
    pub(crate) messages: u64,
    pub(crate) searchable_messages: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) searchable_user_messages: Option<u64>,
    pub(crate) embeddings: u64,
}

pub(crate) struct StatusSnapshot {
    pub(crate) lexical: Option<crate::views::lexical_manifest::Manifest>,
    pub(crate) views: Option<crate::views::manifest::Manifest>,
    pub(crate) silver: Option<crate::silver::certificate::Certificate>,
    pub(crate) counts: Counts,
    pub(crate) current_embeddings: u64,
    pub(crate) derived_clean: bool,
    pub(crate) exact_semantic_coverage: bool,
    pub(crate) connectors: Vec<crate::source::SourceStatus>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct SearchHit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) lookup: Option<crate::search_results::lookup::Lookup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) lexical_match: Option<crate::views::matching::Evidence>,
    pub(crate) id: String,
    /// For a document, its own anchor: documents belong to no chat.
    #[serde(rename = "chat_id")]
    pub(crate) conversation_id: String,
    pub(crate) source: crate::source::Source,
    pub(crate) object: crate::source::ObjectType,
    /// Enclosing containers and segments, nearest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) ancestors: Vec<crate::source::Ancestor>,
    /// Native messages have a role; connector messages have an author kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) role: Option<crate::role::Role>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) author_kind: Option<crate::connectors::AuthorKind>,
    pub(crate) content: String,
    #[serde(default)]
    pub(crate) created_at: Option<i64>,
    #[serde(default)]
    pub(crate) metadata: crate::search_results::ChatMetadata,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) copies: Vec<crate::search_results::MessageReference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) lexical_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) semantic_score: Option<f32>,
    pub(crate) fusion_score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) rerank_score: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) node_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Message {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) attachments: Vec<crate::attachments::Metadata>,
    pub(crate) id: String,
    pub(crate) ordinal: i64,
    /// Native messages have a role; connector messages have an author kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) role: Option<crate::role::Role>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) author_kind: Option<crate::connectors::AuthorKind>,
    pub(crate) content: String,
    pub(crate) created_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum MessageContent {
    #[default]
    Canonical,
    SearchProjection,
}

impl MessageContent {
    // serde's skip_serializing_if callback takes a reference.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub(crate) fn is_canonical(&self) -> bool {
        matches!(self, Self::Canonical)
    }
}

#[cfg(test)]
pub(crate) struct SearchableMessage {
    pub(crate) id: String,
    pub(crate) content: String,
}

pub(crate) struct EmbeddingWrite<'a> {
    pub(crate) message_id: &'a str,
    pub(crate) vector: &'a [u8],
    pub(crate) norm: f32,
}

pub(crate) struct SemanticChunk {
    pub(crate) message_rowids: Vec<i64>,
    pub(crate) values: Vec<u8>,
    pub(crate) norms: Vec<f32>,
    pub(crate) eligible: Vec<bool>,
    pub(crate) dimensions: usize,
}

pub(crate) struct SemanticChunks {
    pub(crate) chunks: Vec<SemanticChunk>,
    pub(crate) dimensions: usize,
}

#[derive(Default)]
pub(crate) struct ConversationChange {
    pub(crate) unchanged: bool,
    pub(crate) tombstoned: bool,
    pub(crate) changed_message_ids: Vec<String>,
    pub(crate) removed_messages: u64,
}

impl Storage {
    /// Search results leave through CLI or MCP output under this policy.
    pub(crate) fn with_output_redaction(mut self, policy: crate::redaction::Policy) -> Self {
        self.output_redaction = policy;
        self
    }

    pub(crate) fn open(path: &Path) -> Result<Self, AppError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(AppError::io)?;
        }
        let connection = Connection::open(path).map_err(AppError::database)?;
        initialize(&connection, &schema::views_path(path))?;
        connection
            .execute_batch(
                "CREATE TEMP TABLE IF NOT EXISTS pending_fts_messages (
                    message_id TEXT PRIMARY KEY
                 ) WITHOUT ROWID;
                 CREATE TEMP TABLE IF NOT EXISTS dirty_semantic_chunks (
                    chunk_id INTEGER PRIMARY KEY,
                    generation TEXT
                 ) WITHOUT ROWID;",
            )
            .map_err(AppError::database)?;
        Ok(Self {
            attachments_config: crate::attachments::Configuration::default(),
            output_redaction: crate::redaction::Policy::ON,
            connection,
            writer_active: false,
            search_updates: SearchUpdates::Immediate,
            transaction_base_messages: 0,
        })
    }

    pub(crate) fn open_writer(path: &Path) -> Result<Self, AppError> {
        let mut storage = Self::open(path)?;
        storage
            .connection
            .busy_timeout(Duration::ZERO)
            .map_err(AppError::database)?;
        storage
            .connection
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(AppError::database)?;
        storage.writer_active = true;
        storage.transaction_base_messages = storage.message_count()?;
        Ok(storage)
    }

    #[tracing::instrument(name = "commit_writer", skip_all)]
    pub(crate) fn commit_writer(&mut self) -> Result<(), AppError> {
        if self.writer_active {
            let threshold = self.measured_fts_bulk_threshold()?;
            self.finalize_pending_fts_updates(threshold)?;
            self.finalize_semantic_chunks()?;
            self.connection
                .execute_batch("COMMIT")
                .map_err(AppError::database)?;
            self.writer_active = false;
        }
        Ok(())
    }

    pub(crate) fn truncate_wal(&self) -> Result<(), AppError> {
        self.connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .map_err(AppError::database)
    }

    #[tracing::instrument(name = "checkpoint_writer", skip_all)]
    pub(crate) fn checkpoint_writer(&mut self) -> Result<(), AppError> {
        self.commit_and_restart_writer("PRAGMA wal_checkpoint(PASSIVE); BEGIN IMMEDIATE")
    }

    pub(crate) fn commit_and_continue_writer(&mut self) -> Result<(), AppError> {
        self.commit_and_restart_writer("BEGIN IMMEDIATE")
    }

    fn commit_and_restart_writer(&mut self, continuation: &str) -> Result<(), AppError> {
        self.require_writer()?;
        let threshold = self.measured_fts_bulk_threshold()?;
        self.finalize_pending_fts_updates(threshold)?;
        self.finalize_semantic_chunks()?;
        self.connection
            .execute_batch("COMMIT")
            .map_err(AppError::database)?;
        self.writer_active = false;
        self.connection
            .execute_batch(continuation)
            .map_err(AppError::database)?;
        self.writer_active = true;
        self.transaction_base_messages = self.message_count()?;
        Ok(())
    }

    fn require_writer(&self) -> Result<(), AppError> {
        if self.writer_active {
            Ok(())
        } else {
            Err(AppError::internal(
                "index mutation requires an active writer",
            ))
        }
    }
}

impl Drop for Storage {
    fn drop(&mut self) {
        if self.writer_active {
            let _ = self.connection.execute_batch("ROLLBACK");
        }
    }
}

/// A column every native entry has; its absence means corrupt silver.
pub(crate) fn required<T>(value: Option<T>, field: &str) -> Result<T, AppError> {
    value.ok_or_else(|| AppError::database_data(format!("native silver is missing {field}")))
}

pub(crate) fn cutoff_timestamp(days: Option<u32>) -> Result<Option<i64>, AppError> {
    let Some(days) = days else {
        return Ok(None);
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| {
            AppError::internal(format!("system clock precedes Unix epoch: {error}"))
        })?;
    let age = std::time::Duration::from_secs(u64::from(days).saturating_mul(86_400));
    let cutoff = now.saturating_sub(age).as_millis();
    i64::try_from(cutoff)
        .map(Some)
        .map_err(|_| AppError::internal("timestamp exceeds SQLite range"))
}

#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;
