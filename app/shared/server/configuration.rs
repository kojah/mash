//! Strict server settings, bounded loading and configured node identities.
use super::Identity;
use crate::AppError;
use crate::shared::qdrant_index;
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
    pub(super) attachments_dir: Option<std::path::PathBuf>,
    version: u32,
    #[serde(default)]
    pub(super) gold: Gold,
    #[serde(default)]
    pub(super) embeddings: EmbeddingSpace,
    #[serde(default = "default_listen")]
    pub(super) listen: SocketAddr,
    pub(super) qdrant_url_env: Option<String>,
    pub(super) qdrant_api_key_env: Option<String>,
    #[serde(default = "default_node_mib")]
    pub(super) qdrant_max_node_mib: usize,
    #[serde(default)]
    pub(super) qdrant_memory: qdrant_index::MemoryTier,
    #[serde(default = "default_uploads")]
    pub(super) max_concurrent_uploads: usize,
    #[serde(default = "default_snapshot_budget")]
    pub(super) max_mutation_snapshot_mib: usize,
    nodes: Vec<AuthorizedNode>,
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Gold {
    pub(super) build: crate::gold::config::Build,
    pub(super) query: Query,
}
/// Retrieval policy for shared candidates, independent of collection construction.
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Query {
    pub(super) vector_search: qdrant_index::VectorIndex,
    /// Complete query defaults for named groups; other groups inherit `vector_search`.
    pub(super) groups: HashMap<String, qdrant_index::VectorIndex>,
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

fn default_node_mib() -> usize {
    64
}
fn default_uploads() -> usize {
    4
}
fn default_snapshot_budget() -> usize {
    crate::shared::snapshot_budget::DEFAULT_MIB
}
fn default_listen() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 8787))
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
    config.gold.query.vector_search.validate()?;
    for (group, policy) in &config.gold.query.groups {
        crate::config::validate_identifier(group, "group_id")?;
        policy.validate()?;
        if !config.nodes.iter().any(|node| &node.group_id == group) {
            return Err(AppError::configuration(
                "gold query defaults name an unconfigured group",
            ));
        }
    }
    crate::shared::snapshot_budget::Budget::validate(config.max_mutation_snapshot_mib)?;
    if config
        .attachments_dir
        .as_ref()
        .is_some_and(|path| !path.is_absolute())
    {
        return Err(AppError::configuration(
            "attachments_dir must be an absolute path",
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
    if config.version != 1 || config.nodes.is_empty() {
        return Err(AppError::configuration(
            "server requires version 1 and configured nodes",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_query_policy_is_independent_of_gold_build_identity() {
        let default: Gold = toml::from_str("").unwrap();
        let hnsw: Gold =
            toml::from_str("[query.vector_search]\nalgorithm = 'hnsw'\nef_runtime = 80").unwrap();
        assert_eq!(
            default.query.vector_search,
            qdrant_index::VectorIndex::Hnsw { ef_runtime: 200 }
        );
        assert_eq!(
            hnsw.query.vector_search,
            qdrant_index::VectorIndex::Hnsw { ef_runtime: 80 }
        );
        assert_eq!(default.build, hnsw.build);
        let exact: Gold = toml::from_str("[query.vector_search]\nalgorithm = 'exact'").unwrap();
        assert_eq!(
            exact.query.vector_search,
            qdrant_index::VectorIndex::Exact {}
        );
        assert_eq!(default.build, exact.build);
        assert_eq!(
            default.build.generation("model"),
            exact.build.generation("model")
        );
        assert_eq!(
            default.build.generation("model"),
            hnsw.build.generation("model")
        );
        hnsw.query.vector_search.validate().unwrap();
        let omitted_ef: Gold = toml::from_str("[query.vector_search]\nalgorithm = 'hnsw'").unwrap();
        assert_eq!(
            omitted_ef.query.vector_search,
            qdrant_index::VectorIndex::Hnsw { ef_runtime: 200 }
        );
    }

    #[test]
    fn group_query_defaults_are_typed_and_reject_unconfigured_groups_before_credentials() {
        let text = "version=1\nnodes=[{group_id='team',node_id='node',token_env='MASH_QUERY_TEST_TOKEN'}]\n[gold.query.groups.team]\nalgorithm='hnsw'\nef_runtime=80\n";
        let config: Configuration = toml::from_str(text).unwrap();
        assert_eq!(
            config.gold.query.groups["team"],
            qdrant_index::VectorIndex::Hnsw { ef_runtime: 80 }
        );
        assert_eq!(
            config.gold.query.vector_search,
            qdrant_index::VectorIndex::default()
        );
        assert_eq!(config.gold.build, crate::gold::config::Build::default());
        let unknown: Configuration =
            toml::from_str(&text.replace("groups.team", "groups.unknown")).unwrap();
        assert!(
            identities(&unknown)
                .err()
                .unwrap()
                .error
                .message
                .contains("unconfigured group")
        );
        let invalid: Configuration =
            toml::from_str(&text.replace("ef_runtime=80", "ef_runtime=0")).unwrap();
        assert!(
            identities(&invalid)
                .err()
                .unwrap()
                .error
                .message
                .contains("ef_runtime")
        );
    }

    #[test]
    fn vector_query_policy_rejects_invalid_or_misplaced_fields() {
        for text in [
            "[query.vector_search]\nalgorithm = 'unknown'",
            "[query.vector_search]\nalgorithm = 'exact'\nef_runtime = 80",
            "[query.vector_search]\nalgorithm = 'hnsw'\nm = 16",
            "[build.vector_search]\nalgorithm = 'hnsw'",
            "[query]\nlimit = 10",
        ] {
            assert!(toml::from_str::<Gold>(text).is_err(), "accepted {text}");
        }
        for ef in [0, 4097] {
            let gold: Gold = toml::from_str(&format!(
                "[query.vector_search]\nalgorithm = 'hnsw'\nef_runtime = {ef}"
            ))
            .unwrap();
            assert!(gold.query.vector_search.validate().is_err());
        }
    }
}
