//! Common Qdrant views: search documents and readiness derived from canonical
//! silver in SQLite. Every point here can be rebuilt by replaying silver.
mod api;
mod configuration;
#[cfg(test)]
mod document_tests;
mod evidence;
mod exact;
mod read;
#[cfg(test)]
mod tests;
mod views;

use crate::AppError;
use crate::views::scope::Scope;
pub(crate) use api::endpoint;
pub(crate) use configuration::{SCHEMA_VERSION, Settings};
use serde::{Deserialize, Serialize};

pub(crate) use evidence::{ProjectionEvidence, Projections};
pub(crate) use read::validate_search;
pub(crate) use views::{Documents, NodeViews};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum MemoryTier {
    #[default]
    Durable,
    Cached,
    Cold,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PhysicalMemory {
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
pub(crate) struct MemoryPolicy {
    pub(crate) tier: MemoryTier,
    pub(crate) activate: bool,
}
impl From<MemoryTier> for MemoryPolicy {
    fn from(tier: MemoryTier) -> Self {
        Self {
            tier,
            activate: false,
        }
    }
}

pub(crate) use crate::views::vector::Search as VectorIndex;

#[derive(Clone)]
pub(crate) struct Store {
    dense_names: std::collections::BTreeMap<String, String>,
    build: crate::views::config::Build,
    api: api::Client,
    vector_index: VectorIndex,
    /// Match evidence follows the server's response masking.
    responses: crate::redaction::Policy,
}

/// The same searchable evidence shape for every source. Filter fields are
/// denormalized from the source and checked when decoding a result.
#[derive(Clone, Deserialize, Serialize)]
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
    unit: SearchUnit,
    /// Omitted by literal scans, which do not inspect dense evidence.
    #[serde(default)]
    vector: Option<Vec<f64>>,
    embedding_fingerprint: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct SearchUnit {
    pub(crate) anchor: String,
    /// Indexed and embedded text; synchronization also fingerprints the evidence.
    pub(crate) text: String,
    pub(crate) content: String,
    pub(crate) title: Option<String>,
    pub(crate) enrichment: Option<crate::silver::enrichment::Evidence>,
    /// Message position in its chat.
    pub(crate) sequence: i64,
    pub(crate) chat_created_at: Option<i64>,
    pub(crate) chat_updated_at: Option<i64>,
    /// Set when the point carries a dense vector of this generation.
    pub(crate) generation: Option<String>,
}

/// Verify source identity, grouping and projection evidence for every query mode.
fn document_hit(
    id: &str,
    d: Document,
    active: &std::collections::BTreeSet<String>,
    dense: Option<(&Scope, &crate::views::config::Build, bool)>,
) -> Result<crate::storage::SearchHit, AppError> {
    if d.unit.content != d.unit.text {
        return Err(failure());
    }
    let expected_id = document_id(&d.node_id, &d.unit.anchor);
    if id != expected_id
        || !active.contains(&d.node_id)
        || d.conversation_id.is_empty()
        || d.unit.anchor.is_empty()
        || d.tokens != tokens(&d.unit.text)
        || d.unit.text.is_empty()
    {
        return Err(failure());
    }
    if let Some((scope, build, lexical)) = dense {
        let included = build.includes_dense(&d.role, &d.unit.text);
        if if included {
            d.unit.generation.as_deref() != Some(scope.generation.as_str())
                || d.vector
                    .as_ref()
                    .is_none_or(|v| !crate::views::retrieval::valid_vector(v, scope.dimensions))
        } else {
            !lexical || d.vector.is_some() || d.unit.generation.is_some()
        } {
            return Err(failure());
        }
    }
    Ok(crate::storage::SearchHit {
        node_id: Some(d.node_id),
        id: d.unit.anchor,
        conversation_id: d.conversation_id,
        provider: d.provider,
        turn_id: None,
        role: d.role,
        sequence: Some(d.unit.sequence),
        content: d.unit.content,
        copies: vec![],
        created_at: d.created_at,
        metadata: crate::search_results::ChatMetadata {
            enrichment: d.unit.enrichment,
            title: d.unit.title,
            project: d.project,
            repository: d.repository,
            created_at: d.unit.chat_created_at,
            updated_at: d.unit.chat_updated_at,
        },
        lexical_score: None,
        lookup: None,
        lexical_match: None,
        semantic_score: None,
        fusion_score: 0.0,
        rerank_score: None,
    })
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum PointKind {
    Configuration,
    Views,
}

fn collection(group: &str, metadata: bool) -> String {
    format!(
        "mash_views_{}_{}",
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
    pub(crate) fn unavailable() -> Self {
        Self {
            dense_names: std::collections::BTreeMap::new(),
            build: crate::views::config::Build::default(),
            api: api::Client::new("http://127.0.0.1:9", None, 64 * 1024 * 1024).unwrap(),
            vector_index: VectorIndex::default(),
            responses: crate::redaction::Policy::ON,
        }
    }
    pub(crate) fn with_response_redaction(mut self, responses: crate::redaction::Policy) -> Self {
        self.responses = responses;
        self
    }
    pub(crate) fn with_build(mut self, build: crate::views::config::Build) -> Self {
        self.build = build;
        self
    }
    pub(crate) fn build(&self) -> &crate::views::config::Build {
        &self.build
    }
    /// Explicit setup of each group's document and views-state collections.
    pub(crate) async fn connect(
        (url, grpc_url): (&str, &str),
        key: Option<&str>,
        groups: &[String],
        dimensions: usize,
        max_response_bytes: usize,
        vector_index: VectorIndex,
        memory: MemoryPolicy,
    ) -> Result<Self, AppError> {
        vector_index.validate()?;
        let api = api::Client::new(url, key, max_response_bytes)?.with_mutations(grpc_url, key)?;
        api.verify_version().await?;
        for group in groups {
            api.initialize(&collection(group, true), None, memory)
                .await?;
            api.initialize(&collection(group, false), Some(dimensions), memory)
                .await?;
        }
        let mut store = Self::open(
            url,
            key,
            groups,
            dimensions,
            max_response_bytes,
            vector_index,
            memory.tier,
        )
        .await?;
        store.api = api;
        Ok(store)
    }

    /// Open existing views for readers. Missing or incomplete setup fails closed;
    /// only the explicit setup path may create indexes or change vector fields.
    pub(crate) async fn open(
        url: &str,
        key: Option<&str>,
        groups: &[String],
        dimensions: usize,
        max_response_bytes: usize,
        vector_index: VectorIndex,
        memory: MemoryTier,
    ) -> Result<Self, AppError> {
        vector_index.validate()?;
        let api = api::Client::new(url, key, max_response_bytes)?;
        api.verify_version().await?;
        let mut dense_names = std::collections::BTreeMap::new();
        for group in groups {
            api.inspect(&collection(group, true), None, memory).await?;
            let name = api
                .inspect(&collection(group, false), Some(dimensions), memory)
                .await?
                .ok_or_else(failure)?;
            dense_names.insert(group.clone(), name);
        }
        Ok(Self {
            dense_names,
            build: crate::views::config::Build::default(),
            api,
            vector_index,
            responses: crate::redaction::Policy::ON,
        })
    }
    fn dense_name(&self, identity: &Scope) -> &str {
        self.dense_names
            .get(&identity.group_id)
            .map_or("dense", String::as_str)
    }
}

fn conflict(message: &str) -> AppError {
    let mut error = failure();
    error.error.kind = crate::ErrorKind::Conflict;
    message.clone_into(&mut error.error.message);
    error
}
