//! Shared SQLite infrastructure for silver history and gold retrieval tables.
//! Canonical mutation also invalidates dependent gold in the same transaction;
//! gold/local owns rebuilding. See schema.sql for the physical layer sections.
use std::fs;
use std::path::Path;
use std::time::Duration;

use crate::AppError;
pub(crate) use crate::records::{Counts, Message, MessageContent, SearchHit};
use rusqlite::Connection;

// Gold views and materialized retrieval data.
mod catalog;
pub(crate) mod embedding_plan;
mod exact;
mod gold_manifest;
mod semantic;
mod semantic_chunks;

// Silver mutation/provenance plus atomic invalidation and publication ownership.
mod attachments;
mod claude_sources;
mod enrichment;
pub(crate) mod export;
mod indexing;
pub(crate) mod publication;
mod silver_state;

// Connection/schema and generated query infrastructure for both layers.
mod queries;
mod schema;

#[cfg(test)]
use schema::SCHEMA_VERSION;
// sqlc-gen-rust emits an unused row parameter for :exec result models.
#[allow(dead_code, unused_variables, clippy::all, clippy::pedantic)]
#[path = "storage/generated/queries.rs"]
mod generated;
use schema::initialize;
#[cfg(test)]
use semantic_chunks::validate_quantized_vector;

pub(crate) struct Storage {
    attachments_config: crate::attachments::Configuration,
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

pub(crate) struct StatusSnapshot {
    pub(crate) lexical: Option<crate::gold::lexical_manifest::Manifest>,
    pub(crate) gold: Option<crate::gold::manifest::Manifest>,
    pub(crate) silver: Option<crate::silver::certificate::Certificate>,
    pub(crate) counts: Counts,
    pub(crate) current_embeddings: u64,
    pub(crate) derived_clean: bool,
    pub(crate) exact_semantic_coverage: bool,
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
    pub(crate) fn open(path: &Path) -> Result<Self, AppError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(AppError::io)?;
        }
        let connection = Connection::open(path).map_err(AppError::database)?;
        initialize(&connection)?;
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

fn cutoff_timestamp(days: Option<u32>) -> Result<Option<i64>, AppError> {
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
