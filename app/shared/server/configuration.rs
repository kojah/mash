//! Strict server settings, bounded loading and configured node identities.
use super::Identity;
use crate::AppError;
use crate::shared::{qdrant_index, redis_index};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::Path;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorizedNode {
    group_id: String,
    node_id: String,
    token_env: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Configuration {
    version: u32,
    #[serde(default)]
    pub(super) embeddings: EmbeddingSpace,
    #[serde(default = "default_listen")]
    pub(super) listen: SocketAddr,
    #[serde(default)]
    pub(super) backend: BackendChoice,
    pub(super) database_url_env: Option<String>,
    pub(super) redis_url_env: Option<String>,
    pub(super) qdrant_url_env: Option<String>,
    pub(super) qdrant_api_key_env: Option<String>,
    #[serde(default = "default_redis_node_mib")]
    pub(super) qdrant_max_node_mib: usize,
    #[serde(default)]
    pub(super) qdrant_vector_index: qdrant_index::VectorIndex,
    #[serde(default = "default_connections")]
    pub(super) max_connections: usize,
    #[serde(default = "default_redis_node_mib")]
    pub(super) redis_max_node_mib: usize,
    #[serde(default)]
    pub(super) redis_vector_index: redis_index::VectorIndex,
    #[serde(default = "default_uploads")]
    pub(super) max_concurrent_uploads: usize,
    nodes: Vec<AuthorizedNode>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EmbeddingSpace {
    model: String,
    pub(super) dimensions: usize,
}
impl Default for EmbeddingSpace {
    fn default() -> Self {
        Self {
            model: "sentence-transformers/all-MiniLM-L6-v2".into(),
            dimensions: 384,
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum BackendChoice {
    Postgres,
    #[default]
    Redis,
    Qdrant,
}

fn default_redis_node_mib() -> usize {
    64
}
fn default_uploads() -> usize {
    4
}
fn default_listen() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 8787))
}
const fn default_connections() -> usize {
    16
}

pub(super) fn env_name(name: &str) -> Result<(), AppError> {
    if name.is_empty()
        || !name
            .bytes()
            .enumerate()
            .all(|(i, b)| b.is_ascii_alphabetic() || b == b'_' || (i > 0 && b.is_ascii_digit()))
    {
        return Err(AppError::configuration(
            "invalid credential environment variable name",
        ));
    }
    Ok(())
}
pub(super) fn identities(
    config: &Configuration,
) -> Result<HashMap<blake3::Hash, Identity>, AppError> {
    config.redis_vector_index.validate()?;
    config.qdrant_vector_index.validate()?;
    if !matches!(config.backend, BackendChoice::Redis)
        && config.redis_vector_index != (redis_index::VectorIndex::Exact {})
    {
        return Err(AppError::configuration(
            "redis_vector_index HNSW settings require backend=redis",
        ));
    }
    if !matches!(config.backend, BackendChoice::Qdrant)
        && (config.qdrant_url_env.is_some()
            || config.qdrant_api_key_env.is_some()
            || config.qdrant_vector_index != qdrant_index::VectorIndex::default())
    {
        return Err(AppError::configuration(
            "Qdrant settings require backend=qdrant",
        ));
    }
    if matches!(config.backend, BackendChoice::Qdrant)
        && (config.database_url_env.is_some() || config.redis_url_env.is_some())
    {
        return Err(AppError::configuration(
            "Qdrant backend does not accept database_url_env or redis_url_env",
        ));
    }
    if !(1..=4096).contains(&config.qdrant_max_node_mib) {
        return Err(AppError::configuration(
            "qdrant_max_node_mib must be between 1 and 4096",
        ));
    }
    if !(1..=16).contains(&config.max_concurrent_uploads) {
        return Err(AppError::configuration(
            "max_concurrent_uploads must be between 1 and 16",
        ));
    }
    if !(1..=4096).contains(&config.redis_max_node_mib) {
        return Err(AppError::configuration(
            "redis_max_node_mib must be between 1 and 4096",
        ));
    }
    if config.version != 1
        || config.nodes.is_empty()
        || !(1..=128).contains(&config.max_connections)
    {
        return Err(AppError::configuration(
            "server requires version 1, configured nodes, and 1..128 connections",
        ));
    }
    if !(1..=2048).contains(&config.embeddings.dimensions)
        || config.embeddings.model.trim().is_empty()
        || config.embeddings.model.len() > 255
        || config.embeddings.model.chars().any(char::is_control)
    {
        return Err(AppError::configuration(
            "invalid server embedding model or dimensions",
        ));
    }
    let mut result = HashMap::new();
    let mut unique = HashSet::new();
    for node in &config.nodes {
        crate::config::validate_identifier(&node.group_id, "group_id")?;
        crate::config::validate_identifier(&node.node_id, "node_id")?;
        env_name(&node.token_env)?;
        let token = std::env::var(&node.token_env).map_err(|_| {
            AppError::configuration(format!("set {} to the node's API token", node.token_env))
        })?;
        if token.len() < 32 || token.len() > 4096 || !token.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(AppError::configuration(
                "API tokens require 32..4096 printable ASCII bytes",
            ));
        }
        if !unique.insert((&node.group_id, &node.node_id))
            || result
                .insert(
                    blake3::hash(token.as_bytes()),
                    Identity {
                        group_id: node.group_id.clone(),
                        node_id: node.node_id.clone(),
                        generation: crate::semantic::model_space(
                            &config.embeddings.model,
                            config.embeddings.dimensions,
                        ),
                        dimensions: config.embeddings.dimensions,
                    },
                )
                .is_some()
        {
            return Err(AppError::configuration(
                "each node identity and API token must be unique",
            ));
        }
    }
    Ok(result)
}
pub(super) async fn load(path: &Path) -> Result<Configuration, AppError> {
    let path = path.to_path_buf();
    crate::blocking(move || {
        use std::io::Read;
        if !std::fs::metadata(&path)
            .map_err(|_| AppError::configuration("could not inspect server configuration"))?
            .is_file()
        {
            return Err(AppError::configuration(
                "server configuration must be a regular file",
            ));
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|_| AppError::configuration("could not open server configuration"))?
            .take(65537)
            .read_to_end(&mut bytes)
            .map_err(|_| AppError::configuration("could not read server configuration"))?;
        if bytes.len() > 65536 {
            return Err(AppError::configuration(
                "server configuration exceeds 65536 bytes",
            ));
        }
        let contents = std::str::from_utf8(&bytes)
            .map_err(|_| AppError::configuration("server configuration must be UTF-8 TOML"))?;
        toml::from_str(contents)
            .map_err(|_| AppError::configuration("invalid server configuration"))
    })
    .await
}
