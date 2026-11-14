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
mod vocabulary;

use crate::AppError;
use crate::views::scope::Scope;
pub(crate) use api::endpoint;
pub(crate) use configuration::{SCHEMA_VERSION, Settings};
use serde::{Deserialize, Serialize};
pub(crate) use vocabulary::file_beside as vocabulary_file;

pub(crate) use evidence::{ProjectionEvidence, Projections};
pub(crate) use read::validate_search;
pub(crate) use views::{Documents, Expected, NodeViews};

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq, schemars::JsonSchema,
)]
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
    vocabulary: vocabulary::Vocabulary,
}

/// The same searchable evidence shape for every source. Filter fields are
/// denormalized from the source and checked when decoding a result.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Document {
    node_id: String,
    provider: crate::provider::Provider,
    role: crate::role::Role,
    chat_id: String,
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
    /// Message position in its chat.
    pub(crate) sequence: i64,
    /// Chat facts that documents once copied from their chat. Search reads
    /// them from silver now and nothing writes them; a document written
    /// before that still carries them until its chat next changes.
    #[serde(default, skip_serializing)]
    pub(crate) title: Option<String>,
    #[serde(default, skip_serializing)]
    pub(crate) enrichment: Option<crate::silver::enrichment::Evidence>,
    #[serde(default, skip_serializing)]
    pub(crate) chat_created_at: Option<i64>,
    #[serde(default, skip_serializing)]
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
        || d.chat_id.is_empty()
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
        chat_id: d.chat_id,
        provider: d.provider,
        turn_id: None,
        role: d.role,
        sequence: Some(d.unit.sequence),
        content: d.unit.content,
        copies: vec![],
        created_at: d.created_at,
        // The chat's title, times and enrichment come from silver; see
        // `storage::chat_facts`.
        metadata: crate::search_results::ChatMetadata {
            project: d.project,
            repository: d.repository,
            ..Default::default()
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

/// Whether the configured Qdrant process answers. This is liveness only:
/// collection schemas and search readiness are reported separately.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Reachability {
    /// A supported Qdrant answered at the configured HTTP URL.
    Reachable,
    /// Nothing answered: the service is stopped, missing or on another port.
    Unreachable,
    /// Something answered but refused the request, or is not a supported
    /// Qdrant (an API key mismatch or an old version, for example).
    Rejected,
}

/// The configured local Qdrant service, as one bounded request observed it.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct ServiceStatus {
    pub(crate) url: String,
    pub state: Reachability,
    /// Why it is not reachable, and how to start it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// The Qdrant `mash qdrant install` set up on this machine, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) installed: Option<crate::core::qdrant::Managed>,
}

/// Ask the configured Qdrant for its version, waiting at most a few seconds.
/// Never creates or inspects collections.
pub async fn reachability(config: &crate::config::QdrantConfig) -> ServiceStatus {
    let (state, message) = match check(config).await {
        Ok(()) => (Reachability::Reachable, None),
        Err(error) if error.error.kind == crate::ErrorKind::QdrantUnavailable => {
            (Reachability::Unreachable, Some(error.error.message))
        }
        Err(error) => (Reachability::Rejected, Some(error.error.message)),
    };
    ServiceStatus {
        url: config.url.clone(),
        state,
        message,
        installed: None,
    }
}

/// The `qdrant-unavailable` message for the configured `url`, with an
/// optional `note` and the caller's own next step `then` instead of a bare
/// retry. `None` when `url` is not a valid endpoint, which never reaches
/// Qdrant and so never yields that error.
pub(crate) fn unavailable_message(url: &str, note: Option<&str>, then: &str) -> Option<String> {
    api::endpoint(url)
        .ok()
        .map(|url| api::unavailable_message(&url, note, then))
}

/// The typed error behind [`reachability`]; unreachable is `qdrant-unavailable`.
pub(crate) async fn check(config: &crate::config::QdrantConfig) -> Result<(), AppError> {
    let key = config
        .api_key_env
        .as_ref()
        .map(|name| {
            std::env::var(name).map_err(|_| {
                AppError::configuration(format!("set {name} to the local Qdrant API key"))
            })
        })
        .transpose()?;
    let api = api::Client::new(&config.url, key.as_deref(), 1024 * 1024)?;
    tokio::time::timeout(std::time::Duration::from_secs(5), api.verify_version())
        .await
        .unwrap_or_else(|_| Err(api.unavailable()))
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
    AppError::new(
        11,
        crate::ErrorKind::QdrantFailure,
        "Qdrant operation failed; inspect Qdrant's terminal output for storage, credentials, or schema errors, or use `docker logs CONTAINER` for a Docker installation.",
        true,
    )
}
fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase)
        .collect()
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
            vocabulary: vocabulary::Vocabulary::default(),
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
            vocabulary: vocabulary::Vocabulary::default(),
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
