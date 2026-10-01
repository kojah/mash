//! Redis connection/schema setup and internal snapshot representation.
mod catalog;
mod fields;
mod metadata;
mod payload;
mod publish;
mod read;
mod response;
mod vector_index;

pub(super) use vector_index::VectorIndex;

#[cfg(test)]
#[path = "redis_index/tests/catalog.rs"]
mod catalog_tests;
#[cfg(test)]
#[path = "redis_index/tests/vector_index.rs"]
mod vector_index_tests;

use crate::provider::Provider;
use std::time::Duration;

use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};

use super::backend::{conflict, valid_id};
use super::protocol::{
    self, ConversationRecord, Forget, Forgotten, Indexed, MessageRecord, NodeState, Search,
};
use super::server::Identity;
use super::snapshot::{Data, Head, Loaded, Plan};
use super::{Freshness, NodeFreshness};
use crate::AppError;
#[cfg(test)]
use crate::semantic;
use crate::storage::{Counts, Message, SearchHit};

// Bound uploads and canonical node snapshots to the configured memory budget.
const STAGE_TTL: u32 = 600;
const CANONICAL_CHUNK_BYTES: usize = 8 * 1024 * 1024;

#[allow(clippy::cast_possible_truncation)]
fn vector_bytes(vector: &[f64]) -> Vec<u8> {
    vector
        .iter()
        .flat_map(|v| (*v as f32).to_le_bytes())
        .collect()
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CanonicalManifest {
    bytes: usize,
    chunks: usize,
    digest: String,
}

#[derive(Clone)]
pub(super) struct Store {
    connection: ConnectionManager,
    max_node_bytes: usize,
    vector_index: VectorIndex,
}
async fn extend_metadata_schema(
    connection: &mut ConnectionManager,
    name: &str,
) -> Result<(), AppError> {
    let info: redis::Value = redis::cmd("FT.INFO")
        .arg(name)
        .query_async(connection)
        .await
        .map_err(redis_error)?;
    let mut fields = std::collections::HashSet::new();
    schema_fields(&info, &mut fields);
    for (field, kind) in [
        (fields::NODE_TAG, "TAG"),
        (fields::ROLE_TAG, "TAG"),
        (fields::PROJECT_TAG, "TAG"),
        (fields::REPOSITORY_TAG, "TAG"),
        (fields::DATED, "NUMERIC"),
        (fields::CONTEXT_PARENT, "NUMERIC"),
    ] {
        if !fields.contains(field) {
            let result = redis::cmd("FT.ALTER")
                .arg(name)
                .arg("SCHEMA")
                .arg("ADD")
                .arg(field)
                .arg(kind)
                .query_async::<()>(connection)
                .await;
            if let Err(error) = result {
                // A second server may have added the same field concurrently.
                let info = redis::cmd("FT.INFO")
                    .arg(name)
                    .query_async::<redis::Value>(connection)
                    .await
                    .map_err(redis_error)?;
                fields.clear();
                schema_fields(&info, &mut fields);
                if !fields.contains(field) {
                    return Err(redis_error(error));
                }
            }
        }
    }
    Ok(())
}

fn schema_fields(value: &redis::Value, names: &mut std::collections::HashSet<String>) {
    if let redis::Value::Array(values) = value {
        for pair in values.windows(2) {
            if redis::from_redis_value::<String>(pair[0].clone()).is_ok_and(|s| s == "identifier")
                && let Ok(name) = redis::from_redis_value::<String>(pair[1].clone())
            {
                names.insert(name);
            }
        }
        for value in values {
            schema_fields(value, names);
        }
    }
}

fn prefix(group: &str) -> String {
    format!("mash:r:{}:", blake3::hash(group.as_bytes()).to_hex())
}
fn index_name(group: &str) -> String {
    format!("{}index", prefix(group))
}
fn document_prefix(group: &str) -> String {
    format!("{}message:", prefix(group))
}
fn document_key(group: &str, version: &str, id: &str) -> String {
    format!(
        "{}{version}:{}",
        document_prefix(group),
        blake3::hash(id.as_bytes()).to_hex()
    )
}
fn state_key(group: &str, version: &str) -> String {
    format!("{}stage:{version}:state", prefix(group))
}
fn data_key(group: &str, version: &str) -> String {
    format!("{}stage:{version}:data", prefix(group))
}
fn catalog_key(group: &str, version: &str) -> String {
    format!("{}stage:{version}:catalog", prefix(group))
}
fn encode(value: &impl Serialize) -> Result<String, AppError> {
    serde_json::to_string(value).map_err(|_| AppError::internal("serialize Redis index data"))
}
fn decode<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, AppError> {
    serde_json::from_str(value).map_err(|_| {
        AppError::shared("invalid Redis index data; restore the index or reindex to a new node_id")
    })
}
fn redis_error(error: redis::RedisError) -> AppError {
    let result = match error.detail().or_else(|| error.code()) {
        Some("MASH_CONFLICT") => conflict("concurrent shared update; retry the operation"),
        Some("MASH_INCOMPATIBLE") => AppError::shared(
            "shared embeddings are incomplete or incompatible; run mash index on publishing nodes",
        ),
        _ => AppError::shared(
            "Redis index operation failed; check service availability, memory, and Redis Search support",
        ),
    };
    drop(error);
    result
}
fn timestamp(value: Option<String>) -> Result<Option<String>, AppError> {
    value
        .map(|value| {
            let (seconds, micros) = value
                .split_once('.')
                .ok_or_else(|| AppError::shared("invalid Redis synchronization timestamp"))?;
            let seconds = seconds
                .parse()
                .map_err(|_| AppError::shared("invalid Redis synchronization timestamp"))?;
            let micros: u32 = micros
                .parse()
                .map_err(|_| AppError::shared("invalid Redis synchronization timestamp"))?;
            if micros >= 1_000_000 {
                return Err(AppError::shared("invalid Redis synchronization timestamp"));
            }
            chrono::DateTime::from_timestamp(seconds, micros * 1000)
                .map(|time| time.to_rfc3339_opts(chrono::SecondsFormat::Micros, true))
                .ok_or_else(|| AppError::shared("invalid Redis synchronization timestamp"))
        })
        .transpose()
}
impl Store {
    #[tracing::instrument(name = "redis_index_initialize", skip_all, fields(vector_algorithm = vector_index.algorithm()))]
    pub(super) async fn connect(
        url: &str,
        groups: &[String],
        dimensions: usize,
        max_node_bytes: usize,
        vector_index: VectorIndex,
    ) -> Result<Self, AppError> {
        vector_index.validate()?;
        let client = redis::Client::open(url)
            .map_err(|_| AppError::configuration("invalid server Redis connection string"))?;
        let config = redis::aio::ConnectionManagerConfig::new()
            .set_connection_timeout(Some(Duration::from_secs(5)))
            .set_response_timeout(Some(Duration::from_secs(30)));
        let mut connection = client
            .get_connection_manager_with_config(config)
            .await
            .map_err(redis_error)?;
        for group in groups {
            let dimension_key = format!("{}dimensions", prefix(group));
            let name = index_name(group);
            let exists: Vec<String> = redis::cmd("FT._LIST")
                .query_async(&mut connection)
                .await
                .map_err(redis_error)?;
            let recorded: Option<usize> = redis::cmd("GET")
                .arg(&dimension_key)
                .query_async(&mut connection)
                .await
                .map_err(redis_error)?;
            // Indexes created before dimension metadata existed were always 384.
            if recorded.is_none() && exists.contains(&name) && dimensions != 384 {
                return Err(AppError::configuration(
                    "existing Redis group uses 384 dimensions; choose a new group_id",
                ));
            }
            redis::cmd("SET")
                .arg(&dimension_key)
                .arg(dimensions)
                .arg("NX")
                .query_async::<Option<String>>(&mut connection)
                .await
                .map_err(redis_error)?;
            let installed: usize = redis::cmd("GET")
                .arg(&dimension_key)
                .query_async(&mut connection)
                .await
                .map_err(redis_error)?;
            if installed != dimensions {
                return Err(AppError::configuration(
                    "Redis group uses different embedding dimensions; use a new group_id when changing dimensions",
                ));
            }
            if !exists.contains(&name) {
                let created = vector_index
                    .create(&mut connection, &name, &document_prefix(group), dimensions)
                    .await;
                if created.is_err() {
                    let exists: Vec<String> = redis::cmd("FT._LIST")
                        .query_async(&mut connection)
                        .await
                        .map_err(redis_error)?;
                    if !exists.contains(&name) {
                        created.map_err(redis_error)?;
                    }
                }
            }
            vector_index
                .wait_until_ready(&mut connection, &name, dimensions)
                .await?;
            extend_metadata_schema(&mut connection, &name).await?;
        }
        Ok(Self {
            connection,
            max_node_bytes,
            vector_index,
        })
    }

    pub(super) fn max_node_bytes(&self) -> u64 {
        u64::try_from(self.max_node_bytes).unwrap_or(u64::MAX)
    }
    async fn head(&self, identity: &Identity) -> Result<(Option<Head>, String), AppError> {
        let raw: Option<String> = redis::cmd("HGET")
            .arg(format!("{}nodes", prefix(&identity.group_id)))
            .arg(&identity.node_id)
            .query_async(&mut self.connection.clone())
            .await
            .map_err(redis_error)?;
        Ok((
            raw.as_deref().map(decode).transpose()?,
            raw.unwrap_or_default(),
        ))
    }
    async fn load(&self, identity: &Identity) -> Result<Loaded, AppError> {
        let pending = self.pending_snapshot(identity).await?;
        crate::blocking(move || pending.decode()).await
    }
    async fn pending_snapshot(
        &self,
        identity: &Identity,
    ) -> Result<super::snapshot::PendingSnapshot, AppError> {
        let (head, expected) = self.head(identity).await?;
        let payload = if let Some(head) = &head {
            let payload = self
                .read_payload(&data_key(&identity.group_id, &head.version))
                .await;
            if self.head(identity).await?.1 != expected {
                return Err(conflict(
                    "shared node changed while reading; retry the operation",
                ));
            }
            Some(payload?)
        } else {
            None
        };
        Ok(super::snapshot::PendingSnapshot {
            payload,
            head,
            expected,
        })
    }
}
