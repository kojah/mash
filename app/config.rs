//! Typed node configuration shared by CLI, MCP and application operations.
use crate::provider::{Provider, ProviderRoots};
use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

mod document;
mod embeddings;
pub(crate) mod init;
mod loading;
mod qdrant;
mod reranking;
pub(crate) mod settings;
#[cfg(test)]
mod tests;

pub(crate) use document::validate_identifier;
pub(crate) use embeddings::Embeddings;
pub(crate) use loading::{default_path, load};
pub(crate) use qdrant::QdrantConfig;
pub(crate) use reranking::Reranking;

const DEFAULT_SINCE_DAYS: u32 = 90;
const MAX_SINCE_DAYS: u32 = 36_500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedConfig {
    pub(crate) embeddings: Embeddings,
    pub(crate) reranking: Reranking,
    pub(crate) qdrant: QdrantConfig,
    pub(crate) attachments: crate::attachments::Configuration,
    pub(crate) redaction: crate::redaction::Settings,
    pub(crate) records: crate::records::capture::Policy,
    pub(crate) path: PathBuf,
    pub(crate) loaded: bool,
    pub(crate) shared: Option<SharedConfig>,
    pub(crate) views: crate::views::config::Configuration,
    pub(crate) providers: ProviderRoots,
    pub(crate) since_days: Option<u32>,
    pub(crate) destination: IndexDestination,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum IndexDestination {
    Local,
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SharedConfig {
    pub(crate) group_id: String,
    pub(crate) node_id: String,
    pub(crate) api_url: String,
    pub(crate) token_env: String,
    /// Whether uploads mask credentials; set from `[redaction]`, not this table.
    #[serde(skip)]
    pub(crate) publication_redaction: crate::redaction::Policy,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub(crate) struct ConfigurationStatus {
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
    pub(crate) fn status(&self) -> ConfigurationStatus {
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
