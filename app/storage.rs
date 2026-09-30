use crate::provider::Provider;
use std::fs;
use std::path::Path;
use std::time::Duration;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::AppError;

mod indexing;
pub(crate) mod publication;
mod queries;
mod schema;
mod semantic;
mod semantic_chunks;

#[cfg(test)]
use schema::SCHEMA_VERSION;
use schema::initialize;
#[cfg(test)]
use semantic_chunks::validate_quantized_vector;

pub(crate) struct Storage {
    connection: Connection,
    writer_active: bool,
    defer_search_updates: bool,
    transaction_base_messages: u64,
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
    pub(crate) embeddings: u64,
}

pub(crate) struct StatusSnapshot {
    pub(crate) counts: Counts,
    pub(crate) current_embeddings: u64,
    pub(crate) derived_clean: bool,
    pub(crate) exact_semantic_coverage: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct SearchHit {
    pub(crate) id: String,
    pub(crate) conversation_id: String,
    pub(crate) provider: Provider,
    pub(crate) role: crate::role::Role,
    pub(crate) content: String,
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
    pub(crate) id: String,
    pub(crate) ordinal: i64,
    pub(crate) role: crate::role::Role,
    pub(crate) content: String,
    pub(crate) created_at: Option<i64>,
}

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
            connection,
            writer_active: false,
            defer_search_updates: false,
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
mod tests;
