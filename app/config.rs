//! Typed node configuration shared by CLI, MCP and application operations.
use crate::provider::{Provider, ProviderRoots};
use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

mod document;
mod loading;
#[cfg(test)]
mod tests;

pub(crate) use document::validate_identifier;
pub(crate) use loading::load;

const DEFAULT_SINCE_DAYS: u32 = 90;
const MAX_SINCE_DAYS: u32 = 36_500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedConfig {
    pub(crate) attachments: crate::attachments::Configuration,
    pub(crate) path: PathBuf,
    pub(crate) loaded: bool,
    pub(crate) shared: Option<SharedConfig>,
    pub(crate) embeddings: Option<crate::semantic::remote::Configuration>,
    pub(crate) gold: crate::gold::config::Configuration,
    pub(crate) providers: ProviderRoots,
    pub(crate) since_days: Option<u32>,
    pub(crate) destination: IndexDestination,
    pub(crate) local_filter: Option<IndexFilter>,
    pub(crate) shared_filter: Option<IndexFilter>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IndexFilter {
    pub(crate) providers: Vec<Provider>,
}

impl IndexFilter {
    pub(crate) fn apply(filter: Option<&Self>, options: &mut crate::silver::IndexOptions) {
        if let Some(filter) = filter {
            options
                .providers
                .retain(|provider, _| filter.providers.contains(provider));
        }
    }
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
}

#[derive(Debug, Serialize)]
pub(crate) struct ConfigurationStatus {
    attachments: crate::attachments::Configuration,
    path: PathBuf,
    loaded: bool,
    gold: crate::gold::config::Configuration,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    local: Option<IndexFilter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shared: Option<IndexFilter>,
}

impl ResolvedConfig {
    pub(crate) fn status(&self) -> ConfigurationStatus {
        ConfigurationStatus {
            attachments: self.attachments.clone(),
            path: self.path.clone(),
            loaded: self.loaded,
            gold: self.gold.clone(),
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
                local: self.local_filter.clone(),
                shared: self.shared_filter.clone(),
            },
        }
    }
}
