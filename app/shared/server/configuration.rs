//! Strict server settings, bounded loading and configured node identities.
use super::Identity;
use crate::AppError;
use crate::views::qdrant as qdrant_index;
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
    /// Shared silver system of record, one database per configured node;
    /// defaults to `shared-silver` in the platform data directory.
    silver_dir: Option<std::path::PathBuf>,
    version: u32,
    #[serde(default)]
    pub(super) views: View,
    #[serde(default)]
    pub(super) embeddings: crate::config::Embeddings,
    #[serde(default = "default_listen")]
    pub(super) listen: SocketAddr,
    pub(super) qdrant_url_env: Option<String>,
    pub(super) qdrant_grpc_url_env: Option<String>,
    pub(super) qdrant_api_key_env: Option<String>,
    #[serde(default = "default_upload_mib")]
    max_upload_mib: u64,
    #[serde(default)]
    pub(super) qdrant_memory: qdrant_index::MemoryTier,
    #[serde(default = "default_uploads")]
    pub(super) max_concurrent_uploads: usize,
    #[serde(default)]
    pub(super) redaction: Redaction,
    nodes: Vec<AuthorizedNode>,
}
/// Where the server masks recognizable credentials; each defaults on.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Redaction {
    /// HTTP read responses: search candidates, chats, view and attachments.
    pub(super) responses: bool,
}
impl Default for Redaction {
    fn default() -> Self {
        Self { responses: true }
    }
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct View {
    pub(super) build: crate::views::config::Build,
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
fn default_upload_mib() -> u64 {
    64
}
fn default_uploads() -> usize {
    4
}
fn default_listen() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 8787))
}
impl Configuration {
    pub(super) fn silver_dir(&self) -> Result<std::path::PathBuf, AppError> {
        match &self.silver_dir {
            Some(path) if path.is_absolute() => Ok(path.clone()),
            Some(_) => Err(AppError::configuration(
                "silver_dir must be an absolute path",
            )),
            None => Ok(crate::platform::mash_home()
                .ok_or_else(|| AppError::configuration("platform data directory is unavailable"))?
                .join("shared-silver")),
        }
    }

    pub(super) fn max_upload_bytes(&self) -> Result<u64, AppError> {
        self.max_upload_mib
            .checked_mul(1024 * 1024)
            .ok_or_else(|| AppError::configuration("max_upload_mib exceeds platform capacity"))
    }
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
    config.views.query.vector_search.validate()?;
    config.embeddings.validate()?;
    config.silver_dir()?;
    for (group, policy) in &config.views.query.groups {
        crate::config::validate_identifier(group, "group_id")?;
        policy.validate()?;
        if !config.nodes.iter().any(|node| &node.group_id == group) {
            return Err(AppError::configuration(
                "views query defaults name an unconfigured group",
            ));
        }
    }
    if config
        .attachments_dir
        .as_ref()
        .is_some_and(|path| !path.is_absolute())
    {
        return Err(AppError::configuration(
            "attachments_dir must be an absolute path",
        ));
    }
    if !(1..=4096).contains(&config.max_upload_mib) {
        return Err(AppError::configuration(
            "max_upload_mib must be between 1 and 4096",
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
                        generation: crate::semantic::SHARED_EMBEDDING_GENERATION.to_owned(),
                        dimensions: 384,
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
    #[test]
    fn durable_memory_is_default_and_profiles_are_strict() {
        let default: super::Configuration = toml::from_str("version=1\nnodes=[]").unwrap();
        assert_eq!(
            default.qdrant_memory,
            crate::views::qdrant::MemoryTier::Durable
        );
        for profile in ["durable", "cached", "cold"] {
            let text = format!("version=1\nnodes=[]\nqdrant_memory='{profile}'");
            assert!(toml::from_str::<super::Configuration>(&text).is_ok());
        }
        for profile in ["ram", "pinned", "disk"] {
            let text = format!("version=1\nnodes=[]\nqdrant_memory='{profile}'");
            assert!(toml::from_str::<super::Configuration>(&text).is_err());
        }
    }
    use super::*;

    #[test]
    fn removed_remote_embeddings_settings_are_rejected() {
        for setting in [
            "[embeddings]\nmodel='test'\ndimensions=384",
            "[redaction]\nremote_embeddings=false",
        ] {
            let error = toml::from_str::<Configuration>(&format!("version=1\nnodes=[]\n{setting}"))
                .err()
                .expect("removed setting");
            assert!(
                error.message().contains("unknown field") || error.message().contains("backend"),
                "{error}"
            );
        }
    }

    #[test]
    fn remote_embeddings_are_configurable() {
        let config: Configuration = toml::from_str(
            "version=1\nnodes=[]\n[embeddings]\nbackend='remote'\nurl='https://example.com/embed'\ntoken_env='MASH_TEST_TOKEN'\ntimeout_seconds=120",
        )
        .unwrap();
        assert!(matches!(
            config.embeddings,
            crate::config::Embeddings::Remote { .. }
        ));
        let default: Configuration = toml::from_str("version=1\nnodes=[]").unwrap();
        assert_eq!(default.embeddings, crate::config::Embeddings::Local {});
    }

    #[test]
    fn redaction_switches_default_on_and_reject_unknown_keys() {
        let default: Configuration = toml::from_str("version=1\nnodes=[]").unwrap();
        assert!(default.redaction.responses);
        let off: Configuration =
            toml::from_str("version=1\nnodes=[]\n[redaction]\nresponses=false").unwrap();
        assert!(!off.redaction.responses);
        assert!(
            toml::from_str::<Configuration>("version=1\nnodes=[]\n[redaction]\noutput=false")
                .is_err()
        );
    }

    #[test]
    fn vector_query_policy_is_independent_of_views_build_identity() {
        let default: View = toml::from_str("").unwrap();
        let hnsw: View =
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
        let exact: View = toml::from_str("[query.vector_search]\nalgorithm = 'exact'").unwrap();
        assert_eq!(
            exact.query.vector_search,
            qdrant_index::VectorIndex::Exact {}
        );
        assert_eq!(default.build, exact.build);
        hnsw.query.vector_search.validate().unwrap();
        let omitted_ef: View = toml::from_str("[query.vector_search]\nalgorithm = 'hnsw'").unwrap();
        assert_eq!(
            omitted_ef.query.vector_search,
            qdrant_index::VectorIndex::Hnsw { ef_runtime: 200 }
        );
    }

    #[test]
    fn group_query_defaults_are_typed_and_reject_unconfigured_groups_before_credentials() {
        let text = "version=1\nnodes=[{group_id='team',node_id='node',token_env='MASH_QUERY_TEST_TOKEN'}]\n[views.query.groups.team]\nalgorithm='hnsw'\nef_runtime=80\n";
        let config: Configuration = toml::from_str(text).unwrap();
        assert_eq!(
            config.views.query.groups["team"],
            qdrant_index::VectorIndex::Hnsw { ef_runtime: 80 }
        );
        assert_eq!(
            config.views.query.vector_search,
            qdrant_index::VectorIndex::default()
        );
        assert_eq!(config.views.build, crate::views::config::Build::default());
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
            assert!(toml::from_str::<View>(text).is_err(), "accepted {text}");
        }
        for ef in [0, 4097] {
            let views: View = toml::from_str(&format!(
                "[query.vector_search]\nalgorithm = 'hnsw'\nef_runtime = {ef}"
            ))
            .unwrap();
            assert!(views.query.vector_search.validate().is_err());
        }
    }
}
