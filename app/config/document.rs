//! Strict TOML input models and semantic validation, without filesystem I/O.
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{DEFAULT_SINCE_DAYS, IndexDestination, MAX_SINCE_DAYS, SharedConfig};
use crate::AppError;
use crate::core::installation::Agent;
use crate::provider::{Provider, ProviderRoots};

#[derive(Debug)]
pub(super) struct ParsedDocument {
    pub(super) embeddings: super::Embeddings,
    pub(super) reranking: super::Reranking,
    pub(super) assistant: Option<super::Assistant>,
    pub(super) qdrant: super::QdrantConfig,
    pub(super) attachments: crate::attachments::Configuration,
    pub(super) redaction: crate::redaction::Settings,
    pub(super) records: crate::records::capture::Policy,
    pub(super) providers: ProviderRoots,
    pub(super) since_days: Option<u32>,
    pub(super) shared: Option<SharedConfig>,
    pub(super) views: crate::views::config::Configuration,
    pub(super) destination: IndexDestination,
    pub(super) connected_agents: Vec<Agent>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawDocument {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agents: Option<RawAgents>,
    #[serde(default)]
    embeddings: super::Embeddings,
    #[serde(default)]
    reranking: super::Reranking,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    assistant: Option<super::Assistant>,
    #[serde(default)]
    records: crate::records::capture::Policy,
    #[serde(default)]
    qdrant: super::QdrantConfig,
    #[serde(default)]
    attachments: crate::attachments::Configuration,
    version: u32,
    providers: RawProviders,
    #[serde(default)]
    index: RawIndex,
    shared: Option<SharedConfig>,
    #[serde(default)]
    views: crate::views::config::Configuration,
    #[serde(default)]
    redaction: crate::redaction::Settings,
}

type RawProviders = BTreeMap<Provider, RawProvider>;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawProvider {
    roots: Vec<PathBuf>,
}

/// The agents `mash init` registered Mash with.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawAgents {
    connected: Vec<Agent>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawIndex {
    since_days: Option<u32>,
    #[serde(default)]
    all_history: bool,
    destination: Option<IndexDestination>,
}

/// Change only the settings offered by setup, preserving all other typed fields.
pub(super) fn configure(text: &str, choices: &super::init::Choices) -> Result<String, AppError> {
    parse_document(text.as_bytes())?;
    let mut document: RawDocument = toml::from_str(text)
        .map_err(|error| AppError::configuration(format!("invalid configuration: {error}")))?;
    document.providers = choices
        .providers
        .iter()
        .map(|(provider, roots)| {
            (
                *provider,
                RawProvider {
                    roots: roots.clone(),
                },
            )
        })
        .collect();
    document.index.since_days = choices.since_days;
    document.index.all_history = choices.since_days.is_none();
    document.index.destination = Some(if choices.shared.is_some() {
        IndexDestination::Both
    } else {
        IndexDestination::Local
    });
    document.shared.clone_from(&choices.shared);
    document.records.clone_from(&choices.records);
    let text = toml::to_string_pretty(&document)
        .map_err(|error| AppError::configuration(format!("serialize configuration: {error}")))?;
    parse_document(text.as_bytes())?;
    Ok(text)
}

/// What the dashboard's settings page may change; see [`super::settings`].
pub(super) struct Edit<'a> {
    pub(super) providers: &'a ProviderRoots,
    pub(super) since_days: Option<u32>,
    pub(super) destination: IndexDestination,
    pub(super) memory_capture: bool,
    pub(super) redaction: crate::redaction::Settings,
}

/// Apply `edit` to the file's text, preserving every other typed field. The
/// result is validated as a whole before it is returned.
pub(super) fn edit(text: &str, edit: &Edit) -> Result<String, AppError> {
    parse_document(text.as_bytes())?;
    let mut document: RawDocument = toml::from_str(text)
        .map_err(|error| AppError::configuration(format!("invalid configuration: {error}")))?;
    document.providers = edit
        .providers
        .iter()
        .map(|(provider, roots)| {
            (
                *provider,
                RawProvider {
                    roots: roots.clone(),
                },
            )
        })
        .collect();
    document.index.since_days = edit.since_days;
    document.index.all_history = edit.since_days.is_none();
    document.index.destination = Some(edit.destination);
    document.records.enabled = edit.memory_capture;
    document.redaction = edit.redaction;
    let text = toml::to_string_pretty(&document)
        .map_err(|error| AppError::configuration(format!("serialize configuration: {error}")))?;
    parse_document(text.as_bytes())?;
    Ok(text)
}

/// Each table's own rules, before the rules that span tables.
fn validate_tables(raw: &RawDocument) -> Result<(), AppError> {
    raw.qdrant.validate()?;
    raw.embeddings.validate()?;
    raw.reranking.validate()?;
    raw.assistant
        .as_ref()
        .map_or(Ok(()), super::Assistant::validate)?;
    raw.views.validate()?;
    raw.attachments.validate()
}

pub(super) fn parse_document(bytes: &[u8]) -> Result<ParsedDocument, AppError> {
    let contents = std::str::from_utf8(bytes)
        .map_err(|_| AppError::configuration("configuration must be UTF-8 TOML"))?;
    let raw: RawDocument = toml::from_str(contents).map_err(|error: toml::de::Error| {
        AppError::configuration(format!("invalid configuration TOML: {}", error.message()))
    })?;
    if raw.records.backfill_since.is_some_and(|time| time < 0) {
        return Err(AppError::configuration(
            "records backfill_since must be a nonnegative Unix timestamp in milliseconds",
        ));
    }
    if raw.version != 1 {
        return Err(AppError::configuration(format!(
            "unsupported configuration version: {}",
            raw.version
        )));
    }
    validate_tables(&raw)?;
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
    let connected_agents = raw.agents.map_or_else(Vec::new, |agents| agents.connected);
    if connected_agents
        .iter()
        .enumerate()
        .any(|(index, agent)| connected_agents[..index].contains(agent))
    {
        return Err(AppError::configuration(
            "agents connected must not repeat an agent",
        ));
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
    Ok(ParsedDocument {
        embeddings: raw.embeddings,
        reranking: raw.reranking,
        assistant: raw.assistant,
        qdrant: raw.qdrant,
        attachments: raw.attachments,
        redaction: raw.redaction,
        records: raw.records,
        providers: raw
            .providers
            .into_iter()
            .map(|(provider, roots)| validate_roots(roots, provider).map(|roots| (provider, roots)))
            .collect::<Result<_, _>>()?,
        since_days,
        shared: raw.shared,
        views: raw.views,
        destination,
        connected_agents,
    })
}

/// Add `agents` to the recorded `[agents] connected` list, or `None` when
/// every one is already recorded. A file without the table gets it appended,
/// so its comments and layout are kept.
pub(super) fn connect_agents(text: &str, agents: &[Agent]) -> Result<Option<String>, AppError> {
    let parsed = parse_document(text.as_bytes())?;
    let mut added = Vec::new();
    for &agent in agents {
        if !parsed.connected_agents.contains(&agent) && !added.contains(&agent) {
            added.push(agent);
        }
    }
    if added.is_empty() {
        return Ok(None);
    }
    let mut document: RawDocument = toml::from_str(text)
        .map_err(|error| AppError::configuration(format!("invalid configuration: {error}")))?;
    let updated = if let Some(recorded) = &mut document.agents {
        recorded.connected.extend(added);
        toml::to_string_pretty(&document)
            .map_err(|error| AppError::configuration(format!("serialize configuration: {error}")))?
    } else {
        let table = toml::to_string(&RawDocumentAgents {
            agents: RawAgents { connected: added },
        })
        .map_err(|error| AppError::configuration(format!("serialize configuration: {error}")))?;
        let separator = if text.ends_with('\n') || text.is_empty() {
            ""
        } else {
            "\n"
        };
        format!(
            "{text}{separator}\n# Agents `mash init` registered Mash with; `mash status --installation`\n# reports them as needing repair if their registration goes missing.\n{table}"
        )
    };
    parse_document(updated.as_bytes())?;
    Ok(Some(updated))
}

#[derive(Serialize)]
struct RawDocumentAgents {
    agents: RawAgents,
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
