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
mod connectors;
mod contexts;
pub(super) mod store;
mod sync;

use super::protocol;
use crate::AppError;
use store::Store;

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
    activity: super::activity::Registry,
    mutation_capacity: usize,
    attachments_dir: Option<std::path::PathBuf>,
    transfer_dir: std::path::PathBuf,
    catalog: sync::CatalogHandle,
    store: Store,
    identities: Arc<HashMap<blake3::Hash, Identity>>,
    uploads: Arc<tokio::sync::Semaphore>,
    waiters: Arc<tokio::sync::Semaphore>,
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
    // A WebSocket owns its own durable operation lifetime; the HTTP bulk
    // deadline must not terminate a resumable synchronization connection.
    if request.uri().path() == "/v1/sync" {
        return next.run(request).await;
    }
    let timeout = match request.uri().path() {
        "/v1/index" => super::BULK_TIMEOUT,
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
    let router = Router::new()
        .route("/v1/freshness", get(freshness))
        .route("/v1/search", post(search))
        .route("/v1/chats", post(list_chats))
        .route("/v1/view", post(view))
        .route("/v1/contexts", post(contexts::read))
        .route("/v1/fingerprints", get(fingerprints))
        .route("/v1/source-transfer", post(source_transfer))
        .route("/v1/attachments", post(attachments::list))
        .route("/v1/attachment", post(attachments::get))
        // Connector batches keep their source-scoped receipt contract until the
        // client can send them over /v1/sync with a connector receipt.
        .route("/v1/connector-batches", post(connectors::ingest))
        .route("/v1/sync", get(sync::upgrade));
    router
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
#[derive(serde::Deserialize)]
struct SourceTransfer {
    source_id: String,
    from: String,
    to: String,
}

/// Move ownership of an account-scoped source. Both nodes must belong to the
/// caller's group; the caller must name the current owner.
async fn source_transfer(
    State(app): State<App>,
    Extension(identity): Extension<Identity>,
    Json(request): Json<SourceTransfer>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if request.source_id.is_empty() || request.source_id.len() > 256 {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            AppError::usage("source_id is required"),
        ));
    }
    let caller_group = identity.group_id.clone();
    for node in [&request.from, &request.to] {
        let known = app
            .identities
            .values()
            .any(|candidate| candidate.group_id == caller_group && candidate.node_id == *node);
        if !known {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                AppError::usage("transfer names a node outside this group"),
            ));
        }
    }
    let catalog = app.catalog.clone();
    let group = identity.group_id.clone();
    let source = request.source_id.clone();
    let (from, to) = (request.from.clone(), request.to.clone());
    crate::blocking(move || {
        let catalog = catalog
            .lock()
            .map_err(|_| AppError::internal("source claim lock is poisoned"))?;
        catalog.transfer_source(&group, &source, &from, &to)
    })
    .await
    .map_err(ApiError::from)?;
    Ok(Json(
        serde_json::json!({"source_id": request.source_id, "node_id": request.to}),
    ))
}

async fn fingerprints(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
) -> Result<Json<super::FingerprintReport>, ApiError> {
    Ok(Json(
        app.store
            .fingerprint_report(&id.group_id, id.dimensions)
            .await?,
    ))
}

async fn freshness(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
) -> Result<Json<super::Freshness>, ApiError> {
    let mut freshness = app.store.freshness(&id).await?;
    freshness.activity = Some(app.activity.snapshot(
        &id.group_id,
        app.mutation_capacity,
        app.uploads.available_permits(),
    ));
    freshness.unpublished_nodes =
        Some(unpublished_nodes(&app.identities, &id.group_id, &freshness));
    let root = app.transfer_dir.clone();
    let group = id.group_id.clone();
    let node = id.node_id.clone();
    freshness.baseline = crate::blocking(
        move || -> Result<Option<crate::shared::BaselineCoverage>, AppError> {
            let catalog = sync::open_catalog(&root)?;
            Ok(catalog.latest_backfill(&group, &node)?.map(|backfill| {
                crate::shared::BaselineCoverage {
                    id: backfill.backfill_id,
                    state: backfill.state,
                    covered: backfill.covered,
                    total: backfill.total_conversations,
                }
            }))
        },
    )
    .await
    .ok()
    .flatten();
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
async fn search(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
    input: Result<Json<protocol::Search>, JsonRejection>,
) -> Result<Json<protocol::Candidates>, ApiError> {
    let mut candidates = app.store.search(&id, json(input)?).await?;
    prepare_candidates(&mut candidates, app.store.responses());
    Ok(Json(candidates))
}

pub(super) fn prepare_candidates(
    candidates: &mut protocol::Candidates,
    redaction: crate::redaction::Policy,
) {
    for document in &mut candidates.documents {
        document.projection = redaction.projection(&document.projection);
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
        hit.content = redaction.projection(&hit.content);
        redaction.metadata(&mut hit.metadata);
        redaction.ancestors(&mut hit.ancestors);
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
async fn list_chats(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
    input: Result<Json<crate::chat_catalog::Request>, JsonRejection>,
) -> Result<Json<crate::chat_catalog::Page>, ApiError> {
    let input = json(input)?;
    input.validate()?;
    let mut page = app.store.list_chats(&id, input).await?;
    for thread in &mut page.chats {
        app.store.responses().metadata(&mut thread.metadata);
        app.store.responses().ancestors(&mut thread.ancestors);
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
    let redaction = app.store.responses();
    for message in &mut messages {
        message.content = redaction.text(&message.content);
        redaction.attachments(&mut message.attachments);
        redaction.tools(&mut message.tools);
    }
    Ok(Json(messages))
}
/// Explicit operator changes applied before serving.
pub(crate) struct Activation {
    pub(crate) views: bool,
    pub(crate) query: bool,
    pub(crate) memory: bool,
}

/// `models` is used for connector records, which arrive without vectors, and
/// for `replay` of all views from silver before serving.
pub(crate) async fn run(
    path: &Path,
    activation: Activation,
    models: &Path,
    replay: bool,
) -> Result<(), AppError> {
    let config = configuration::load(path).await?;
    let identities = configuration::identities(&config)?;
    let remote_redaction = crate::redaction::Policy::new(config.redaction.remote_embeddings);
    let store = store::initialize(
        &config,
        &identities,
        activation.views,
        activation.query,
        activation.memory,
    )
    .await?
    .with_embedder(store::connectors::Embedder {
        models: models.to_path_buf(),
        remote: config.embeddings.remote(remote_redaction)?,
    })
    .with_response_redaction(crate::redaction::Policy::new(config.redaction.responses));
    if replay {
        let remote = config.embeddings.remote(remote_redaction)?;
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
    let transfer_dir = config.silver_dir()?.join("sync");
    let catalog = std::sync::Arc::new(std::sync::Mutex::new(sync::open_catalog(&transfer_dir)?));
    let store = store.with_source_claims(catalog.clone());
    let app = App {
        activity: super::activity::Registry::default(),
        mutation_capacity: config.max_concurrent_uploads,
        attachments_dir: config.attachments_dir,
        transfer_dir,
        catalog,
        store,
        identities: Arc::new(identities),
        uploads: Arc::new(tokio::sync::Semaphore::new(config.max_concurrent_uploads)),
        waiters: Arc::new(tokio::sync::Semaphore::new(admission::MAX_WAITERS)),
    };
    // Resume durable transfers before accepting requests. A failure is logged
    // rather than fatal: the endpoint still serves and a client can resume.
    match sync::recover(&app).await {
        Ok(0) => {}
        Ok(resumed) => {
            crate::write_json(
                &mut std::io::stderr(),
                &serde_json::json!({"sync_recovered_operations": resumed}),
            )?;
        }
        Err(error) => {
            tracing::warn!(
                kind = ?error.error.kind,
                message = %error.error.message,
                "durable synchronization recovery failed"
            );
        }
    }
    let expiry = sync::spawn_periodic_expiry(app.clone());
    let apply_retry = sync::spawn_apply_recovery(app.clone());
    let served = axum::serve(listener, router(app))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|_| AppError::internal("HTTP server failed"));
    expiry.abort();
    apply_retry.abort();
    served
}

#[cfg(test)]
mod tests;
