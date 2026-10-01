//! Authenticated pooled HTTP transport and typed read endpoints.
use super::{Freshness, protocol};
use crate::{AppError, config::SharedConfig, storage::Message};
use serde::Serialize;
use std::{sync::LazyLock, time::Duration};

pub(crate) async fn freshness(config: &SharedConfig) -> Result<Freshness, AppError> {
    get(config, Endpoint::Freshness).await
}

#[derive(Clone, Copy)]
pub(super) enum Endpoint {
    Freshness,
    State,
    Search,
    Threads,
    View,
    Index,
    Forget,
}

impl Endpoint {
    const fn route(self) -> &'static str {
        match self {
            Self::Freshness => "freshness",
            Self::State => "state",
            Self::Search => "search",
            Self::Threads => "threads",
            Self::View => "view",
            Self::Index => "index",
            Self::Forget => "forget",
        }
    }

    pub(super) const fn timeout(self) -> Duration {
        match self {
            // Publication can include a large streaming snapshot. Interactive
            // reads must fail promptly so local results remain responsive.
            Self::Index | Self::Forget | Self::State => Duration::from_secs(600),
            _ => Duration::from_secs(2),
        }
    }
}

// Keep the transport pool alive across MCP calls and watch refreshes. Identity
// headers belong to each request so configuration/token changes cannot retain
// another node's credentials in a cached client.
static HTTP_CLIENT: LazyLock<Result<reqwest::Client, reqwest::Error>> = LazyLock::new(|| {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(600))
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
pub(super) async fn response<T: serde::de::DeserializeOwned>(
    mut request: reqwest::RequestBuilder,
) -> Result<T, AppError> {
    use tracing::Instrument;
    let span = tracing::info_span!(
        "shared_http_request",
        trace_id = tracing::field::Empty,
        status = tracing::field::Empty,
        outcome = tracing::field::Empty,
        elapsed_ms = tracing::field::Empty,
    );
    if !span.is_disabled()
        && let Some(id) = crate::observability::http::TraceId::generate()
    {
        span.record("trace_id", id.as_str());
        request = request.header(crate::observability::http::HEADER, id.as_str());
    }
    async {
        let started = std::time::Instant::now();
        let result = response_inner(request).await;
        tracing::Span::current().record("elapsed_ms", crate::progress::milliseconds(started));
        tracing::Span::current()
            .record("outcome", if result.is_ok() { "success" } else { "error" });
        result
    }
    .instrument(span)
    .await
}

async fn response_inner<T: serde::de::DeserializeOwned>(
    request: reqwest::RequestBuilder,
) -> Result<T, AppError> {
    let response = request.send().await.map_err(|error| {
        AppError::shared(if error.is_timeout() {
            "shared index HTTP request timed out"
        } else {
            "shared index HTTP request failed; check the API address and network"
        })
    })?;
    tracing::Span::current().record("status", response.status().as_u16());
    if !response.status().is_success() {
        let status = response.status();
        let message = match status.as_u16() {
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
    response
        .json()
        .await
        .map_err(|_| AppError::shared("invalid shared index HTTP response"))
}
pub(super) async fn get<T: serde::de::DeserializeOwned>(
    config: &SharedConfig,
    endpoint: Endpoint,
) -> Result<T, AppError> {
    response(request(config, reqwest::Method::GET, endpoint)?).await
}
pub(super) async fn post<T: serde::de::DeserializeOwned>(
    config: &SharedConfig,
    endpoint: Endpoint,
    body: &impl Serialize,
) -> Result<T, AppError> {
    response(request(config, reqwest::Method::POST, endpoint)?.json(body)).await
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
            content: crate::storage::MessageContent::Canonical,
        },
    )
    .await
}
