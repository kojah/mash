//! Metadata discovery independent of inference; merge before global pagination.
use super::required_shared;
use crate::AppError;
use crate::config::ResolvedConfig;
use crate::shared;
use crate::storage::Storage;
use crate::thread_catalog::{Collector, Page, Request, Thread};
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
}

#[tracing::instrument(name = "list_threads", skip_all)]
pub(super) async fn run(
    config: &ResolvedConfig,
    database: &Path,
    input: Request,
    local_only: bool,
    shared_only: bool,
) -> Result<Response, AppError> {
    input.validate()?;
    if local_only && shared_only {
        return Err(AppError::usage(
            "local and shared thread discovery are mutually exclusive",
        ));
    }
    let use_shared = shared_only || (!local_only && config.shared.is_some());
    let node = config
        .shared
        .as_ref()
        .map_or("local", |config| config.node_id.as_str());
    let local = async {
        if shared_only {
            return Ok(Vec::new());
        }
        let input = input.clone();
        let path = database.to_path_buf();
        let node = node.to_owned();
        crate::blocking(move || Storage::open_catalog(&path)?.thread_candidates(&input, &node))
            .await
    };
    let remote = async {
        if !use_shared {
            return Ok(Vec::new());
        }
        let mut request = input.clone();
        request.exclude_self |= !shared_only;
        tokio::time::timeout(
            Duration::from_secs(2),
            shared_window(required_shared(config)?, request),
        )
        .await
        .map_err(|_| AppError::shared("shared thread discovery deadline exceeded"))?
    };
    let (local, remote) = tokio::join!(local, remote);
    let local = local?;
    let (remote, shared_error) = match remote {
        Ok(rows) => (rows, None),
        Err(error) if !shared_only => (Vec::new(), Some(error.error.message)),
        Err(error) => return Err(error),
    };
    let mut collector = Collector::new(input)?;
    for thread in local.into_iter().chain(remote) {
        collector.push(thread);
    }
    Ok(Response {
        page: collector.finish(),
        shared: use_shared && shared_error.is_none(),
        shared_error,
    })
}

async fn shared_window(
    config: &crate::config::SharedConfig,
    mut request: Request,
) -> Result<Vec<Thread>, AppError> {
    let window = request.offset + request.limit + 1;
    request.offset = 0;
    let mut rows = Vec::new();
    while rows.len() < window {
        request.limit = (window - rows.len()).min(100);
        let page = shared::list_threads(config, &request).await?;
        if page.threads.len() > request.limit
            || (page.next_offset.is_some() && page.threads.len() != request.limit)
            || page
                .next_offset
                .is_some_and(|next| next != request.offset + request.limit || next > 10_000)
        {
            return Err(AppError::shared("invalid shared thread pagination"));
        }
        rows.extend(page.threads);
        let Some(offset) = page.next_offset else {
            break;
        };
        request.offset = offset;
    }
    Ok(rows)
}
