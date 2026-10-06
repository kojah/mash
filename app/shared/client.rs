//! Authenticated pooled HTTP transport and typed read endpoints.
use super::{Freshness, protocol};
use crate::{AppError, config::SharedConfig, records::Message};
use serde::Serialize;
use std::{sync::LazyLock, time::Duration};
use tokio::sync::{Semaphore, SemaphorePermit};
mod body;
mod contexts;
pub(super) use contexts::read as context_batch;
#[cfg(test)]
mod retry_tests;

struct Admission {
    interactive: Semaphore,
    bulk: Semaphore,
}

impl Admission {
    const fn new(interactive: usize, bulk: usize) -> Self {
        Self {
            interactive: Semaphore::const_new(interactive),
            bulk: Semaphore::const_new(bulk),
        }
    }
}

// Preserve the existing eight-way context retrieval budget. Bulk work cannot
// consume those slots, and queued readers do not buffer response bodies.
static ADMISSION: Admission = Admission::new(8, 1);

pub(crate) async fn freshness(config: &SharedConfig) -> Result<Freshness, AppError> {
    get(config, Endpoint::Freshness).await
}

#[derive(Clone, Copy)]
pub(super) enum Endpoint {
    Capabilities,
    Freshness,
    State,
    Silver,
    PublicationPlan,
    Gold,
    Search,
    Threads,
    View,
    Contexts,
    Index,
    Forget,
    Attachments,
    Attachment,
    AttachmentUpload,
}

impl Endpoint {
    const fn bulk(self) -> bool {
        matches!(
            self,
            Self::State
                | Self::Silver
                | Self::PublicationPlan
                | Self::Gold
                | Self::Index
                | Self::Forget
                | Self::AttachmentUpload
        )
    }

    fn capacity(self, admission: &'static Admission) -> &'static Semaphore {
        if self.bulk() {
            &admission.bulk
        } else {
            &admission.interactive
        }
    }
    const fn route(self) -> &'static str {
        match self {
            Self::Capabilities => "capabilities",
            Self::Freshness => "freshness",
            Self::State => "state",
            Self::Silver => "silver",
            Self::PublicationPlan => "publication-plan",
            Self::Gold => "gold",
            Self::Search => "search",
            Self::Threads => "threads",
            Self::View => "view",
            Self::Contexts => "contexts",
            Self::Index => "index",
            Self::Forget => "forget",
            Self::Attachments => "attachments",
            Self::Attachment => "attachment",
            Self::AttachmentUpload => "attachment-upload",
        }
    }

    pub(super) const fn timeout(self) -> Duration {
        if self.bulk() {
            super::BULK_TIMEOUT
        } else if matches!(self, Self::Attachment) {
            Duration::from_secs(15)
        } else if matches!(self, Self::Search) {
            // Exact vector retrieval over a full node can take nearly two
            // seconds before network transit and typed response decoding.
            Duration::from_secs(5)
        } else {
            Duration::from_secs(2)
        }
    }
}

// Keep the transport pool alive across MCP calls and watch refreshes. Identity
// headers belong to each request so configuration/token changes cannot retain
// another node's credentials in a cached client.
static HTTP_CLIENT: LazyLock<Result<reqwest::Client, reqwest::Error>> = LazyLock::new(|| {
    reqwest::Client::builder()
        .gzip(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(super::BULK_TIMEOUT)
        .build()
});

pub(super) fn request(
    config: &SharedConfig,
    method: reqwest::Method,
    endpoint: Endpoint,
) -> Result<reqwest::RequestBuilder, AppError> {
    let token = std::env::var(&config.token_env).map_err(|_| {
        AppError::shared(format!("set {} to this node's API token", config.token_env))
    })?;
    let mut headers = reqwest::header::HeaderMap::new();
    let mut authorization = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
        .map_err(|_| AppError::shared("invalid API token"))?;
    authorization.set_sensitive(true);
    headers.insert(reqwest::header::AUTHORIZATION, authorization);
    for (key, value) in [
        ("x-mash-group", &config.group_id),
        ("x-mash-node", &config.node_id),
    ] {
        headers.insert(
            key,
            reqwest::header::HeaderValue::from_str(value)
                .map_err(|_| AppError::shared("invalid API identity"))?,
        );
    }
    let client = HTTP_CLIENT
        .as_ref()
        .map_err(|_| AppError::shared("could not configure HTTP client"))?;
    Ok(client
        .request(
            method,
            format!(
                "{}/v1/{}",
                config.api_url.trim_end_matches('/'),
                endpoint.route()
            ),
        )
        .headers(headers)
        .timeout(endpoint.timeout()))
}
pub(super) async fn response<T: serde::de::DeserializeOwned + Send + 'static>(
    request: reqwest::RequestBuilder,
    endpoint: Endpoint,
) -> Result<T, AppError> {
    response_admitted(request, endpoint, &ADMISSION).await
}

async fn response_admitted<T: serde::de::DeserializeOwned + Send + 'static>(
    mut request: reqwest::RequestBuilder,
    endpoint: Endpoint,
    admission: &'static Admission,
) -> Result<T, AppError> {
    use tracing::Instrument;
    let span = tracing::info_span!(
        "shared_http_request",
        trace_id = tracing::field::Empty,
        status = tracing::field::Empty,
        outcome = tracing::field::Empty,
        elapsed_ms = tracing::field::Empty,
        queue_ms = tracing::field::Empty,
        endpoint = endpoint.route(),
        retry_attempt = tracing::field::Empty,
        admission = if endpoint.bulk() {
            "bulk"
        } else {
            "interactive"
        },
    );
    if !span.is_disabled()
        && let Some(id) = crate::observability::http::TraceId::generate()
    {
        span.record("trace_id", id.as_str());
        request = request.header(crate::observability::http::HEADER, id.as_str());
    }
    async {
        let started = std::time::Instant::now();
        let mut admitted = false;
        let result = tokio::time::timeout(endpoint.timeout(), async {
            let permit = endpoint
                .capacity(admission)
                .acquire()
                .await
                .map_err(|_| AppError::shared("shared index HTTP admission is unavailable"))?;
            admitted = true;
            tracing::Span::current().record("queue_ms", crate::progress::milliseconds(started));
            // Keep transport/retry state off the nested CLI/MCP indexing future.
            Box::pin(response_inner(request, endpoint, permit)).await
        })
        .await
        .unwrap_or_else(|_| Err(AppError::shared("shared index HTTP request timed out")));
        if !admitted {
            tracing::Span::current().record("queue_ms", crate::progress::milliseconds(started));
        }
        tracing::Span::current().record("elapsed_ms", crate::progress::milliseconds(started));
        tracing::Span::current()
            .record("outcome", if result.is_ok() { "success" } else { "error" });
        result
    }
    .instrument(span)
    .await
}

async fn response_inner<T: serde::de::DeserializeOwned + Send + 'static>(
    mut request: reqwest::RequestBuilder,
    endpoint: Endpoint,
    permit: SemaphorePermit<'static>,
) -> Result<T, AppError> {
    // Negotiation is an idempotent GET and owns no mutation. Busy responses
    // can outlive a newly visible head while the previous writer cleans up.
    // Only negotiation and the explicitly read-only search POST may be replayed.
    // A search can overlap head publication and fail its final revision check.
    let retry = matches!(endpoint, Endpoint::PublicationPlan)
        && request
            .try_clone()
            .and_then(|copy| copy.build().ok())
            .is_some_and(|copy| copy.method() == reqwest::Method::GET);
    let started = std::time::Instant::now();
    let search = matches!(endpoint, Endpoint::Search);
    let mut delay = Duration::from_secs(1);
    let mut attempts = 0;
    loop {
        let next = if retry || search {
            request.try_clone()
        } else {
            None
        };
        let response = request.send().await.map_err(|error| {
            AppError::shared(if error.is_timeout() {
                "shared index HTTP request timed out"
            } else {
                "shared index HTTP request failed; check the API address and network"
            })
        })?;
        tracing::Span::current().record("status", response.status().as_u16());
        let busy = retry && response.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE;
        let changed = search && attempts == 0 && response.status() == reqwest::StatusCode::CONFLICT;
        if (busy || changed)
            && let Some(next) = next
        {
            drop(response);
            attempts += 1;
            tracing::Span::current().record("retry_attempt", attempts);
            if retry {
                crate::progress::emit(&crate::progress::Event::new(
                    crate::progress::Phase::SharedWaiting,
                    crate::progress::milliseconds(started),
                    None,
                ));
            }
            // The enclosing admission deadline bounds all attempts and releases
            // capacity on cancellation. Interactive requests use separate slots.
            tokio::time::sleep(if changed {
                Duration::from_millis(25)
            } else {
                delay
            })
            .await;
            delay = (delay * 2).min(Duration::from_secs(5));
            request = next;
            continue;
        }
        if !response.status().is_success() {
            let status = response.status();
            let message = match status.as_u16() {
                409 if search => "shared index changed while searching; retry the search",
                401 | 403 => {
                    "shared index authorization failed; check node token and group configuration"
                }
                409 => {
                    "shared index revision or node identity conflict; retry indexing or choose a new node_id"
                }
                400 | 413 | 422 => "shared index rejected invalid or oversized data",
                _ => "shared index service is unavailable; retry later",
            };
            let mut error = AppError::shared(message);
            error.error.retryable = !matches!(status.as_u16(), 400 | 401 | 403 | 413 | 422);
            return Err(error);
        }
        return body::read(response, endpoint, permit).await;
    }
}
pub(super) async fn get<T: serde::de::DeserializeOwned + Send + 'static>(
    config: &SharedConfig,
    endpoint: Endpoint,
) -> Result<T, AppError> {
    response(request(config, reqwest::Method::GET, endpoint)?, endpoint).await
}
pub(super) async fn post<T: serde::de::DeserializeOwned + Send + 'static>(
    config: &SharedConfig,
    endpoint: Endpoint,
    body: &impl Serialize,
) -> Result<T, AppError> {
    response(
        request(config, reqwest::Method::POST, endpoint)?.json(body),
        endpoint,
    )
    .await
}

pub(crate) async fn list_threads(
    config: &SharedConfig,
    input: &crate::thread_catalog::Request,
) -> Result<crate::thread_catalog::Page, AppError> {
    input.validate()?;
    post(config, Endpoint::Threads, input).await
}
pub(crate) async fn view(
    config: &SharedConfig,
    node: &str,
    id: &str,
    context: u32,
) -> Result<Vec<Message>, AppError> {
    post(
        config,
        Endpoint::View,
        &protocol::View {
            node_id: node.to_owned(),
            id: id.to_owned(),
            context,
            content: crate::records::MessageContent::Canonical,
        },
    )
    .await
}

#[cfg(test)]
mod admission_tests;
