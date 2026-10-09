//! Canonical connector records and transactional receipts in the common silver schema.
//! No vectors, lexical indexes, provider plugins or external code execute here.

use super::{Batch, Change, Id, Object, Receipt, Role, Schema, Version};
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
    /// An object that the source's declared schema does not admit.
    SchemaViolation,
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
        Error::InvalidBatch(_) | Error::InvalidReferences | Error::SchemaViolation => {
            AppError::usage(problem.to_string())
        }
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

    /// Fetch one canonical object by source and identity; this is not search.
    ///
    /// # Errors
    /// Returns an error for storage failures or malformed canonical rows.
    pub fn object(&self, source: &Id, id: &Id) -> Result<Option<Object>, Error> {
        sql::ConnectorObject::builder()
            .source_id(source.as_str())
            .id(id.as_str())
            .build()
            .query_opt(&self.connection)
            .map_err(database)?
            .map(|row| record(row.record.as_deref()))
            .transpose()
    }
}

/// A connector row's exact submitted object; every connector row has one.
fn record(value: Option<&str>) -> Result<Object, Error> {
    decode(value.ok_or(Error::CorruptRecord)?)
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
    // Parents may arrive later in the batch; the edge constraint is deferred
    // and the stored graph is checked once every change is applied. Silver
    // revision triggers record whether anything actually changed.
    for change in &batch.changes {
        mutate(transaction, &batch.source_id, change)?;
    }
    let source = batch.source_id.as_str();
    let schema = stored_schema(transaction, source)?;
    let mut checked = BTreeSet::new();
    let ids = objects_to_check(transaction, batch, schema.is_some())?;
    for id in &ids {
        check_object(transaction, source, id, schema.as_ref(), &mut checked)?;
    }
    for id in &ids {
        group(transaction, source, id)?;
    }
    let silver_revision = sql::ConnectorSilverRevision
        .query_one(transaction)
        .map_err(database)?
        .revision;
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

/// Objects whose stored graph a batch can have changed.
fn objects_to_check(
    transaction: &Transaction<'_>,
    batch: &Batch,
    schema: bool,
) -> Result<Vec<String>, Error> {
    let source = batch.source_id.as_str();
    // A new schema must hold for everything the source already stored.
    let ids = if batch
        .changes
        .iter()
        .any(|change| matches!(change, Change::DeclareSchema(_)))
    {
        sql::ConnectorSourceObjects::builder()
            .source_id(source)
            .build()
            .query_many(transaction)
            .map_err(database)?
            .into_iter()
            .map(|row| row.id)
            .collect()
    } else {
        let mut ids = Vec::new();
        for change in &batch.changes {
            let Change::UpsertObject(object) = change else {
                continue;
            };
            ids.push(object.id.as_str().to_owned());
            // A parent's new kind must still admit its existing children.
            if schema {
                ids.extend(
                    sql::ConnectorChildren::builder()
                        .source_id(source)
                        .parent_id(Some(object.id.as_str()))
                        .build()
                        .query_many(transaction)
                        .map_err(database)?
                        .into_iter()
                        .map(|row| row.id),
                );
            }
        }
        ids
    };
    Ok(ids)
}

pub(crate) fn role_name(role: Role) -> &'static str {
    match role {
        Role::Container => "container",
        Role::Segment => "segment",
        Role::Entry => "entry",
        Role::Document => "document",
    }
}

fn parse_role(role: &str) -> Result<Role, Error> {
    match role {
        "container" => Ok(Role::Container),
        "segment" => Ok(Role::Segment),
        "entry" => Ok(Role::Entry),
        "document" => Ok(Role::Document),
        _ => Err(Error::CorruptRecord),
    }
}

fn mutate(transaction: &Transaction<'_>, source: &Id, change: &Change) -> Result<(), Error> {
    match change {
        Change::UpsertObject(object) => put_object(transaction, source, object)?,
        // Descendants go with it through the cascading parent edge.
        Change::DeleteObject { id } => {
            sql::ConnectorDeleteObject::builder()
                .source_id(source.as_str())
                .id(id.as_str())
                .build()
                .execute(transaction)
                .map_err(database)?;
        }
        Change::DeclareSchema(schema) => {
            sql::ConnectorPutSchema::builder()
                .source_id(source.as_str())
                .schema(&encode(schema)?)
                .build()
                .execute(transaction)
                .map_err(database)?;
        }
    }
    Ok(())
}

/// Store one object exactly as submitted; grouping happens after the batch.
pub(crate) fn put_object(
    connection: &Connection,
    source: &Id,
    object: &Object,
) -> Result<(), Error> {
    sql::ConnectorPutObject::builder()
        .source_id(source.as_str())
        .id(object.id.as_str())
        .role(role_name(object.role))
        .kind(object.kind.as_str())
        .parent_id(object.parent.as_ref().map(Id::as_str))
        .reply_to(object.reply_to.as_ref().map(Id::as_str))
        .author(object.author.as_ref().map(encode).transpose()?.as_deref())
        .title(object.title.as_deref())
        .content(object.content.as_deref())
        .created_at(object.created_at)
        .updated_at(object.updated_at)
        .url(object.url.as_deref())
        .metadata(object.metadata.as_ref().map(encode).transpose()?.as_deref())
        .attachments(&encode(&object.attachments)?)
        .record(Some(&encode(object)?))
        .build()
        .execute(connection)
        .map_err(database)?;
    // Link the edge both ways now, so deleting a parent later in the same
    // batch still takes children that arrived ahead of it.
    sql::ConnectorLinkParent::builder()
        .source_id(source.as_str())
        .id(object.id.as_str())
        .build()
        .execute(connection)
        .map_err(database)?;
    sql::ConnectorLinkChildren::builder()
        .source_id(source.as_str())
        .id(object.id.as_str())
        .build()
        .execute(connection)
        .map_err(database)?;
    Ok(())
}

/// Record an object's nearest container, and its non-container descendants'.
/// Moving a segment or entry regroups everything below it.
fn group(connection: &Connection, source: &str, id: &str) -> Result<(), Error> {
    let load = |id: &str| {
        sql::ConnectorObject::builder()
            .source_id(source)
            .id(id)
            .build()
            .query_opt(connection)
            .map_err(database)
    };
    let Some(row) = load(id)? else {
        return Ok(()); // Deleted later in the same batch.
    };
    let role = parse_role(&row.role)?;
    let mut nearest = id.to_owned();
    if matches!(role, Role::Segment | Role::Entry) {
        let mut current = row.parent_id;
        while let Some(parent) = current.take() {
            let parent_row = load(&parent)?.ok_or(Error::InvalidReferences)?;
            if parse_role(&parent_row.role)? == Role::Container {
                nearest = parent;
                break;
            }
            current = parent_row.parent_id;
        }
    }
    let mut pending = vec![(id.to_owned(), role, nearest)];
    while let Some((id, role, nearest)) = pending.pop() {
        sql::ConnectorSetConversation::builder()
            .conversation_id(&nearest)
            .source_id(source)
            .id(&id)
            .build()
            .execute(connection)
            .map_err(database)?;
        // A container's own descendants group under it.
        let inherited = if role == Role::Container {
            id.clone()
        } else {
            nearest
        };
        for child in sql::ConnectorChildren::builder()
            .source_id(source)
            .parent_id(Some(id.as_str()))
            .build()
            .query_many(connection)
            .map_err(database)?
        {
            let child_role = parse_role(&child.role)?;
            if child_role != Role::Container {
                pending.push((child.id, child_role, inherited.clone()));
            }
        }
    }
    Ok(())
}

/// The stored graph around one object: its parent exists and is not a document,
/// segments and entries sit under a container, documents have no children, and
/// neither the parent chain nor the reply chain loops. Reply targets may be
/// outside the stored window, but stored ones must be entries.
fn stored_schema(connection: &Connection, source: &str) -> Result<Option<Schema>, Error> {
    sql::ConnectorSchema::builder()
        .source_id(source)
        .build()
        .query_opt(connection)
        .map_err(database)?
        .map(|row| decode(&row.schema))
        .transpose()
}

fn check_object(
    connection: &Connection,
    source: &str,
    id: &str,
    schema: Option<&Schema>,
    checked: &mut BTreeSet<(String, String)>,
) -> Result<(), Error> {
    if !checked.insert((source.to_owned(), id.to_owned())) {
        return Ok(());
    }
    let row = sql::ConnectorObject::builder()
        .source_id(source)
        .id(id)
        .build()
        .query_opt(connection)
        .map_err(database)?
        .ok_or(Error::InvalidReferences)?;
    let role = parse_role(&row.role)?;
    if let Some(schema) = schema {
        let object = record(row.record.as_deref())?;
        let parent_kind = match &row.parent_id {
            None => None,
            Some(parent) => {
                let parent = sql::ConnectorObject::builder()
                    .source_id(source)
                    .id(parent)
                    .build()
                    .query_opt(connection)
                    .map_err(database)?
                    .ok_or(Error::InvalidReferences)?;
                Some(record(parent.record.as_deref())?.kind)
            }
        };
        if !schema.admits(&object.kind, role, parent_kind.as_ref()) {
            return Err(Error::SchemaViolation);
        }
    }
    if role == Role::Document
        && !sql::ConnectorChildren::builder()
            .source_id(source)
            .parent_id(Some(id))
            .build()
            .query_many(connection)
            .map_err(database)?
            .is_empty()
    {
        return Err(Error::InvalidReferences);
    }
    let mut visited = BTreeSet::from([id.to_owned()]);
    let mut contained = false;
    let mut current = row.parent_id;
    while let Some(parent) = current {
        if !visited.insert(parent.clone()) {
            return Err(Error::InvalidReferences);
        }
        let parent = sql::ConnectorObject::builder()
            .source_id(source)
            .id(&parent)
            .build()
            .query_opt(connection)
            .map_err(database)?
            .ok_or(Error::InvalidReferences)?;
        match parse_role(&parent.role)? {
            Role::Document => return Err(Error::InvalidReferences),
            Role::Container => {
                contained = true;
                break;
            }
            Role::Segment | Role::Entry => current = parent.parent_id,
        }
    }
    if matches!(role, Role::Segment | Role::Entry) && !contained {
        return Err(Error::InvalidReferences);
    }
    let mut visited = BTreeSet::from([id.to_owned()]);
    let mut current = row.reply_to;
    while let Some(target) = current {
        if !visited.insert(target.clone()) {
            return Err(Error::InvalidReferences);
        }
        let Some(target) = sql::ConnectorObject::builder()
            .source_id(source)
            .id(&target)
            .build()
            .query_opt(connection)
            .map_err(database)?
        else {
            break; // The stored window may exclude a reply target.
        };
        if parse_role(&target.role)? != Role::Entry {
            return Err(Error::InvalidReferences);
        }
        current = target.reply_to;
    }
    Ok(())
}

pub(crate) fn validate_archive(connection: &Connection) -> Result<(), Error> {
    let mut schemas = std::collections::BTreeMap::new();
    for row in sql::ConnectorAllSchemas
        .query_many(connection)
        .map_err(database)?
    {
        let schema: Schema = decode(&row.schema)?;
        schema.validate().map_err(Error::InvalidBatch)?;
        schemas.insert(row.source_id, schema);
    }
    let mut checked = BTreeSet::new();
    for row in sql::ConnectorAllObjects
        .query_many(connection)
        .map_err(database)?
    {
        let schema = schemas.get(&row.source_id);
        check_object(connection, &row.source_id, &row.id, schema, &mut checked)?;
        // Each container regroups its subtree; parentless objects group alone.
        if row.role == "container" || row.parent_id.is_none() {
            group(connection, &row.source_id, &row.id)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
