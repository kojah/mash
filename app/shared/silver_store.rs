//! Shared silver: the server's SQLite system of record for published node history.
//! Gold (vectors, search documents and their readiness) lives only in Qdrant and
//! can be rebuilt from these rows.
use super::protocol::{Checkpoint, ConversationRecord, MessageRecord, ScanReport, Tombstone};
use super::server::Identity;
use super::{silver_coverage::Coverage, silver_revision::State};
use crate::{AppError, provider::Provider, storage::Counts};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub(super) mod apply;
pub(super) mod read;
pub(super) use apply::Applied;

const SCHEMA_VERSION: i64 = 1;
const SCHEMA: &str = include_str!("silver_store/schema.sql");

// sqlc-gen-rust emits an unused row parameter for :exec result models.
#[allow(dead_code, unused_variables, clippy::all, clippy::pedantic)]
#[path = "silver_store/generated/queries.rs"]
mod generated;
use generated as sql;

/// Silver state of one node's publications; the upload counter and its token.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Head {
    pub(super) instance: String,
    pub(super) revision: i64,
    /// Changes with every committed publication, including gold-only rebuilds.
    pub(super) version: String,
    pub(super) silver_revision: Option<State>,
    pub(super) source_silver: Option<crate::silver::certificate::Certificate>,
    pub(super) silver_coverage: Vec<Coverage>,
    pub(super) silver_scans: Vec<ScanReport>,
    pub(super) last_synced_at: Option<String>,
    pub(super) last_updated_at: Option<String>,
}

/// One SQLite file; WAL readers run concurrently with the single writer.
#[derive(Clone)]
pub(super) struct SilverStore {
    path: PathBuf,
    writer: Arc<Mutex<()>>,
}

fn database(error: rusqlite::Error) -> AppError {
    AppError::database(error)
}

pub(super) fn key(provider: Provider, id: &str) -> String {
    format!("{provider}\0{id}")
}

pub(super) fn now() -> String {
    chrono::DateTime::from(std::time::SystemTime::now())
        .to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

pub(super) fn token() -> Result<String, AppError> {
    let mut random = [0u8; 32];
    getrandom::fill(&mut random).map_err(|_| AppError::internal("create publication token"))?;
    Ok(blake3::hash(&random).to_hex().to_string())
}

fn encode(value: &impl Serialize) -> Result<String, AppError> {
    serde_json::to_string(value).map_err(|_| AppError::internal("encode shared silver"))
}

fn decode<T: serde::de::DeserializeOwned>(json: &str) -> Result<T, AppError> {
    serde_json::from_str(json).map_err(|_| AppError::database_data("invalid shared silver row"))
}

impl SilverStore {
    /// Create or open the server database; only the API process writes it.
    pub(super) fn open(path: &Path) -> Result<Self, AppError> {
        if !path.is_absolute() {
            return Err(AppError::configuration(
                "silver_database must be an absolute path",
            ));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(AppError::io)?;
        }
        let connection = Connection::open(path).map_err(database)?;
        connection
            .execute_batch("PRAGMA journal_mode = WAL;")
            .map_err(database)?;
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(database)?;
        if version > SCHEMA_VERSION {
            return Err(AppError::schema(
                "shared silver database is newer than this version of mash",
            ));
        }
        if version < SCHEMA_VERSION {
            connection.execute_batch(SCHEMA).map_err(database)?;
            connection
                .pragma_update(None, "user_version", SCHEMA_VERSION)
                .map_err(database)?;
        }
        Ok(Self {
            path: path.to_path_buf(),
            writer: Arc::default(),
        })
    }

    /// Test-only store whose every access fails, like an unreachable backend.
    #[cfg(test)]
    pub(super) fn unavailable() -> Self {
        Self {
            path: PathBuf::from("/proc/mash-unavailable/silver.sqlite3"),
            writer: Arc::default(),
        }
    }

    pub(super) fn connect(&self) -> Result<Connection, AppError> {
        let connection = Connection::open(&self.path).map_err(database)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(30))
            .map_err(database)?;
        Ok(connection)
    }

    /// Serialize this process's writes; SQLite also locks across processes.
    pub(super) fn write<T>(
        &self,
        work: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let _writer = self
            .writer
            .lock()
            .map_err(|_| AppError::internal("shared silver writer is poisoned"))?;
        let mut connection = self.connect()?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(database)?;
        let result = work(&transaction)?;
        transaction.commit().map_err(database)?;
        Ok(result)
    }
}

fn scope(identity: &Identity) -> (&str, &str) {
    (&identity.group_id, &identity.node_id)
}

pub(super) fn head(connection: &Connection, identity: &Identity) -> Result<Option<Head>, AppError> {
    let (group, node) = scope(identity);
    sql::Head::builder()
        .group_id(group)
        .node_id(node)
        .build()
        .query_opt(connection)
        .map_err(database)?
        .map(|row| decode(&row.head))
        .transpose()
}

pub(super) fn heads(
    connection: &Connection,
    group: &str,
) -> Result<std::collections::BTreeMap<String, Head>, AppError> {
    sql::Heads::builder()
        .group_id(group)
        .build()
        .query_many(connection)
        .map_err(database)?
        .into_iter()
        .map(|row| Ok((row.node_id, decode(&row.head)?)))
        .collect()
}

pub(super) fn put_head(
    connection: &Connection,
    identity: &Identity,
    head: &Head,
) -> Result<(), AppError> {
    let (group, node) = scope(identity);
    sql::PutHead::builder()
        .group_id(group)
        .node_id(node)
        .head(&encode(head)?)
        .build()
        .execute(connection)
        .map_err(database)?;
    Ok(())
}

/// Silver counts. Embedding coverage belongs to gold and is reported separately.
pub(super) fn counts(connection: &Connection, identity: &Identity) -> Result<Counts, AppError> {
    let (group, node) = scope(identity);
    let row = sql::Counts::builder()
        .group_id(group)
        .node_id(node)
        .build()
        .query_one(connection)
        .map_err(database)?;
    let count = |value: i64| u64::try_from(value).map_err(|_| AppError::database_data("count"));
    Ok(Counts {
        conversations: count(row.conversations)?,
        messages: count(row.messages)?,
        searchable_messages: count(row.searchable)?,
        searchable_user_messages: Some(count(row.searchable_users)?),
        embeddings: 0,
    })
}

pub(super) fn conversation(
    connection: &Connection,
    identity: &Identity,
    id: &str,
) -> Result<Option<ConversationRecord>, AppError> {
    let (group, node) = scope(identity);
    sql::Conversation::builder()
        .group_id(group)
        .node_id(node)
        .id(id)
        .build()
        .query_opt(connection)
        .map_err(database)?
        .map(|row| decode(&row.record))
        .transpose()
}

/// One thread's silver messages in ordinal order. Vectors are never stored.
pub(super) fn thread_messages(
    connection: &Connection,
    identity: &Identity,
    id: &str,
) -> Result<Vec<MessageRecord>, AppError> {
    let (group, node) = scope(identity);
    sql::ThreadMessages::builder()
        .group_id(group)
        .node_id(node)
        .conversation_id(id)
        .build()
        .query_many(connection)
        .map_err(database)?
        .into_iter()
        .map(|row| decode(&row.record))
        .collect()
}

pub(super) fn message(
    connection: &Connection,
    identity: &Identity,
    id: &str,
) -> Result<Option<MessageRecord>, AppError> {
    let (group, node) = scope(identity);
    sql::Message::builder()
        .group_id(group)
        .node_id(node)
        .id(id)
        .build()
        .query_opt(connection)
        .map_err(database)?
        .map(|row| decode(&row.record))
        .transpose()
}

pub(super) fn sources(
    connection: &Connection,
    identity: &Identity,
) -> Result<Vec<Checkpoint>, AppError> {
    let (group, node) = scope(identity);
    sql::NodeSources::builder()
        .group_id(group)
        .node_id(node)
        .build()
        .query_many(connection)
        .map_err(database)?
        .into_iter()
        .map(|row| decode(&row.record))
        .collect()
}

pub(super) fn tombstones(
    connection: &Connection,
    identity: &Identity,
) -> Result<Vec<Tombstone>, AppError> {
    let (group, node) = scope(identity);
    sql::NodeTombstones::builder()
        .group_id(group)
        .node_id(node)
        .build()
        .query_many(connection)
        .map_err(database)?
        .into_iter()
        .map(|row| decode(&row.record))
        .collect()
}

/// Thread dependency fingerprints, the reuse identity offered to publishers.
pub(super) fn fingerprints(
    connection: &Connection,
    identity: &Identity,
) -> Result<std::collections::BTreeMap<String, String>, AppError> {
    let (group, node) = scope(identity);
    Ok(sql::Fingerprints::builder()
        .group_id(group)
        .node_id(node)
        .build()
        .query_many(connection)
        .map_err(database)?
        .into_iter()
        .map(|row| (row.id, row.fingerprint))
        .collect())
}

/// Messages eligible for dense vectors under this recipe.
pub(super) fn dense_messages(
    connection: &Connection,
    identity: &Identity,
    build: &crate::gold::config::Build,
) -> Result<u64, AppError> {
    counts(connection, identity)?
        .expected_dense(build)
        .ok_or_else(|| AppError::database_data("invalid shared silver counts"))
}

pub(super) fn providers(
    connection: &Connection,
    identity: &Identity,
) -> Result<Vec<Provider>, AppError> {
    let (group, node) = scope(identity);
    sql::Providers::builder()
        .group_id(group)
        .node_id(node)
        .build()
        .query_many(connection)
        .map_err(database)?
        .into_iter()
        .map(|row| row.provider.parse())
        .collect()
}

/// Whether provider scan evidence covers every retained thread with current sources.
pub(super) fn coverage_gaps(
    connection: &Connection,
    identity: &Identity,
    provider: Provider,
) -> Result<bool, AppError> {
    let (group, node) = scope(identity);
    let row = sql::CoverageGaps::builder()
        .group_id(group)
        .node_id(node)
        .provider(provider.as_str())
        .interpretation_version(i64::from(
            crate::silver::certificate::INTERPRETATION_VERSION,
        ))
        .build()
        .query_one(connection)
        .map_err(database)?;
    Ok(row.stale != 0 || row.unmapped != 0)
}
