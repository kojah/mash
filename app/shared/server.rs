use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{DefaultBodyLimit, Extension, Request, State, rejection::JsonRejection};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

mod admission;
mod attachments;
mod configuration;
mod contexts;
pub(super) mod store;
mod upload;

use super::protocol;
use crate::AppError;
use store::Store;
use upload::spool;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Identity {
    pub group_id: String,
    pub node_id: String,
    pub generation: String,
    pub dimensions: usize,
}

#[derive(Clone)]
struct App {
    attachments_dir: Option<std::path::PathBuf>,
    store: Store,
    identities: Arc<HashMap<blake3::Hash, Identity>>,
    uploads: Arc<tokio::sync::Semaphore>,
}

#[derive(serde::Serialize)]
struct Listening {
    listening: std::net::SocketAddr,
}

struct ApiError(StatusCode, AppError);
impl From<AppError> for ApiError {
    fn from(error: AppError) -> Self {
        let status = match error.error.kind {
            crate::ErrorKind::Usage | crate::ErrorKind::Configuration => StatusCode::BAD_REQUEST,
            crate::ErrorKind::Conflict => StatusCode::CONFLICT,
            _ => StatusCode::SERVICE_UNAVAILABLE,
        };
        Self(status, error)
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(self.1)).into_response()
    }
}
fn json<T>(input: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    input
        .map(|Json(input)| input)
        .map_err(|error| ApiError(error.status(), AppError::usage("invalid JSON request")))
}
#[tracing::instrument(name = "http_request", skip_all, fields(request_id = crate::observability::request_id(), process_id = std::process::id(), trace_id = tracing::field::Empty, status = tracing::field::Empty, elapsed_ms = tracing::field::Empty))]
async fn authorize(State(app): State<App>, request: Request, next: Next) -> Response {
    let span = tracing::Span::current();
    if !span.is_disabled() {
        use crate::observability::http::{HEADER, TraceId};
        let mut values = request.headers().get_all(HEADER).iter();
        let id = values
            .next()
            .and_then(|value| {
                values
                    .next()
                    .is_none()
                    .then(|| TraceId::parse(value.as_bytes()))
                    .flatten()
            })
            .or_else(TraceId::generate);
        if let Some(id) = id {
            span.record("trace_id", id.as_str());
        }
    }
    let started = std::time::Instant::now();
    let response = authorize_inner(app, request, next).await;
    tracing::Span::current().record("status", response.status().as_u16());
    tracing::Span::current().record("elapsed_ms", crate::progress::milliseconds(started));
    response
}

async fn authorize_inner(app: App, mut request: Request, next: Next) -> Response {
    let token = request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));
    let identity = token.and_then(|t| app.identities.get(&blake3::hash(t.as_bytes())));
    let Some(identity) = identity else {
        let mut error = AppError::shared("invalid node authorization");
        error.error.retryable = false;
        return ApiError(StatusCode::UNAUTHORIZED, error).into_response();
    };
    for (header, expected) in [
        ("x-mash-group", &identity.group_id),
        ("x-mash-node", &identity.node_id),
    ] {
        if request
            .headers()
            .get(header)
            .is_some_and(|value| value.to_str().ok() != Some(expected.as_str()))
        {
            let mut error = AppError::shared("token does not match configured node identity");
            error.error.retryable = false;
            return ApiError(StatusCode::FORBIDDEN, error).into_response();
        }
    }
    request.extensions_mut().insert(identity.clone());
    let timeout = match request.uri().path() {
        "/v1/index" | "/v1/publication-plan" => super::BULK_TIMEOUT,
        _ => Duration::from_secs(600),
    };
    tokio::time::timeout(timeout, next.run(request))
        .await
        .unwrap_or_else(|_| {
            ApiError(
                StatusCode::REQUEST_TIMEOUT,
                AppError::shared("request timed out"),
            )
            .into_response()
        })
}
fn router(app: App) -> Router {
    Router::new()
        .route("/v1/freshness", get(freshness))
        .route("/v1/status", get(status))
        .route("/v1/capabilities", get(capabilities))
        .route("/v1/publication-plan", get(publication_plan))
        .route("/v1/search", post(search))
        .route("/v1/threads", post(list_threads))
        .route("/v1/view", post(view))
        .route("/v1/contexts", post(contexts::read))
        .route("/v1/attachments", post(attachments::list))
        .route("/v1/attachment", post(attachments::get))
        .route(
            "/v1/attachment-upload",
            post(attachments::upload).layer(DefaultBodyLimit::max(8 * 1024 * 1024)),
        )
        .route("/v1/index", post(index))
        .method_not_allowed_fallback(|| async {
            ApiError(
                StatusCode::METHOD_NOT_ALLOWED,
                AppError::usage("unsupported API method"),
            )
        })
        .fallback(|| async {
            ApiError(StatusCode::NOT_FOUND, AppError::usage("unknown API route"))
        })
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(
            tower_http::compression::CompressionLayer::new()
                .quality(tower_http::CompressionLevel::Fastest),
        )
        .layer(middleware::from_fn_with_state(app.clone(), authorize))
        .with_state(app)
}
async fn freshness(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
) -> Result<Json<super::Freshness>, ApiError> {
    let mut freshness = app.store.freshness(&id).await?;
    freshness.unpublished_nodes =
        Some(unpublished_nodes(&app.identities, &id.group_id, &freshness));
    Ok(Json(freshness))
}

fn unpublished_nodes(
    identities: &HashMap<blake3::Hash, Identity>,
    group: &str,
    freshness: &super::Freshness,
) -> Vec<String> {
    identities
        .values()
        .filter(|id| {
            id.group_id == group
                && !freshness
                    .nodes
                    .iter()
                    .any(|node| node.node_id == id.node_id)
        })
        .map(|id| id.node_id.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}
async fn status(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
) -> Result<Json<crate::storage::Counts>, ApiError> {
    Ok(Json(app.store.status(&id).await?))
}
async fn capabilities(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
) -> Result<Json<crate::gold::config::Build>, ApiError> {
    Ok(Json(app.store.capabilities(&id).await?))
}

async fn publication_plan(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
) -> Result<Response, ApiError> {
    let permit = admission::reserve(&app.uploads)?;
    let (file, permit) = app.store.publication_plan(&id, permit).await?;
    stream_snapshot(file, permit)
}

fn stream_snapshot(
    file: tempfile::NamedTempFile,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> Result<Response, ApiError> {
    use futures_util::StreamExt;
    let bytes = file.as_file().metadata().map_err(AppError::io)?.len();
    let input = tokio::fs::File::from_std(file.reopen().map_err(AppError::io)?);
    let stream = tokio_util::io::ReaderStream::new(input).map(move |frame| {
        let _retain = (&file, &permit);
        frame
    });
    Ok((
        [
            ("content-type", "application/json".to_owned()),
            ("content-length", bytes.to_string()),
        ],
        axum::body::Body::from_stream(stream),
    )
        .into_response())
}
async fn search(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
    input: Result<Json<protocol::Search>, JsonRejection>,
) -> Result<Json<protocol::Candidates>, ApiError> {
    let mut candidates = app.store.search(&id, json(input)?).await?;
    prepare_candidates(&mut candidates);
    Ok(Json(candidates))
}

pub(super) fn prepare_candidates(candidates: &mut protocol::Candidates) {
    for document in &mut candidates.documents {
        document.projection = crate::redaction::projection(&document.projection);
    }
    let projections: HashMap<_, _> = candidates
        .documents
        .iter()
        .map(|document| {
            (
                (document.node_id.as_str(), document.id.as_str()),
                document.projection.as_str(),
            )
        })
        .collect();
    for hit in candidates
        .lexical
        .iter_mut()
        .chain(&mut candidates.semantic)
    {
        hit.content = crate::redaction::projection(&hit.content);
        crate::redaction::metadata(&mut hit.metadata);
        // Clients restore projections before fusion and reranking. Send text
        // once when the same node/message document supplies it; exact matches
        // have no document records and keep their content inline.
        if projections
            .get(&(hit.node_id.as_deref().unwrap_or_default(), hit.id.as_str()))
            .is_some_and(|projection| *projection == hit.content)
        {
            hit.content = String::new();
        }
    }
}
async fn list_threads(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
    input: Result<Json<crate::thread_catalog::Request>, JsonRejection>,
) -> Result<Json<crate::thread_catalog::Page>, ApiError> {
    let input = json(input)?;
    input.validate()?;
    let mut page = app.store.list_threads(&id, input).await?;
    for thread in &mut page.threads {
        crate::redaction::metadata(&mut thread.metadata);
    }
    Ok(Json(page))
}
async fn view(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
    input: Result<Json<protocol::View>, JsonRejection>,
) -> Result<Json<Vec<crate::storage::Message>>, ApiError> {
    let input = json(input)?;
    if input.context > 1000 || input.id.is_empty() || input.id.len() > 4096 {
        return Err(AppError::usage("invalid view request").into());
    }
    let mut messages = app.store.view(&id, input).await?;
    for message in &mut messages {
        message.content = crate::redaction::text(&message.content);
        crate::redaction::attachments(&mut message.attachments);
    }
    Ok(Json(messages))
}
async fn index(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
    request: Request,
) -> Result<Json<protocol::Indexed>, ApiError> {
    let permit = admission::reserve(&app.uploads)?;
    if request
        .headers()
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        != Some("application/x-ndjson")
    {
        return Err(AppError::usage("index uploads require application/x-ndjson").into());
    }
    let max_bytes = app.store.max_node_bytes();
    let file = spool(request.into_body(), max_bytes).await?;
    Ok(Json(
        Box::pin(admission::run(permit, async move {
            app.store.index(&id, file).await
        }))
        .await?,
    ))
}
/// Explicit operator changes applied before serving.
pub(crate) struct Activation {
    pub(crate) gold: bool,
    pub(crate) query: bool,
    pub(crate) memory: bool,
}

pub(crate) async fn run(
    path: &Path,
    activation: Activation,
    replay_models: Option<&Path>,
) -> Result<(), AppError> {
    let config = configuration::load(path).await?;
    let identities = configuration::identities(&config)?;
    let store = store::initialize(
        &config,
        &identities,
        activation.gold,
        activation.query,
        activation.memory,
    )
    .await?;
    if let Some(models) = replay_models {
        let remote = config.embeddings.remote()?;
        let mut nodes = identities.values().collect::<Vec<_>>();
        nodes.sort_by(|a, b| (&a.group_id, &a.node_id).cmp(&(&b.group_id, &b.node_id)));
        for identity in nodes {
            let replayed = store.replay(identity, models, remote.as_ref()).await?;
            crate::write_json(&mut std::io::stderr(), &replayed)?;
        }
    }
    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .map_err(|_| AppError::configuration("could not bind HTTP listener"))?;
    let address = listener
        .local_addr()
        .map_err(|_| AppError::internal("get HTTP listener address"))?;
    crate::write_json(&mut std::io::stderr(), &Listening { listening: address })?;
    let app = App {
        attachments_dir: config.attachments_dir,
        store,
        identities: Arc::new(identities),
        uploads: Arc::new(tokio::sync::Semaphore::new(config.max_concurrent_uploads)),
    };
    axum::serve(listener, router(app))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|_| AppError::internal("HTTP server failed"))
}

#[cfg(test)]
mod tests;
