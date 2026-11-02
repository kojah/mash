//! Typed node configuration shared by CLI, MCP and application operations.
use crate::provider::{Provider, ProviderRoots};
use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

mod document;
pub(crate) mod init;
mod loading;
#[cfg(test)]
mod tests;

pub(crate) use document::validate_identifier;
pub(crate) use loading::{default_path, load};

const DEFAULT_SINCE_DAYS: u32 = 90;
const MAX_SINCE_DAYS: u32 = 36_500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedConfig {
    pub(crate) attachments: crate::attachments::Configuration,
    pub(crate) redaction: crate::redaction::Settings,
    pub(crate) path: PathBuf,
    pub(crate) loaded: bool,
    pub(crate) shared: Option<SharedConfig>,
    pub(crate) embeddings: Option<crate::semantic::remote::Configuration>,
    pub(crate) views: crate::views::config::Configuration,
    pub(crate) providers: ProviderRoots,
    pub(crate) since_days: Option<u32>,
    pub(crate) destination: IndexDestination,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum IndexDestination {
    Local,
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
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

#[derive(Debug, Serialize)]
pub(crate) struct ConfigurationStatus {
    attachments: crate::attachments::Configuration,
    redaction: crate::redaction::Settings,
    path: PathBuf,
    loaded: bool,
    views: crate::views::config::Configuration,
    shared: Option<SharedConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    embeddings: Option<Box<crate::semantic::remote::Configuration>>,
    providers: BTreeMap<Provider, ProviderStatus>,
    index: IndexStatus,
}

#[derive(Debug, Serialize)]
struct ProviderStatus {
    roots: Vec<PathBuf>,
}

#[derive(Debug, Serialize)]
struct IndexStatus {
    since_days: Option<u32>,
    destination: IndexDestination,
}

impl ResolvedConfig {
    pub(crate) fn status(&self) -> ConfigurationStatus {
        ConfigurationStatus {
            attachments: self.attachments.clone(),
            redaction: self.redaction,
            path: self.path.clone(),
            loaded: self.loaded,
            views: self.views.clone(),
            shared: self.shared.clone(),
            embeddings: self.embeddings.clone().map(Box::new),
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
