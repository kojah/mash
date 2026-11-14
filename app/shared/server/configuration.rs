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
    /// Environment variable holding the node's token.
    #[serde(default)]
    token_env: Option<String>,
    /// BLAKE3 of the node's token, for a node added with `--add-node`: the
    /// server then keeps no copy of the token itself.
    #[serde(default)]
    token_hash: Option<String>,
}

/// Nodes added with `--add-node`, kept beside the configuration so the
/// operator's own file is never rewritten.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddedNodes {
    #[serde(default)]
    nodes: Vec<AuthorizedNode>,
}

/// The file `--add-node` appends to: the configuration's path plus `.nodes`.
pub(super) fn added_nodes_path(configuration: &Path) -> std::path::PathBuf {
    let mut name = configuration.as_os_str().to_owned();
    name.push(".nodes");
    name.into()
}

/// Whether `group_id/node_id` is already a node of this server.
pub fn has_node(config: &Configuration, group_id: &str, node_id: &str) -> bool {
    config
        .nodes
        .iter()
        .any(|node| node.group_id == group_id && node.node_id == node_id)
}

/// Record a new node by the hash of its token. The caller has validated the
/// identifiers, which are therefore safe to write as TOML strings.
pub fn append_node(
    configuration: &Path,
    group_id: &str,
    node_id: &str,
    token: &str,
) -> Result<(), AppError> {
    use std::io::Write;
    let path = added_nodes_path(configuration);
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .map_err(|_| AppError::configuration("could not write the added-nodes file"))?;
    write!(
        file,
        "\n[[nodes]]\ngroup_id = \"{group_id}\"\nnode_id = \"{node_id}\"\ntoken_hash = \"{}\"\n",
        blake3::hash(token.as_bytes()).to_hex()
    )
    .and_then(|()| file.sync_all())
    .map_err(|_| AppError::configuration("could not write the added-nodes file"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub attachments_dir: Option<std::path::PathBuf>,
    /// Shared silver system of record, one database per configured node;
    /// defaults to `shared-silver` in the platform data directory.
    silver_dir: Option<std::path::PathBuf>,
    version: u32,
    #[serde(default)]
    pub(super) views: View,
    #[serde(default)]
    pub(super) embeddings: crate::config::Embeddings,
    #[serde(default = "default_listen")]
    pub listen: SocketAddr,
    pub qdrant_url_env: Option<String>,
    pub qdrant_grpc_url_env: Option<String>,
    pub(super) qdrant_api_key_env: Option<String>,
    #[serde(default = "default_upload_mib")]
    max_upload_mib: u64,
    #[serde(default)]
    pub(super) qdrant_memory: qdrant_index::MemoryTier,
    #[serde(default = "default_uploads")]
    pub max_concurrent_uploads: usize,
    #[serde(default)]
    pub redaction: Redaction,
    nodes: Vec<AuthorizedNode>,
}
/// Where the server masks recognizable credentials; each defaults on.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Redaction {
    /// HTTP read responses: search candidates, chats, view and attachments.
    pub responses: bool,
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
    pub fn silver_dir(&self) -> Result<std::path::PathBuf, AppError> {
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
pub fn identities(config: &Configuration) -> Result<HashMap<blake3::Hash, Identity>, AppError> {
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
        let hash = match (&node.token_env, &node.token_hash) {
            (Some(name), None) => {
                env_name(name)?;
                let token = std::env::var(name).map_err(|_| {
                    AppError::configuration(format!("set {name} to the node's API token"))
                })?;
                if token.len() < 32
                    || token.len() > 4096
                    || !token.bytes().all(|b| b.is_ascii_graphic())
                {
                    return Err(AppError::configuration(
                        "API tokens require 32..4096 printable ASCII bytes",
                    ));
                }
                blake3::hash(token.as_bytes())
            }
            (None, Some(hex)) => blake3::Hash::from_hex(hex).map_err(|_| {
                AppError::configuration("token_hash must be 64 hexadecimal characters")
            })?,
            _ => {
                return Err(AppError::configuration(
                    "each node needs exactly one of token_env and token_hash",
                ));
            }
        };
        if !unique.insert((&node.group_id, &node.node_id))
            || result
                .insert(
                    hash,
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
pub async fn load(path: &Path) -> Result<Configuration, AppError> {
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
        std::fs::File::open(&path)
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
        let mut config: Configuration = toml::from_str(contents)
            .map_err(|_| AppError::configuration("invalid server configuration"))?;
        // Nodes added with --add-node; they carry token hashes only.
        match std::fs::read_to_string(added_nodes_path(&path)) {
            Ok(text) if text.len() <= 65536 => {
                let added: AddedNodes = toml::from_str(&text)
                    .map_err(|_| AppError::configuration("invalid added-nodes file"))?;
                if added.nodes.iter().any(|node| node.token_env.is_some()) {
                    return Err(AppError::configuration(
                        "the added-nodes file holds token hashes only",
                    ));
                }
                config.nodes.extend(added.nodes);
            }
            Ok(_) => {
                return Err(AppError::configuration(
                    "the added-nodes file exceeds 65536 bytes",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                return Err(AppError::configuration(
                    "could not read the added-nodes file",
                ));
            }
        }
        Ok(config)
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

    #[tokio::test]
    async fn added_nodes_are_known_by_the_hash_of_their_token_only() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("server.toml");
        std::fs::write(&path, "version=1\nnodes=[]\n").unwrap();
        // Nothing added yet: the configuration alone names no node.
        assert!(identities(&load(&path).await.unwrap()).is_err());
        let token = "0123456789abcdef0123456789abcdef";
        append_node(&path, "team", "laptop", token).unwrap();
        append_node(&path, "team", "desktop", "ffffffffffffffffffffffffffffffff").unwrap();
        let added = std::fs::read_to_string(added_nodes_path(&path)).unwrap();
        assert!(
            !added.contains(token),
            "the server keeps no copy of the token"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(added_nodes_path(&path))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let config = load(&path).await.unwrap();
        assert!(has_node(&config, "team", "laptop") && has_node(&config, "team", "desktop"));
        assert!(!has_node(&config, "team", "phone"));
        let known = identities(&config).unwrap();
        assert_eq!(known.len(), 2);
        assert_eq!(known[&blake3::hash(token.as_bytes())].node_id, "laptop");
        // The file holds hashes only, and a node names exactly one credential.
        std::fs::write(
            added_nodes_path(&path),
            "[[nodes]]\ngroup_id='team'\nnode_id='x'\ntoken_env='MASH_X'\n",
        )
        .unwrap();
        assert!(load(&path).await.is_err());
        for node in [
            "{group_id='team',node_id='x'}",
            "{group_id='team',node_id='x',token_env='MASH_X',token_hash='00'}",
            "{group_id='team',node_id='x',token_hash='not-hex'}",
        ] {
            let config: Configuration =
                toml::from_str(&format!("version=1\nnodes=[{node}]\n")).unwrap();
            assert!(identities(&config).is_err(), "{node}");
        }
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
