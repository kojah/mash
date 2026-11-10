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

mod admission;
mod attachments;
mod configuration;
mod contexts;
pub(crate) mod store;
mod sync;

use super::protocol;
use crate::AppError;
use store::Store;

pub(crate) use crate::views::scope::Scope as Identity;

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
    let timeout = Duration::from_secs(600);
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
        .route("/v1/records", post(list_records))
        .route("/v1/record", post(get_record))
        .route("/v1/catalog", get(catalog))
        .route("/v1/chat", post(chat_page))
        .route("/v1/view", post(view))
        .route("/v1/contexts", post(contexts::read))
        .route("/v1/query", post(query))
        .route("/v1/fingerprints", get(fingerprints))
        .route("/v1/attachments", post(attachments::list))
        .route("/v1/attachment", post(attachments::get))
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

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SharedQuery {
    /// Member of the caller's group; defaults to the caller's own node.
    #[serde(default)]
    node: Option<String>,
    request: crate::silver_query::Request,
}

/// Typed, read-only query over one node's shared silver. Scope is derived from
/// the token, never the body: `node` may only name a member of that group, and
/// there is no SQL field to accept.
async fn query(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
    input: Result<Json<SharedQuery>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let SharedQuery { node, request } = json(input)?;
    let target = match node {
        None => id,
        Some(node) => app
            .identities
            .values()
            .find(|candidate| candidate.group_id == id.group_id && candidate.node_id == node)
            .cloned()
            .ok_or_else(|| {
                ApiError(
                    StatusCode::NOT_FOUND,
                    AppError::usage("node is not in this group"),
                )
            })?,
    };
    Ok(Json(
        app.store
            .query(&target, request, Duration::from_secs(30))
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
                    total: backfill.total_chats,
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
async fn list_records(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
    input: Result<Json<crate::records::List>, JsonRejection>,
) -> Result<Json<crate::records::Page>, ApiError> {
    let mut page = app.store.list_records(&id, json(input)?).await?;
    for record in &mut page.records {
        record.redact(app.store.responses());
    }
    Ok(Json(page))
}

async fn get_record(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
    input: Result<Json<crate::records::Get>, JsonRejection>,
) -> Result<Json<crate::records::Record>, ApiError> {
    let mut record = app.store.get_record(&id, json(input)?).await?;
    record.redact(app.store.responses());
    Ok(Json(record))
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

async fn catalog(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
) -> Result<Json<Vec<crate::chat_catalog::Chat>>, ApiError> {
    let mut chats = app
        .store
        .catalog(
            &id,
            &crate::chat_catalog::Request {
                exclude_self: true,
                ..Default::default()
            },
        )
        .await?;
    chats.sort_by(|a, b| {
        b.metadata
            .updated_at
            .cmp(&a.metadata.updated_at)
            .then_with(|| a.node_id.cmp(&b.node_id))
            .then_with(|| a.chat_id.cmp(&b.chat_id))
    });
    // One extra row lets the dashboard report an incomplete catalogue.
    chats.truncate(10_001);
    for chat in &mut chats {
        // The dashboard is this endpoint's only reader and shows one line of
        // each title and no provenance, so neither is masked or sent.
        chat.metadata.enrichment = None;
        if let Some(title) = &mut chat.metadata.title {
            crate::chat_catalog::headline(title);
        }
        app.store.responses().metadata(&mut chat.metadata);
    }
    Ok(Json(chats))
}

async fn chat_page(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
    input: Result<Json<protocol::ChatPage>, JsonRejection>,
) -> Result<Json<Vec<crate::storage::Message>>, ApiError> {
    let input = json(input)?;
    if input.limit == 0
        || input.limit > 500
        || input.chat_id.is_empty()
        || input.chat_id.len() > 4096
    {
        return Err(AppError::usage("invalid chat page request").into());
    }
    let mut messages = app.store.chat_page(&id, input).await?;
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

/// `models` is used to replay views from silver before serving.
pub(crate) async fn run(
    path: &Path,
    activation: Activation,
    models: &Path,
    replay: bool,
) -> Result<(), AppError> {
    let config = configuration::load(path).await?;
    let identities = configuration::identities(&config)?;
    let store = store::initialize(
        &config,
        &identities,
        activation.views,
        activation.query,
        activation.memory,
    )
    .await?
    .with_response_redaction(crate::redaction::Policy::new(config.redaction.responses));
    if replay {
        let mut nodes = identities.values().collect::<Vec<_>>();
        nodes.sort_by(|a, b| (&a.group_id, &a.node_id).cmp(&(&b.group_id, &b.node_id)));
        for identity in nodes {
            let replayed = store.replay(identity, models).await?;
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
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                () = test_parent_exit() => {}
            }
        })
        .await
        .map_err(|_| AppError::internal("HTTP server failed"));
    expiry.abort();
    apply_retry.abort();
    served
}

/// Test servers exit once the test that spawned them does: the test holds the
/// write end of stdin, which the kernel closes however the test ends.
#[cfg(debug_assertions)]
async fn test_parent_exit() {
    if std::env::var_os("MASH_TEST_EXIT_WITH_STDIN").is_none() {
        return std::future::pending().await;
    }
    let mut stdin = tokio::io::stdin();
    let mut buffer = [0; 64];
    while tokio::io::AsyncReadExt::read(&mut stdin, &mut buffer)
        .await
        .is_ok_and(|read| read > 0)
    {}
}

#[cfg(not(debug_assertions))]
async fn test_parent_exit() {
    std::future::pending::<()>().await;
}

#[cfg(test)]
mod tests;
