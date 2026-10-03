//! Qdrant storage adapter: layered silver/gold revisions and gold query documents.
//! Message points are denormalized gold views; state chunks retain silver history.
mod api;
mod configuration;
mod context;
mod exact;
mod publish;
mod read;
mod silver;
#[cfg(test)]
mod tests;

use super::server::Identity;
use super::snapshot::{Head, Loaded};
use crate::AppError;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(super) enum MemoryTier {
    #[default]
    Cached,
    Cold,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "algorithm", rename_all = "lowercase", deny_unknown_fields)]
pub(super) enum VectorIndex {
    Exact {},
    Hnsw {
        #[serde(default = "default_ef")]
        ef_runtime: usize,
    },
}
impl Default for VectorIndex {
    fn default() -> Self {
        Self::Exact {}
    }
}
const fn default_ef() -> usize {
    200
}
impl VectorIndex {
    pub(super) fn validate(self) -> Result<(), AppError> {
        if matches!(self, Self::Hnsw { ef_runtime } if !(1..=4096).contains(&ef_runtime)) {
            return Err(AppError::configuration(
                "Qdrant ef_runtime must be between 1 and 4096",
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(super) struct Store {
    build: crate::gold::config::Build,
    api: api::Client,
    max_node_bytes: usize,
    vector_index: VectorIndex,
    snapshot_budget: super::snapshot_budget::Budget,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    bytes: usize,
    chunks: usize,
    digest: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NodeHead {
    kind: PointKind,
    node_id: String,
    version: String,
    head: Option<Head>,
    manifest: Option<Manifest>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Chunk {
    kind: PointKind,
    version: String,
    text: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: String,
    node_id: String,
    provider: crate::provider::Provider,
    role: crate::role::Role,
    conversation_id: String,
    project: Option<String>,
    repository: Option<String>,
    created_at: Option<i64>,
    tokens: Vec<String>,
    conversation: super::protocol::ConversationRecord,
    message: super::protocol::MessageRecord,
}
#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum PointKind {
    Head,
    Chunk,
    Catalog,
    Configuration,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CatalogRecord {
    kind: PointKind,
    version: String,
    node_id: String,
    conversation: super::protocol::ConversationRecord,
}
fn collection(group: &str, metadata: bool) -> String {
    format!(
        "mash_q_{}_{}",
        blake3::hash(group.as_bytes()).to_hex(),
        if metadata { "state" } else { "messages" }
    )
}
fn point_id(parts: &[&str]) -> String {
    let mut hash = blake3::Hasher::new();
    for part in parts {
        hash.update(&u64::try_from(part.len()).unwrap_or(u64::MAX).to_le_bytes());
        hash.update(part.as_bytes());
    }
    let hex = hash.finalize().to_hex();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}
fn failure() -> AppError {
    AppError::shared("Qdrant operation failed; check service availability, credentials, and schema")
}
fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase)
        .collect()
}
fn sparse(text: &str, query: bool) -> api::Sparse {
    use std::collections::BTreeMap;
    let words = tokens(text);
    let length = words.len();
    let mut counts = BTreeMap::<u32, u32>::new();
    for word in words {
        let hash = blake3::hash(word.as_bytes());
        let index = u32::from_le_bytes(hash.as_bytes()[..4].try_into().expect("four hash bytes"));
        *counts.entry(index).or_default() += 1;
    }
    #[allow(clippy::cast_precision_loss)]
    let norm = 1.2 * (0.25 + 0.75 * length as f32 / 256.0);
    api::Sparse {
        indices: counts.keys().copied().collect(),
        #[allow(clippy::cast_precision_loss)]
        values: counts
            .values()
            .map(|&tf| {
                if query {
                    1.0
                } else {
                    tf as f32 * 2.2 / (tf as f32 + norm)
                }
            })
            .collect(),
    }
}
impl Store {
    #[cfg(test)]
    pub(in crate::shared) fn unavailable() -> Self {
        Self {
            build: crate::gold::config::Build::default(),
            api: api::Client::new("http://127.0.0.1:9", None, 64 * 1024 * 1024).unwrap(),
            max_node_bytes: 64 * 1024 * 1024,
            vector_index: VectorIndex::default(),
            snapshot_budget: super::snapshot_budget::Budget::default(),
        }
    }
    pub(in crate::shared) fn with_snapshot_budget(
        mut self,
        budget: super::snapshot_budget::Budget,
    ) -> Self {
        self.snapshot_budget = budget;
        self
    }
    pub(in crate::shared) fn with_build(mut self, build: crate::gold::config::Build) -> Self {
        self.build = build;
        self
    }
    pub(super) fn max_node_bytes(&self) -> u64 {
        u64::try_from(self.max_node_bytes).unwrap_or(u64::MAX)
    }
    pub(super) async fn connect(
        url: &str,
        key: Option<&str>,
        groups: &[String],
        dimensions: usize,
        max_node_bytes: usize,
        vector_index: VectorIndex,
        memory: MemoryTier,
    ) -> Result<Self, AppError> {
        vector_index.validate()?;
        let api = api::Client::new(url, key, max_node_bytes)?;
        api.verify_version().await?;
        for group in groups {
            api.initialize(&collection(group, true), None, MemoryTier::Cached)
                .await?;
            api.initialize(&collection(group, false), Some(dimensions), memory)
                .await?;
        }
        Ok(Self {
            build: crate::gold::config::Build::default(),
            api,
            max_node_bytes,
            vector_index,
            snapshot_budget: super::snapshot_budget::Budget::default(),
        })
    }
    async fn node(&self, identity: &Identity) -> Result<NodeHead, AppError> {
        let mut nodes = configuration::catalogue_nodes(self.active_configuration(identity).await?)?;
        let node = nodes.remove(&identity.node_id).unwrap_or_else(|| NodeHead {
            kind: PointKind::Head,
            node_id: identity.node_id.clone(),
            version: String::new(),
            head: None,
            manifest: None,
        });
        if node.kind != PointKind::Head
            || node.node_id != identity.node_id
            || node
                .head
                .as_ref()
                .is_some_and(|h| h.version != node.version)
            || node.head.is_some() != node.manifest.is_some()
        {
            return Err(failure());
        }
        Ok(node)
    }
    async fn load(&self, identity: &Identity) -> Result<Loaded, AppError> {
        let pending = self.pending_snapshot(identity).await?;
        crate::blocking(move || pending.decode()).await
    }
    async fn pending_snapshot(
        &self,
        identity: &Identity,
    ) -> Result<super::snapshot::PendingSnapshot, AppError> {
        let node = self.node(identity).await?;
        let payload = if let Some(manifest) = &node.manifest {
            if manifest.bytes > self.max_node_bytes
                || manifest.chunks == 0
                || manifest.chunks > manifest.bytes
            {
                return Err(failure());
            }
            let payload = async {
                let mut receiver = super::snapshot::payload::Receiver::new(manifest.bytes).await?;
                for chunk in 0..manifest.chunks {
                    let id = point_id(&["chunk", &node.version, &chunk.to_string()]);
                    let points = self
                        .api
                        .retrieve::<Chunk>(&collection(&identity.group_id, true), &[id])
                        .await?;
                    let point = points.into_iter().next().ok_or_else(failure)?;
                    if point.payload.kind != PointKind::Chunk
                        || point.payload.version != node.version
                    {
                        return Err(failure());
                    }
                    receiver.append(point.payload.text.as_bytes()).await?;
                }
                receiver.finish(&manifest.digest).await
            }
            .await;
            self.unchanged(identity, &node.version).await?;
            Some(payload?)
        } else {
            None
        };
        Ok(super::snapshot::PendingSnapshot {
            payload,
            head: node.head,
            expected: node.version,
        })
    }
    async fn unchanged(&self, identity: &Identity, version: &str) -> Result<(), AppError> {
        if self.node(identity).await?.version != version {
            return Err(super::validation::conflict(
                "Qdrant node changed while reading; retry the operation",
            ));
        }
        Ok(())
    }
}
