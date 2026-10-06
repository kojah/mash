//! Qdrant shared gold: search documents and their readiness, derived from shared
//! silver in SQLite. Every point here can be rebuilt by replaying silver.
mod api;
mod configuration;
mod exact;
mod gold;
mod read;
#[cfg(test)]
mod tests;

use super::server::Identity;
use crate::AppError;
use serde::{Deserialize, Serialize};

pub(super) use gold::{GoldThread, NodeGold};
pub(super) use read::validate_search;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(super) enum MemoryTier {
    #[default]
    Durable,
    Cached,
    Cold,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(super) enum PhysicalMemory {
    Cached,
    Cold,
    Pinned,
}

/// Payloads are read only for returned hits; every profile keeps them cold on disk.
const PAYLOAD_MEMORY: PhysicalMemory = PhysicalMemory::Cold;

impl MemoryTier {
    fn dense(self) -> PhysicalMemory {
        if self == Self::Cold {
            PhysicalMemory::Cold
        } else {
            PhysicalMemory::Cached
        }
    }
    fn index(self) -> PhysicalMemory {
        if self == Self::Durable {
            PhysicalMemory::Pinned
        } else {
            self.dense()
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct MemoryPolicy {
    pub(super) tier: MemoryTier,
    pub(super) activate: bool,
}
impl From<MemoryTier> for MemoryPolicy {
    fn from(tier: MemoryTier) -> Self {
        Self {
            tier,
            activate: false,
        }
    }
}

pub(super) use crate::gold::vector::Search as VectorIndex;

#[derive(Clone)]
pub(super) struct Store {
    dense_names: std::collections::BTreeMap<String, String>,
    build: crate::gold::config::Build,
    api: api::Client,
    vector_index: VectorIndex,
}

/// One searchable message, denormalized with its thread metadata.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Document {
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

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum PointKind {
    Configuration,
    Gold,
}

fn collection(group: &str, metadata: bool) -> String {
    format!(
        "mash_gold_{}_{}",
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
fn document_id(node: &str, message: &str) -> String {
    point_id(&["message", node, message])
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
            dense_names: std::collections::BTreeMap::new(),
            build: crate::gold::config::Build::default(),
            api: api::Client::new("http://127.0.0.1:9", None, 64 * 1024 * 1024).unwrap(),
            vector_index: VectorIndex::default(),
        }
    }
    pub(in crate::shared) fn with_build(mut self, build: crate::gold::config::Build) -> Self {
        self.build = build;
        self
    }
    pub(in crate::shared) fn build(&self) -> &crate::gold::config::Build {
        &self.build
    }
    /// Explicit setup of each group's document and gold-state collections.
    pub(super) async fn connect(
        url: &str,
        key: Option<&str>,
        groups: &[String],
        dimensions: usize,
        max_response_bytes: usize,
        vector_index: VectorIndex,
        memory: MemoryPolicy,
    ) -> Result<Self, AppError> {
        vector_index.validate()?;
        let api = api::Client::new(url, key, max_response_bytes)?;
        api.verify_version().await?;
        let mut dense_names = std::collections::BTreeMap::new();
        for group in groups {
            api.initialize(&collection(group, true), None, memory)
                .await?;
            let name = api
                .initialize(&collection(group, false), Some(dimensions), memory)
                .await?
                .ok_or_else(failure)?;
            dense_names.insert(group.clone(), name);
        }
        Ok(Self {
            dense_names,
            build: crate::gold::config::Build::default(),
            api,
            vector_index,
        })
    }
    fn dense_name(&self, identity: &Identity) -> &str {
        self.dense_names
            .get(&identity.group_id)
            .map_or("dense", String::as_str)
    }
}
