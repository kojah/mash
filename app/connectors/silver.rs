//! Canonical connector records and transactional receipts in the common silver schema.
//! No vectors, lexical indexes, provider plugins or external code execute here.

use super::{Batch, Change, Chat, Document, Id, Message, Receipt, Version};
use crate::storage::generated as sql;
use rusqlite::{Connection, Transaction, TransactionBehavior};
use serde::{Serialize, de::DeserializeOwned};
use std::{collections::BTreeSet, path::Path, time::Duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidBatch(super::Error),
    Database,
    UnsupportedSchema,
    CorruptRecord,
    RevisionConflict,
    BatchConflict,
    RevisionExhausted,
    InvalidReferences,
}

impl std::fmt::Display for Error {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(output, "connector ingestion failed: {self:?}")
    }
}
impl std::error::Error for Error {}

fn database(_: rusqlite::Error) -> Error {
    Error::Database
}

pub(crate) fn app_error(problem: Error) -> crate::AppError {
    use crate::{AppError, ErrorKind};
    match problem {
        Error::BatchConflict | Error::RevisionConflict => {
            AppError::new(12, ErrorKind::Conflict, problem.to_string(), false)
        }
        Error::InvalidBatch(_) | Error::InvalidReferences => AppError::usage(problem.to_string()),
        Error::UnsupportedSchema => AppError::schema(problem.to_string()),
        Error::Database => AppError::new(5, ErrorKind::Database, problem.to_string(), true),
        Error::CorruptRecord | Error::RevisionExhausted => {
            AppError::database_data(problem.to_string())
        }
    }
}
fn encode(value: &impl Serialize) -> Result<String, Error> {
    serde_json::to_string(value).map_err(|_| Error::CorruptRecord)
}
fn decode<T: DeserializeOwned>(value: &str) -> Result<T, Error> {
    serde_json::from_str(value).map_err(|_| Error::CorruptRecord)
}

/// A canonical-only reader/writer. Local and shared callers use the same schema.
pub struct Store {
    connection: Connection,
}

impl Store {
    /// Open fresh/current silver. Another release's schema is never migrated.
    ///
    /// # Errors
    /// Rejects incompatible schemas and inaccessible databases.
    pub fn open(path: &Path) -> Result<Self, Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| Error::Database)?;
        }
        let connection = Connection::open(path).map_err(database)?;
        connection
            .busy_timeout(Duration::from_secs(30))
            .map_err(database)?;
        crate::storage::schema::initialize_silver(&connection).map_err(|error| {
            if error.error.kind == crate::ErrorKind::SchemaIncompatible {
                Error::UnsupportedSchema
            } else {
                Error::Database
            }
        })?;
        Ok(Self { connection })
    }

    /// Mutations, source ordering and the receipt become durable together.
    ///
    /// # Errors
    /// Rejects invalid data, conflicting identities/revisions and invalid replies.
    pub fn apply(&mut self, batch: &Batch) -> Result<Receipt, Error> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database)?;
        let receipt = apply(&transaction, batch)?;
        transaction.commit().map_err(database)?;
        Ok(receipt)
    }

    /// Fetch one canonical chat by source and identity; this is not search.
    ///
    /// # Errors
    /// Returns an error for storage failures or malformed canonical rows.
    pub fn chat(&self, source: &Id, id: &Id) -> Result<Option<Chat>, Error> {
        sql::ConnectorChat::builder()
            .source_id(source.as_str())
            .id(id.as_str())
            .build()
            .query_opt(&self.connection)
            .map_err(database)?
            .map(|row| decode(&row.record))
            .transpose()
    }

    /// Fetch one canonical message by source and identity.
    ///
    /// # Errors
    /// Returns an error for storage failures or malformed canonical rows.
    pub fn message(&self, source: &Id, id: &Id) -> Result<Option<Message>, Error> {
        sql::ConnectorMessage::builder()
            .source_id(source.as_str())
            .id(id.as_str())
            .build()
            .query_opt(&self.connection)
            .map_err(database)?
            .map(|row| decode(&row.record))
            .transpose()
    }

    /// Fetch one standalone canonical document by source and identity.
    ///
    /// # Errors
    /// Returns an error for storage failures or malformed canonical rows.
    pub fn document(&self, source: &Id, id: &Id) -> Result<Option<Document>, Error> {
        sql::ConnectorDocument::builder()
            .source_id(source.as_str())
            .id(id.as_str())
            .build()
            .query_opt(&self.connection)
            .map_err(database)?
            .map(|row| decode(&row.record))
            .transpose()
    }
}

/// Shared service callers can supply their existing per-node silver transaction.
///
/// # Errors
/// Rejects invalid batches and state conflicts; the caller must roll back on error.
pub(crate) fn apply(transaction: &Transaction<'_>, batch: &Batch) -> Result<Receipt, Error> {
    batch.validate().map_err(Error::InvalidBatch)?;
    let fingerprint = blake3::hash(encode(batch)?.as_bytes()).to_hex().to_string();
    if let Some(stored) = sql::ConnectorReceipt::builder()
        .source_id(batch.source_id.as_str())
        .batch_id(batch.batch_id.as_str())
        .build()
        .query_opt(transaction)
        .map_err(database)?
    {
        if stored.fingerprint != fingerprint {
            return Err(Error::BatchConflict);
        }
        return decode(&stored.receipt);
    }
    let revision = sql::ConnectorRevision::builder()
        .id(batch.source_id.as_str())
        .build()
        .query_opt(transaction)
        .map_err(database)?
        .map_or(Ok(0), |row| {
            row.revision
                .parse::<u64>()
                .map_err(|_| Error::CorruptRecord)
        })?;
    if revision != batch.expected_revision {
        return Err(Error::RevisionConflict);
    }
    let next_revision = revision.checked_add(1).ok_or(Error::RevisionExhausted)?;
    sql::ConnectorSource::builder()
        .id(batch.source_id.as_str())
        .revision(&next_revision.to_string())
        .build()
        .execute(transaction)
        .map_err(database)?;
    let mut changed = false;
    // Parents can arrive later in the same batch. Create chats before messages.
    for change in &batch.changes {
        if let Change::UpsertChat(chat) = change {
            changed |= sql::ConnectorPutChat::builder()
                .source_id(batch.source_id.as_str())
                .id(chat.id.as_str())
                .record(&encode(chat)?)
                .build()
                .execute(transaction)
                .map_err(database)?
                > 0;
        }
    }
    for change in &batch.changes {
        changed |= mutate(transaction, &batch.source_id, change)?;
    }
    validate_stored_replies(transaction, batch)?;
    let current = sql::ConnectorSilverRevision
        .query_one(transaction)
        .map_err(database)?
        .revision;
    let silver_revision = if changed {
        current.checked_add(1).ok_or(Error::RevisionExhausted)?
    } else {
        current
    };
    if changed {
        sql::ConnectorSetSilverRevision::builder()
            .revision(silver_revision)
            .build()
            .execute(transaction)
            .map_err(database)?;
    }
    let receipt = Receipt {
        schema_version: Version,
        source_id: batch.source_id.clone(),
        batch_id: batch.batch_id.clone(),
        source_revision: next_revision,
        silver_revision: u64::try_from(silver_revision).map_err(|_| Error::CorruptRecord)?,
        // This boundary commits silver only. Gold publication has its own gate.
        gold_ready: false,
    };
    sql::ConnectorPutReceipt::builder()
        .source_id(batch.source_id.as_str())
        .batch_id(batch.batch_id.as_str())
        .fingerprint(&fingerprint)
        .receipt(&encode(&receipt)?)
        .build()
        .execute(transaction)
        .map_err(database)?;
    Ok(receipt)
}

fn mutate(transaction: &Transaction<'_>, source: &Id, change: &Change) -> Result<bool, Error> {
    let rows = match change {
        Change::UpsertChat(_) => 0,
        Change::UpsertMessage(message) => {
            if sql::ConnectorChat::builder()
                .source_id(source.as_str())
                .id(message.chat_id.as_str())
                .build()
                .query_opt(transaction)
                .map_err(database)?
                .is_none()
            {
                return Err(Error::InvalidReferences);
            }
            sql::ConnectorPutMessage::builder()
                .source_id(source.as_str())
                .id(message.id.as_str())
                .chat_id(message.chat_id.as_str())
                .reply_to(message.reply_to.as_ref().map(Id::as_str))
                .record(&encode(message)?)
                .build()
                .execute(transaction)
                .map_err(database)?
        }
        Change::UpsertDocument(document) => sql::ConnectorPutDocument::builder()
            .source_id(source.as_str())
            .id(document.id.as_str())
            .record(&encode(document)?)
            .build()
            .execute(transaction)
            .map_err(database)?,
        Change::DeleteChat { id } => sql::ConnectorDeleteChat::builder()
            .source_id(source.as_str())
            .id(id.as_str())
            .build()
            .execute(transaction)
            .map_err(database)?,
        Change::DeleteMessage { id } => sql::ConnectorDeleteMessage::builder()
            .source_id(source.as_str())
            .id(id.as_str())
            .build()
            .execute(transaction)
            .map_err(database)?,
        Change::DeleteDocument { id } => sql::ConnectorDeleteDocument::builder()
            .source_id(source.as_str())
            .id(id.as_str())
            .build()
            .execute(transaction)
            .map_err(database)?,
    };
    Ok(rows > 0)
}

type Completed = std::collections::BTreeMap<(String, String), BTreeSet<String>>;

fn validate_stored_replies(transaction: &Transaction<'_>, batch: &Batch) -> Result<(), Error> {
    let mut completed = Completed::new();
    for change in &batch.changes {
        let Change::UpsertMessage(message) = change else {
            continue;
        };
        verify_reply(
            transaction,
            batch.source_id.as_str(),
            message.chat_id.as_str(),
            message.id.as_str(),
            &mut completed,
        )?;
    }
    Ok(())
}

fn verify_reply(
    connection: &Connection,
    source: &str,
    chat: &str,
    message: &str,
    completed: &mut Completed,
) -> Result<(), Error> {
    // Moving a parent cannot leave existing children in a different chat.
    for child in sql::ConnectorChildren::builder()
        .source_id(source)
        .id(Some(message))
        .build()
        .query_many(connection)
        .map_err(database)?
    {
        if child.chat_id != chat {
            return Err(Error::InvalidReferences);
        }
    }
    let completed = completed
        .entry((source.to_owned(), chat.to_owned()))
        .or_default();
    let mut visited = BTreeSet::new();
    let mut current = Some(message.to_owned());
    while let Some(id) = current {
        if completed.contains(&id) {
            break;
        }
        if !visited.insert(id.clone()) {
            return Err(Error::InvalidReferences);
        }
        let edge = sql::ConnectorEdge::builder()
            .source_id(source)
            .id(&id)
            .build()
            .query_opt(connection)
            .map_err(database)?;
        let Some(edge) = edge else { break }; // The selected window may exclude a parent.
        if edge.chat_id != chat {
            return Err(Error::InvalidReferences);
        }
        current = edge.reply_to;
    }
    completed.extend(visited);
    Ok(())
}

pub(crate) fn validate_archive(connection: &Connection) -> Result<(), Error> {
    let mut completed = Completed::new();
    let mut statement = sql::ConnectorAllEdges
        .prepare(connection)
        .map_err(database)?;
    let rows = statement
        .query_map([], sql::ConnectorAllEdgesRow::from_row)
        .map_err(database)?;
    for row in rows {
        let row = row.map_err(database)?;
        verify_reply(
            connection,
            &row.source_id,
            &row.chat_id,
            &row.id,
            &mut completed,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
