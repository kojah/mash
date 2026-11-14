//! Authenticated pooled HTTP transport and typed read endpoints.
use super::{Freshness, protocol};
use crate::{AppError, config::SharedConfig, storage::Message};
use serde::Serialize;
use std::{sync::LazyLock, time::Duration};
use tokio::sync::{Semaphore, SemaphorePermit};
mod body;
mod contexts;
mod query_error;
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

pub async fn freshness(config: &SharedConfig) -> Result<Freshness, AppError> {
    get(config, Endpoint::Freshness).await
}

/// Group-wide duplicate-history report keyed by canonical fingerprints.
pub(crate) async fn fingerprints(
    config: &SharedConfig,
) -> Result<super::FingerprintReport, AppError> {
    get(config, Endpoint::Fingerprints).await
}

#[derive(Clone, Copy)]
pub enum Endpoint {
    Freshness,
    Search,
    Fingerprints,
    Chats,
    Records,
    Record,
    Catalog,
    ChatPage,
    View,
    Contexts,
    /// Bulk-shape fixture for transport tests; the legacy index route it named
    /// was removed from the released router.
    #[allow(
        dead_code,
        reason = "bulk-shape fixture for transport tests after the route cutover"
    )]
    Index,
    Attachments,
    Attachment,
    Query,
    Events,
}

impl Endpoint {
    const fn bulk(self) -> bool {
        matches!(self, Self::Index)
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
            Self::Freshness => "freshness",
            Self::Search => "search",
            Self::Fingerprints => "fingerprints",
            Self::Chats => "chats",
            Self::Records => "records",
            Self::Record => "record",
            Self::Catalog => "catalog",
            Self::ChatPage => "chat",
            Self::View => "view",
            Self::Contexts => "contexts",
            Self::Index => "index",
            Self::Attachments => "attachments",
            Self::Attachment => "attachment",
            Self::Query => "query",
            Self::Events => "events",
        }
    }

    pub(super) const fn timeout(self) -> Duration {
        if self.bulk() {
            super::BULK_TIMEOUT
        } else if matches!(self, Self::Events) {
            // A stream stays open; its reader reconnects after this long.
            Duration::from_secs(3600)
        } else if matches!(self, Self::Attachment) {
            Duration::from_secs(15)
        } else if matches!(self, Self::Fingerprints | Self::Catalog) {
            // These enumerate the fleet. Catalogs include provider titles,
            // which may be full opening prompts rather than short labels.
            Duration::from_secs(10)
        } else if matches!(self, Self::Query) {
            // The server runs a bounded query; leave room for its 30s budget.
            Duration::from_secs(35)
        } else if matches!(self, Self::Search | Self::Freshness) {
            // Exact vector retrieval over a full node can take nearly two
            // seconds before network transit and typed response decoding.
            // Freshness counts each node's silver through its indexes.
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
    let token = config.token().ok_or_else(|| {
        AppError::shared(format!("set {} to this node's API token", config.token_env))
    })?;
    request_with_token(config, method, endpoint, &token)
}

fn request_with_token(
    config: &SharedConfig,
    method: reqwest::Method,
    endpoint: Endpoint,
    token: &str,
) -> Result<reqwest::RequestBuilder, AppError> {
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
pub async fn response<T: serde::de::DeserializeOwned + Send + 'static>(
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
    let retry = matches!(endpoint, Endpoint::Freshness)
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
            let query = matches!(endpoint, Endpoint::Query);
            if (search || query)
                && status == reqwest::StatusCode::BAD_REQUEST
                && let Some(error) = query_error::read(response).await
            {
                return Err(error);
            }
            if query {
                let message = match status.as_u16() {
                    400 => "shared query was rejected; check the target, filters and time budget",
                    404 => "shared query named a node outside this group",
                    408 | 504 => {
                        "shared query exceeded its time budget; narrow the query or choose a smaller node"
                    }
                    _ => "shared query failed; inspect `mash status --shared`",
                };
                let mut error = AppError::shared(message);
                error.error.retryable = matches!(status.as_u16(), 408 | 504);
                return Err(error);
            }
            let message = match status.as_u16() {
                409 if search => "shared index changed while searching; retry the search",
                401 | 403 => {
                    "shared index authorization failed; check node token and group configuration"
                }
                409 => {
                    "shared index revision or node identity conflict; retry indexing or choose a new node_id"
                }
                400 | 413 | 422 => "shared index rejected invalid or oversized data",
                503 => {
                    "shared index service is unavailable or mutation capacity is busy; inspect `mash status --shared` activity for the active node, phase and progress before retrying"
                }
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

pub(crate) async fn list_records(
    config: &SharedConfig,
    input: &crate::records::List,
) -> Result<crate::records::Page, AppError> {
    post(config, Endpoint::Records, input).await
}

pub(crate) async fn get_record(
    config: &SharedConfig,
    input: &crate::records::Get,
) -> Result<crate::records::Record, AppError> {
    post(config, Endpoint::Record, input).await
}

pub(crate) async fn list_chats(
    config: &SharedConfig,
    input: &crate::chat_catalog::Request,
) -> Result<crate::chat_catalog::Page, AppError> {
    input.validate()?;
    post(config, Endpoint::Chats, input).await
}
/// Opens the stream of `change` events for this node's group. The caller
/// reads it and reconnects; nothing here retries.
pub async fn events(config: &SharedConfig) -> Result<reqwest::Response, AppError> {
    let response = request(config, reqwest::Method::GET, Endpoint::Events)?
        .send()
        .await
        .map_err(|_| AppError::shared("could not reach the shared index for live changes"))?;
    if response.status().is_success() {
        Ok(response)
    } else {
        Err(AppError::shared("the shared index offers no live changes"))
    }
}

pub(crate) async fn catalog(
    config: &SharedConfig,
) -> Result<Vec<crate::chat_catalog::Chat>, AppError> {
    get(config, Endpoint::Catalog).await
}

pub(crate) async fn chat_page(
    config: &SharedConfig,
    input: &protocol::ChatPage,
) -> Result<Vec<Message>, AppError> {
    post(config, Endpoint::ChatPage, input).await
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
            content: crate::storage::MessageContent::Canonical,
        },
    )
    .await
}

#[derive(serde::Serialize)]
struct QueryBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    node: Option<&'a str>,
    request: &'a crate::silver_query::Request,
}

/// Run a typed, read-only query against shared silver.
pub(crate) async fn query(
    config: &SharedConfig,
    node: Option<&str>,
    request: &crate::silver_query::Request,
) -> Result<serde_json::Value, AppError> {
    let token = config.token().ok_or_else(|| {
        AppError::shared(format!("set {} to this node's API token", config.token_env))
    })?;
    query_with_token(config, &token, node, request).await
}

pub async fn query_with_token(
    config: &SharedConfig,
    token: &str,
    node: Option<&str>,
    request: &crate::silver_query::Request,
) -> Result<serde_json::Value, AppError> {
    let builder = request_with_token(config, reqwest::Method::POST, Endpoint::Query, token)?
        .json(&QueryBody { node, request });
    response(builder, Endpoint::Query).await
}

#[cfg(test)]
mod admission_tests;
