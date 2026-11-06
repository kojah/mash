//! Metadata discovery independent of inference; merge before global pagination.
use super::required_shared;
use crate::AppError;
use crate::chat_catalog::{Chat, Collector, Page, Request};
use crate::config::ResolvedConfig;
use crate::observability::retrieval::{Capture, Guard, Measurement, Stage};
use crate::shared;
use crate::storage::Storage;
use crate::views::diagnostics::{Artifact, Scope, Source};
use serde::Serialize;
use std::path::Path;
use std::time::Duration;

#[cfg(test)]
mod tests;

#[derive(Debug, Serialize)]
pub(crate) struct Response {
    #[serde(flatten)]
    pub(crate) page: Page,
    pub(crate) shared: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) shared_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) diagnostics: Option<Diagnostics>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Diagnostics {
    policy_version: u16,
    selected_artifacts: Vec<Artifact>,
    requested_scope: Scope,
    completed_sources: Vec<Source>,
    shared_fallback: bool,
    returned_results: usize,
    /// Listing core time; excludes CLI/MCP startup and configuration loading.
    elapsed_ms: u64,
    stages: Vec<Measurement>,
}

/// A bounded catalogue, retaining machine identity even for copied histories.
#[derive(Debug, Serialize)]
pub(crate) struct DashboardCatalog {
    pub(crate) chats: Vec<Chat>,
    pub(crate) local_node: String,
    pub(crate) truncated: bool,
    pub(crate) shared_error: Option<String>,
}

pub(super) async fn dashboard(
    config: &ResolvedConfig,
    database: &Path,
) -> Result<DashboardCatalog, AppError> {
    let local_node = config
        .shared
        .as_ref()
        .map_or("local", |shared| shared.node_id.as_str())
        .to_owned();
    let node = local_node.clone();
    let path = database.to_path_buf();
    let local = crate::blocking(move || {
        Storage::open_silver(&path)?.chat_candidates(
            &Request {
                limit: 100,
                offset: 10_000,
                ..Default::default()
            },
            &node,
        )
    });
    let remote = async {
        match &config.shared {
            Some(shared) => shared::catalog(shared).await,
            None => Ok(Vec::new()),
        }
    };
    let (local, remote) = tokio::join!(local, remote);
    let mut chats = local?;
    let shared_error = match remote {
        Ok(remote) => {
            chats.extend(remote.into_iter().filter(|chat| chat.node_id != local_node));
            None
        }
        Err(error) => Some(error.error.message),
    };
    chats.sort_by(|a, b| {
        b.metadata
            .updated_at
            .cmp(&a.metadata.updated_at)
            .then_with(|| a.node_id.cmp(&b.node_id))
            .then_with(|| a.chat_id.cmp(&b.chat_id))
    });
    let truncated = chats.len() > 10_000;
    chats.truncate(10_000);
    for chat in &mut chats {
        // The dashboard needs display metadata, not provenance or entire prompts
        // used as fallback titles. Avoid serializing/redacting unused evidence.
        chat.metadata.enrichment = None;
        // Redact before truncation: cutting a credential could prevent a pattern
        // from recognizing it and expose its prefix in the title.
        config.redaction.output().metadata(&mut chat.metadata);
        if let Some(title) = &mut chat.metadata.title {
            let first = title
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or(title);
            let mut preview: String = first.chars().take(240).collect();
            if first.chars().count() > 240 {
                preview.push('…');
            }
            *title = preview;
        }
    }
    Ok(DashboardCatalog {
        chats,
        local_node,
        truncated,
        shared_error,
    })
}

#[tracing::instrument(name = "list_chats", skip_all)]
pub(super) async fn run(
    config: &ResolvedConfig,
    database: &Path,
    input: Request,
    local_only: bool,
    shared_only: bool,
    diagnostics: bool,
) -> Result<Response, AppError> {
    let started = std::time::Instant::now();
    let capture = diagnostics.then(Capture::default);
    let work = run_inner(config, database, input, local_only, shared_only);
    let mut response = match &capture {
        Some(capture) => capture.scope(work).await?,
        None => work.await?,
    };
    for thread in &mut response.page.chats {
        config.redaction.output().metadata(&mut thread.metadata);
    }
    if let Some(capture) = capture {
        let use_shared = shared_only || (!local_only && config.shared.is_some());
        response.diagnostics = Some(Diagnostics {
            policy_version: 1,
            selected_artifacts: vec![Artifact::Metadata],
            requested_scope: if shared_only {
                Scope::Shared
            } else if use_shared {
                Scope::Combined
            } else {
                Scope::Local
            },
            completed_sources: if shared_only {
                vec![Source::Shared]
            } else if response.shared {
                vec![Source::Local, Source::Shared]
            } else {
                vec![Source::Local]
            },
            shared_fallback: response.shared_error.is_some(),
            returned_results: response.page.chats.len(),
            elapsed_ms: crate::progress::milliseconds(started),
            stages: capture.finish(),
        });
    }
    Ok(response)
}

async fn run_inner(
    config: &ResolvedConfig,
    database: &Path,
    input: Request,
    local_only: bool,
    shared_only: bool,
) -> Result<Response, AppError> {
    super::validate_chat_request(&input, local_only, shared_only)?;
    let use_shared = shared_only || (!local_only && config.shared.is_some());
    let node = config
        .shared
        .as_ref()
        .map_or("local", |config| config.node_id.as_str());
    let local = async {
        if shared_only {
            return Ok::<Vec<Chat>, AppError>(Vec::new());
        }
        let mut stage = Guard::begin(Stage::LocalMetadata);
        let input = input.clone();
        let path = database.to_path_buf();
        let node = node.to_owned();
        let rows =
            crate::blocking(move || Storage::open_silver(&path)?.chat_candidates(&input, &node))
                .await?;
        stage.complete(None, Some(rows.len()));
        Ok(rows)
    };
    let remote = async {
        if !use_shared {
            return Ok::<Vec<Chat>, AppError>(Vec::new());
        }
        let mut stage = Guard::begin(Stage::SharedMetadata);
        let mut request = input.clone();
        request.exclude_self |= !shared_only;
        let rows = tokio::time::timeout(
            Duration::from_secs(2),
            shared_window(required_shared(config)?, request),
        )
        .await
        .map_err(|_| AppError::shared("shared thread discovery deadline exceeded"))??;
        stage.complete(None, Some(rows.len()));
        Ok(rows)
    };
    let (local, remote) = tokio::join!(local, remote);
    let local = local?;
    let (remote, shared_error) = match remote {
        Ok(rows) => (rows, None),
        Err(error) if !shared_only => (Vec::new(), Some(error.error.message)),
        Err(error) => return Err(error),
    };
    let mut collector = Collector::new(input)?;
    let mut stage = Guard::begin(Stage::MetadataMerge);
    let candidates = local.len() + remote.len();
    for thread in local.into_iter().chain(remote) {
        collector.push(thread);
    }
    let page = collector.finish();
    stage.complete(Some(candidates), Some(page.chats.len()));
    Ok(Response {
        page,
        shared: use_shared && shared_error.is_none(),
        shared_error,
        diagnostics: None,
    })
}

async fn shared_window(
    config: &crate::config::SharedConfig,
    mut request: Request,
) -> Result<Vec<Chat>, AppError> {
    let window = request.offset + request.limit + 1;
    request.offset = 0;
    let mut rows = Vec::new();
    while rows.len() < window {
        request.limit = (window - rows.len()).min(100);
        let page = shared::list_chats(config, &request).await?;
        if page.chats.len() > request.limit
            || (page.next_offset.is_some() && page.chats.len() != request.limit)
            || page
                .next_offset
                .is_some_and(|next| next != request.offset + request.limit || next > 10_000)
        {
            return Err(AppError::shared("invalid shared thread pagination"));
        }
        rows.extend(page.chats);
        let Some(offset) = page.next_offset else {
            break;
        };
        request.offset = offset;
    }
    Ok(rows)
}
