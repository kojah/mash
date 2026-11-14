//! Shared silver: the server's system of record for published node history.
//! Each node's history is its own SQLite database using the one silver schema
//! that every local index also uses. View (vectors, search documents and their
//! readiness) lives only in Qdrant and can be rebuilt from these databases.
use super::protocol::{ChatRecord, MessageRecord, Tombstone};
use super::server::Identity;
use super::silver_revision::State;
use crate::{AppError, provider::Provider, storage::Counts};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(crate) mod apply;
pub(crate) mod read;
pub(crate) use apply::Applied;

// sqlc-gen-rust emits an unused row parameter for :exec result models.
#[allow(dead_code, unused_variables, clippy::all, clippy::pedantic)]
#[path = "silver_store/generated/queries.rs"]
mod generated;
use generated as sql;

/// Server-only publication head of one node, kept in its `shared_head` row.
/// Fields of retired protocol features are ignored when read back.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Head {
    pub(crate) instance: String,
    pub(crate) revision: i64,
    /// Changes with every committed publication, including views-only rebuilds.
    pub(crate) version: String,
    pub(crate) silver_revision: Option<State>,
    pub(crate) source_silver: Option<crate::silver::certificate::Certificate>,
    pub(crate) last_synced_at: Option<String>,
    pub(crate) last_updated_at: Option<String>,
    /// Last `/v1/sync` operation committed in this same transaction. Absent for
    /// publications that arrived through the legacy HTTP mutation route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) operation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) manifest_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) receipt_digest: Option<String>,
}

/// Transfer receipt recorded with the silver commit it describes. The
/// transport catalog only tracks spool state; this is the canonical evidence.
#[derive(Clone, Debug)]
pub struct SyncReceipt {
    pub operation_id: String,
    pub manifest_digest: String,
}

/// Receipt identity over the transfer manifest and committed revision. The
/// client, the silver head and the transport catalog all use this function.
pub fn receipt_digest(manifest: &str, revision: i64) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"mash-sync-receipt-v1\0");
    hasher.update(manifest.as_bytes());
    hasher.update(&revision.to_le_bytes());
    hasher.finalize().to_hex().to_string()
}

/// One silver database per configured node under a root directory:
/// `<root>/<group_id>/<node_id>.sqlite3`. WAL readers run concurrently with
/// the node's single writer. One API process owns the directory at a time.
#[derive(Clone)]
pub struct SilverStore {
    root: PathBuf,
    groups: BTreeMap<String, Vec<String>>,
    /// None only for `unavailable`, whose every connection fails.
    owner: Option<std::sync::Arc<std::fs::File>>,
}

/// Exclusive ownership of a silver directory for this process's lifetime:
/// per-node publication locks and views synchronization are in-process only.
fn own(root: &Path) -> Result<std::fs::File, AppError> {
    std::fs::create_dir_all(root).map_err(AppError::io)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(root.join("serve.lock"))
        .map_err(AppError::io)?;
    match lock.try_lock() {
        Ok(()) => Ok(lock),
        Err(std::fs::TryLockError::WouldBlock) => Err(AppError::configuration(
            "another mash-server is already using this silver_dir",
        )),
        Err(std::fs::TryLockError::Error(error)) => Err(AppError::io(error)),
    }
}

fn database(error: rusqlite::Error) -> AppError {
    AppError::database(error)
}

#[cfg(test)]
pub(crate) fn key(provider: Provider, id: &str) -> String {
    format!("{provider}\0{id}")
}

pub(crate) fn now() -> String {
    chrono::DateTime::from(std::time::SystemTime::now())
        .to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

pub fn token() -> Result<String, AppError> {
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

fn provider(name: &str) -> Result<Provider, AppError> {
    name.parse()
        .map_err(|_| AppError::database_data("invalid shared silver provider"))
}

impl SilverStore {
    /// Create or open every configured node's database; only the API process
    /// writes them.
    pub fn open<'a>(
        root: &Path,
        identities: impl IntoIterator<Item = &'a Identity>,
    ) -> Result<Self, AppError> {
        if !root.is_absolute() {
            return Err(AppError::configuration(
                "silver_dir must be an absolute path",
            ));
        }
        let mut store = Self {
            root: root.to_path_buf(),
            groups: BTreeMap::new(),
            owner: Some(std::sync::Arc::new(own(root)?)),
        };
        for identity in identities {
            store
                .groups
                .entry(identity.group_id.clone())
                .or_default()
                .push(identity.node_id.clone());
            drop(store.connect(identity)?);
        }
        for nodes in store.groups.values_mut() {
            nodes.sort();
            nodes.dedup();
        }
        Ok(store)
    }

    /// Test-only store whose every access fails, like an unreachable backend.
    #[cfg(any(test, feature = "test-support"))]
    pub fn unavailable() -> Self {
        Self {
            root: PathBuf::from("/proc/mash-unavailable/silver"),
            groups: BTreeMap::new(),
            owner: None,
        }
    }

    fn path(&self, identity: &Identity) -> PathBuf {
        self.root
            .join(&identity.group_id)
            .join(format!("{}.sqlite3", identity.node_id))
    }

    /// Remove one node's published history, leaving an empty database with no
    /// publication head, as before the node first published. The caller holds
    /// the node's lock; this process owns the directory.
    pub(crate) fn retire(&self, identity: &Identity) -> Result<(), AppError> {
        if self.owner.is_none() {
            return Err(AppError::io(std::io::Error::other(
                "shared silver is unavailable",
            )));
        }
        let path = self.path(identity);
        for suffix in ["", "-wal", "-shm"] {
            let mut file = path.clone().into_os_string();
            file.push(suffix);
            match std::fs::remove_file(&file) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(AppError::io(error)),
            }
        }
        drop(self.connect(identity)?);
        Ok(())
    }

    /// Configured nodes of one group, in identity order.
    pub(crate) fn nodes(&self, group: &str) -> &[String] {
        self.groups.get(group).map_or(&[], Vec::as_slice)
    }

    pub(crate) fn connect(&self, identity: &Identity) -> Result<Connection, AppError> {
        // Explicit rather than an uncreatable root: any path is creatable on
        // some platform (Windows maps `/proc/...` to the current drive).
        if self.owner.is_none() {
            return Err(AppError::io(std::io::Error::other(
                "shared silver is unavailable",
            )));
        }
        let path = self.path(identity);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(AppError::io)?;
        }
        let mut connection = Connection::open(&path).map_err(database)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(30))
            .map_err(database)?;
        crate::storage::schema::initialize_silver(&mut connection)?;
        Ok(connection)
    }

    /// One transaction on a node's database; SQLite serializes its writers.
    pub(crate) fn write<T>(
        &self,
        identity: &Identity,
        work: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let mut connection = self.connect(identity)?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(database)?;
        let result = work(&transaction)?;
        transaction.commit().map_err(database)?;
        Ok(result)
    }

    /// Heads of the group's nodes that have published.
    pub(crate) fn heads(&self, group: &str) -> Result<BTreeMap<String, Head>, AppError> {
        let mut heads = BTreeMap::new();
        for node in self.nodes(group) {
            if let Some(head) = head(&self.connect(&node_identity(group, node))?)? {
                heads.insert(node.clone(), head);
            }
        }
        Ok(heads)
    }
}

/// Addressing identity of another node in the caller's group.
pub(crate) fn node_identity(group: &str, node: &str) -> Identity {
    Identity {
        group_id: group.to_owned(),
        node_id: node.to_owned(),
        generation: String::new(),
        dimensions: 0,
    }
}

pub(crate) fn head(connection: &Connection) -> Result<Option<Head>, AppError> {
    sql::Head
        .query_opt(connection)
        .map_err(database)?
        .map(|row| decode(&row.head))
        .transpose()
}

pub fn put_head(connection: &Connection, head: &Head) -> Result<(), AppError> {
    sql::PutHead::builder()
        .head(&encode(head)?)
        .build()
        .execute(connection)
        .map_err(database)?;
    Ok(())
}

/// Silver counts. Embedding coverage belongs to views and is reported separately.
pub(crate) fn counts(connection: &Connection) -> Result<Counts, AppError> {
    let row = sql::Counts.query_one(connection).map_err(database)?;
    let count = |value: i64| u64::try_from(value).map_err(|_| AppError::database_data("count"));
    Ok(Counts {
        chats: count(row.chats)?,
        messages: count(row.messages)?,
        searchable_messages: count(row.searchable)?,
        searchable_user_messages: Some(count(row.searchable_users)?),
        embeddings: 0,
    })
}

/// Typed silver columns back into the published record. This must be lossless:
/// thread fingerprints and silver revisions are computed from these records.
macro_rules! chat_record {
    ($row:expr) => {{
        let row = $row;
        Ok::<_, $crate::AppError>($crate::shared::protocol::ChatRecord {
            enrichment: row
                .enrichment
                .as_deref()
                .map($crate::shared::silver_store::decode::<$crate::silver::enrichment::Evidence>)
                .transpose()?,
            id: row.id,
            provider: $crate::shared::silver_store::provider(&row.provider)?,
            title: row.title,
            project: row.project,
            repository: row.repository,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }};
}

macro_rules! message_record {
    ($row:expr) => {{
        let row = $row;
        Ok::<_, $crate::AppError>($crate::shared::protocol::MessageRecord {
            attachments: $crate::shared::silver_store::decode(&row.attachments)?,
            id: row.id,
            chat_id: row.chat_id,
            ordinal: row.ordinal,
            context: $crate::message_context::MessageContext::from_parent(row.context_parent),
            role: $crate::role::Role::from(row.role),
            content: row.content,
            projection: row.search_projection.unwrap_or_default(),
            created_at: row.created_at,
            generation: None,
            vector: None,
            tools: $crate::tools::Activity::decode(row.metadata.as_deref())?,
        })
    }};
}

use message_record;

pub(crate) fn chat(connection: &Connection, id: &str) -> Result<Option<ChatRecord>, AppError> {
    sql::Chat::builder()
        .id(id)
        .build()
        .query_opt(connection)
        .map_err(database)?
        .map(|row| chat_record!(row))
        .transpose()
}

pub(crate) fn chats(connection: &Connection) -> Result<Vec<ChatRecord>, AppError> {
    sql::NodeChats
        .query_many(connection)
        .map_err(database)?
        .into_iter()
        .map(|row| chat_record!(row))
        .collect()
}

/// One thread's silver messages in ordinal order. Vectors are never stored.
pub(crate) fn chat_messages(
    connection: &Connection,
    id: &str,
) -> Result<Vec<MessageRecord>, AppError> {
    sql::ChatMessages::builder()
        .chat_id(id)
        .build()
        .query_many(connection)
        .map_err(database)?
        .into_iter()
        .map(|row| message_record!(row))
        .collect()
}

pub(crate) fn tombstones(connection: &Connection) -> Result<Vec<Tombstone>, AppError> {
    sql::NodeTombstones
        .query_many(connection)
        .map_err(database)?
        .into_iter()
        .map(|row| {
            Ok(Tombstone {
                provider: provider(&row.provider)?,
                chat_id: row.chat_id,
            })
        })
        .collect()
}

/// Thread dependency fingerprints, the reuse identity offered to publishers.
pub(crate) fn fingerprints(connection: &Connection) -> Result<BTreeMap<String, String>, AppError> {
    Ok(sql::Fingerprints
        .query_many(connection)
        .map_err(database)?
        .into_iter()
        .map(|row| (row.id, row.chat_fingerprint.unwrap_or_default()))
        .collect())
}

#[cfg(test)]
mod tests {
    #[test]
    fn one_process_owns_a_silver_directory() {
        let directory = tempfile::tempdir().unwrap();
        let identity = super::node_identity("group", "node");
        let first = super::SilverStore::open(directory.path(), [&identity]).unwrap();
        let second = super::SilverStore::open(directory.path(), [&identity]);
        assert!(
            second
                .err()
                .is_some_and(|error| error.error.message.contains("already using"))
        );
        drop(first);
        super::SilverStore::open(directory.path(), [&identity]).unwrap();
    }

    #[test]
    fn index_only_counts_run_on_a_fresh_node_database() {
        let directory = tempfile::tempdir().unwrap();
        let identity = super::node_identity("group", "node");
        let store = super::SilverStore::open(directory.path(), [&identity]).unwrap();
        let connection = store.connect(&identity).unwrap();
        let counts = super::counts(&connection).unwrap();
        assert_eq!((counts.chats, counts.messages), (0, 0));
    }

    #[test]
    fn listed_chats_name_their_first_message_by_display_position() {
        let directory = tempfile::tempdir().unwrap();
        let identity = super::node_identity("group", "node");
        let store = super::SilverStore::open(directory.path(), [&identity]).unwrap();
        let connection = store.connect(&identity).unwrap();
        connection
            .execute_batch(
                "INSERT INTO chats(id,provider) VALUES ('chat','codex'),('empty','codex');
                 INSERT INTO messages(id,chat_id,ordinal,role,content_packed)
                 VALUES ('later','chat',1,'assistant','b'),('opening','chat',0,'user','a');",
            )
            .unwrap();
        let chats = super::read::list_chats(&connection, "node").unwrap();
        let first = |id: &str| {
            chats
                .iter()
                .find(|chat| chat.chat.id == id)
                .unwrap()
                .first_message_id
                .clone()
        };
        assert_eq!(first("chat").as_deref(), Some("opening"));
        assert_eq!(first("empty"), None);
    }

    #[test]
    fn node_writers_commit_without_an_fsync_per_transaction() {
        let directory = tempfile::tempdir().unwrap();
        let identity = super::node_identity("group", "node");
        let store = super::SilverStore::open(directory.path(), [&identity]).unwrap();
        let connection = store.connect(&identity).unwrap();
        let sync: i64 = connection
            .query_row("PRAGMA synchronous", [], |row| row.get(0))
            .unwrap();
        assert_eq!(sync, 1);
    }
}
