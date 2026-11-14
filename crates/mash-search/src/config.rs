//! Typed node configuration shared by CLI, MCP and application operations.
use crate::AppError;
use crate::provider::{Provider, ProviderRoots};
use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

mod agent;
mod assistant;
mod document;
mod embeddings;
pub mod init;
mod loading;
mod qdrant;
mod reranking;
pub mod settings;
#[cfg(test)]
mod tests;

pub use agent::Agent;
pub use assistant::Assistant;
pub use document::validate_identifier;
pub use embeddings::Embeddings;
pub use loading::{default_path, load};
pub use qdrant::QdrantConfig;
pub use reranking::Reranking;

const DEFAULT_SINCE_DAYS: u32 = 90;
const MAX_SINCE_DAYS: u32 = 36_500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedConfig {
    pub embeddings: Embeddings,
    pub reranking: Reranking,
    /// The model behind the dashboard's assistant, when one is configured.
    pub assistant: Option<Assistant>,
    pub qdrant: QdrantConfig,
    pub attachments: crate::attachments::Configuration,
    pub redaction: crate::redaction::Settings,
    pub records: crate::records::capture::Policy,
    pub path: PathBuf,
    pub loaded: bool,
    pub shared: Option<SharedConfig>,
    pub views: crate::views::config::Configuration,
    pub providers: ProviderRoots,
    pub since_days: Option<u32>,
    pub destination: IndexDestination,
    /// Agents that `mash init` registered Mash with (`[agents] connected`).
    /// `status --installation` expects them even if a registration is removed;
    /// empty when the configuration predates the field.
    pub connected_agents: Vec<crate::config::Agent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum IndexDestination {
    Local,
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SharedConfig {
    pub group_id: String,
    pub node_id: String,
    pub api_url: String,
    pub token_env: String,
    /// Whether uploads mask credentials; set from `[redaction]`, not this table.
    #[serde(skip)]
    pub publication_redaction: crate::redaction::Policy,
}

impl SharedConfig {
    /// This node's API token: the variable `token_env` names, or the copy a
    /// join saved for it under Mash's own directory.
    pub fn token(&self) -> Option<String> {
        secret(&self.token_env)
    }
}

/// A credential by the name of its environment variable: the variable itself,
/// or the copy saved under Mash's own directory.
fn secret(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|token| !token.is_empty())
        .or_else(|| saved_token(&saved_tokens()?, name))
}

/// Where `mash init --join` keeps a node token, readable only by the user, so
/// a joined machine needs no shell profile edit.
pub fn saved_tokens() -> Option<std::path::PathBuf> {
    crate::platform::mash_home().map(|home| home.join("node.env"))
}

fn saved_token(path: &std::path::Path, name: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()?
        .lines()
        .find_map(|line| {
            line.strip_prefix(name)?
                .strip_prefix('=')
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })
}

/// Save `name=token` in `path`, replacing an earlier value for that name and
/// keeping the others.
pub fn save_token(path: &std::path::Path, name: &str, token: &str) -> Result<(), AppError> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let prefix = format!("{name}=");
    let mut text = String::new();
    for line in existing.lines().filter(|line| !line.starts_with(&prefix)) {
        text.push_str(line);
        text.push('\n');
    }
    text.push_str(&prefix);
    text.push_str(token);
    text.push('\n');
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(AppError::io)?;
    }
    std::fs::write(path, text).map_err(AppError::io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(AppError::io)?;
    }
    Ok(())
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ConfigurationStatus {
    embeddings: Embeddings,
    reranking: Reranking,
    qdrant: QdrantConfig,
    attachments: crate::attachments::Configuration,
    redaction: crate::redaction::Settings,
    records: crate::records::capture::Policy,
    path: PathBuf,
    loaded: bool,
    views: crate::views::config::Configuration,
    shared: Option<SharedConfig>,
    providers: BTreeMap<Provider, ProviderStatus>,
    index: IndexStatus,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct ProviderStatus {
    roots: Vec<PathBuf>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct IndexStatus {
    since_days: Option<u32>,
    destination: IndexDestination,
}

impl ResolvedConfig {
    pub fn status(&self) -> ConfigurationStatus {
        ConfigurationStatus {
            embeddings: self.embeddings.clone(),
            reranking: self.reranking.clone(),
            qdrant: self.qdrant.clone(),
            attachments: self.attachments.clone(),
            redaction: self.redaction,
            records: self.records.clone(),
            path: self.path.clone(),
            loaded: self.loaded,
            views: self.views.clone(),
            shared: self.shared.clone(),
            providers: self
                .providers
                .iter()
                .map(|(&provider, roots)| {
                    (
                        provider,
                        ProviderStatus {
                            roots: roots.clone(),
                        },
                    )
                })
                .collect(),
            index: IndexStatus {
                since_days: self.since_days,
                destination: self.destination,
            },
        }
    }
}
