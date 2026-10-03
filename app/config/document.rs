//! Strict TOML input models and semantic validation, without filesystem I/O.
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use serde::Deserialize;

use super::{DEFAULT_SINCE_DAYS, IndexDestination, IndexFilter, MAX_SINCE_DAYS, SharedConfig};
use crate::AppError;
use crate::provider::{Provider, ProviderRoots};

#[derive(Debug)]
pub(super) struct ParsedDocument {
    pub(super) providers: ProviderRoots,
    pub(super) since_days: Option<u32>,
    pub(super) shared: Option<SharedConfig>,
    pub(super) embeddings: Option<crate::semantic::remote::Configuration>,
    pub(super) gold: crate::gold::config::Configuration,
    pub(super) destination: IndexDestination,
    pub(super) local_filter: Option<IndexFilter>,
    pub(super) shared_filter: Option<IndexFilter>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDocument {
    version: u32,
    providers: RawProviders,
    #[serde(default)]
    index: RawIndex,
    shared: Option<SharedConfig>,
    embeddings: Option<crate::semantic::remote::Configuration>,
    #[serde(default)]
    gold: crate::gold::config::Configuration,
}

type RawProviders = BTreeMap<Provider, RawProvider>;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProvider {
    roots: Vec<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawIndex {
    since_days: Option<u32>,
    #[serde(default)]
    all_history: bool,
    destination: Option<IndexDestination>,
    local: Option<IndexFilter>,
    shared: Option<IndexFilter>,
}

pub(super) fn parse_document(bytes: &[u8]) -> Result<ParsedDocument, AppError> {
    let contents = std::str::from_utf8(bytes)
        .map_err(|_| AppError::configuration("configuration must be UTF-8 TOML"))?;
    let raw: RawDocument = toml::from_str(contents).map_err(|error: toml::de::Error| {
        AppError::configuration(format!("invalid configuration TOML: {}", error.message()))
    })?;
    if raw.version != 1 {
        return Err(AppError::configuration(format!(
            "unsupported configuration version: {}",
            raw.version
        )));
    }
    if let Some(embeddings) = &raw.embeddings {
        embeddings.validate()?;
    }
    raw.gold.validate()?;
    if raw.index.all_history && raw.index.since_days.is_some() {
        return Err(AppError::configuration(
            "index all_history and since_days cannot both be specified",
        ));
    }
    let since_days = if raw.index.all_history {
        None
    } else {
        Some(raw.index.since_days.unwrap_or(DEFAULT_SINCE_DAYS))
    };
    if since_days.is_some_and(|days| !(1..=MAX_SINCE_DAYS).contains(&days)) {
        return Err(AppError::configuration(
            "index since_days must be between 1 and 36500",
        ));
    }
    if let Some(shared) = &raw.shared {
        validate_identifier(&shared.group_id, "group ID")?;
        validate_identifier(&shared.node_id, "node ID")?;
        let name = &shared.token_env;
        if name.is_empty()
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || name.as_bytes()[0].is_ascii_digit()
        {
            return Err(AppError::configuration("invalid token_env"));
        }
        let url = reqwest::Url::parse(&shared.api_url)
            .map_err(|_| AppError::configuration("invalid shared api_url"))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(AppError::configuration(
                "shared api_url must be HTTP(S), without credentials, query, or fragment",
            ));
        }
    }
    let destination = raw.index.destination.unwrap_or(if raw.shared.is_some() {
        IndexDestination::Both
    } else {
        IndexDestination::Local
    });
    if destination != IndexDestination::Local && raw.shared.is_none() {
        return Err(AppError::configuration(
            "both indexing destinations require shared configuration",
        ));
    }
    for filter in [&raw.index.local, &raw.index.shared].into_iter().flatten() {
        let mut unique = HashSet::new();
        for provider in &filter.providers {
            if !unique.insert(provider) {
                return Err(AppError::configuration(
                    "index provider filters must contain unique supported provider names",
                ));
            }
            if !raw.providers.contains_key(provider) {
                return Err(AppError::configuration(
                    "index provider filter selects a disabled provider",
                ));
            }
        }
    }
    validate_local_coverage(&raw, destination)?;
    Ok(ParsedDocument {
        providers: raw
            .providers
            .into_iter()
            .map(|(provider, roots)| validate_roots(roots, provider).map(|roots| (provider, roots)))
            .collect::<Result<_, _>>()?,
        since_days,
        shared: raw.shared,
        embeddings: raw.embeddings,
        gold: raw.gold,
        destination,
        local_filter: raw.index.local,
        shared_filter: raw.index.shared,
    })
}

pub(crate) fn validate_identifier(value: &str, label: &str) -> Result<(), AppError> {
    let bytes = value.as_bytes();
    let valid = (1..=255).contains(&bytes.len())
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    if valid {
        Ok(())
    } else {
        Err(AppError::configuration(format!("invalid {label}: {value}")))
    }
}

fn validate_roots(
    provider: RawProvider,
    provider_name: Provider,
) -> Result<Vec<PathBuf>, AppError> {
    if provider.roots.is_empty() {
        return Err(AppError::configuration(format!(
            "provider {provider_name} roots must not be empty"
        )));
    }
    let mut unique = HashSet::with_capacity(provider.roots.len());
    for root in &provider.roots {
        if !root.is_absolute() {
            return Err(AppError::configuration(format!(
                "provider {provider_name} root is not absolute: {}",
                root.display()
            )));
        }
        if !unique.insert(root) {
            return Err(AppError::configuration(format!(
                "provider {provider_name} has duplicate root: {}",
                root.display()
            )));
        }
    }
    Ok(provider.roots)
}

fn validate_local_coverage(
    raw: &RawDocument,
    destination: IndexDestination,
) -> Result<(), AppError> {
    if destination == IndexDestination::Both
        && let Some(local) = &raw.index.local
    {
        for &provider in raw.providers.keys() {
            let forwarded = raw
                .index
                .shared
                .as_ref()
                .is_none_or(|shared| shared.providers.contains(&provider));
            if forwarded && !local.providers.contains(&provider) {
                return Err(AppError::configuration(
                    "the local provider allowlist must include every provider forwarded to the shared index",
                ));
            }
        }
    }
    Ok(())
}
