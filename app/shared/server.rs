use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Extension, Request, State, rejection::JsonRejection};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod, Runtime, Timeouts};
use futures_util::TryStreamExt;
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

use super::{backend, protocol};
use crate::AppError;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Identity {
    pub group_id: String,
    pub node_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorizedNode {
    group_id: String,
    node_id: String,
    token_env: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Configuration {
    version: u32,
    #[serde(default = "default_listen")]
    listen: SocketAddr,
    database_url_env: String,
    #[serde(default = "default_connections")]
    max_connections: usize,
    nodes: Vec<AuthorizedNode>,
}
fn default_listen() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 8787))
}
const fn default_connections() -> usize {
    16
}

#[derive(Clone)]
struct App {
    pool: Pool,
    identities: Arc<HashMap<blake3::Hash, Identity>>,
    uploads: Arc<tokio::sync::Semaphore>,
}

struct ApiError(StatusCode, AppError);
impl From<AppError> for ApiError {
    fn from(error: AppError) -> Self {
        let status = match error.error.kind {
            "usage" | "configuration" => StatusCode::BAD_REQUEST,
            "conflict" => StatusCode::CONFLICT,
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
fn env_name(name: &str) -> Result<(), AppError> {
    if name.is_empty()
        || !name
            .bytes()
            .enumerate()
            .all(|(i, b)| b.is_ascii_alphabetic() || b == b'_' || (i > 0 && b.is_ascii_digit()))
    {
        return Err(AppError::configuration(
            "invalid credential environment variable name",
        ));
    }
    Ok(())
}
fn identities(config: &Configuration) -> Result<HashMap<blake3::Hash, Identity>, AppError> {
    if config.version != 1
        || config.nodes.is_empty()
        || !(1..=128).contains(&config.max_connections)
    {
        return Err(AppError::configuration(
            "server requires version 1, configured nodes, and 1..128 connections",
        ));
    }
    let mut result = HashMap::new();
    let mut unique = HashSet::new();
    for node in &config.nodes {
        crate::config::validate_identifier(&node.group_id, "group_id")?;
        crate::config::validate_identifier(&node.node_id, "node_id")?;
        env_name(&node.token_env)?;
        let token = std::env::var(&node.token_env).map_err(|_| {
            AppError::configuration(format!("set {} to the node's API token", node.token_env))
        })?;
        if token.len() < 32 || token.len() > 4096 || !token.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(AppError::configuration(
                "API tokens require 32..4096 printable ASCII bytes",
            ));
        }
        if !unique.insert((&node.group_id, &node.node_id))
            || result
                .insert(
                    blake3::hash(token.as_bytes()),
                    Identity {
                        group_id: node.group_id.clone(),
                        node_id: node.node_id.clone(),
                    },
                )
                .is_some()
        {
            return Err(AppError::configuration(
                "each node identity and API token must be unique",
            ));
        }
    }
    Ok(result)
}
fn pool(config: &Configuration) -> Result<Pool, AppError> {
    env_name(&config.database_url_env)?;
    let url = std::env::var(&config.database_url_env).map_err(|_| {
        AppError::configuration(format!(
            "set {} to the server PostgreSQL connection string",
            config.database_url_env
        ))
    })?;
    let mut options: tokio_postgres::Config = url
        .parse()
        .map_err(|_| AppError::configuration("invalid server PostgreSQL connection string"))?;
    if options.get_ssl_mode() != tokio_postgres::config::SslMode::Disable {
        options.ssl_mode(tokio_postgres::config::SslMode::Require);
    }
    options.connect_timeout(Duration::from_secs(5));
    options.tcp_user_timeout(Duration::from_secs(30));
    options.application_name("mash-server");
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| AppError::configuration("could not configure PostgreSQL TLS"))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let manager = Manager::from_config(
        options,
        tokio_postgres_rustls::MakeRustlsConnect::new(tls),
        ManagerConfig {
            recycling_method: RecyclingMethod::Verified,
        },
    );
    Pool::builder(manager)
        .max_size(config.max_connections)
        .runtime(Runtime::Tokio1)
        .timeouts(Timeouts {
            wait: Some(Duration::from_secs(5)),
            create: Some(Duration::from_secs(5)),
            recycle: Some(Duration::from_secs(5)),
        })
        .build()
        .map_err(|_| AppError::configuration("could not configure PostgreSQL pool"))
}
async fn authorize(State(app): State<App>, mut request: Request, next: Next) -> Response {
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
    tokio::time::timeout(Duration::from_secs(600), next.run(request))
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
        .route("/v1/state", get(state))
        .route("/v1/search", post(search))
        .route("/v1/view", post(view))
        .route("/v1/forget", post(forget))
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
        .layer(middleware::from_fn_with_state(app.clone(), authorize))
        .with_state(app)
}
async fn freshness(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
) -> Result<Json<super::Freshness>, ApiError> {
    Ok(Json(backend::freshness(&app.pool, &id).await?))
}
async fn status(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
) -> Result<Json<crate::storage::Counts>, ApiError> {
    Ok(Json(backend::status(&app.pool, &id).await?))
}
async fn state(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
) -> Result<Json<protocol::NodeState>, ApiError> {
    Ok(Json(backend::state(&app.pool, &id).await?))
}
async fn search(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
    input: Result<Json<protocol::Search>, JsonRejection>,
) -> Result<Json<protocol::Candidates>, ApiError> {
    Ok(Json(backend::search(&app.pool, &id, json(input)?).await?))
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
    Ok(Json(
        backend::view(&app.pool, &id, &input.node_id, &input.id, input.context).await?,
    ))
}
async fn forget(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
    input: Result<Json<protocol::Forget>, JsonRejection>,
) -> Result<Json<protocol::Forgotten>, ApiError> {
    Ok(Json(backend::forget(&app.pool, &id, json(input)?).await?))
}
async fn spool(body: Body) -> Result<tempfile::NamedTempFile, ApiError> {
    let file = crate::blocking(|| {
        tempfile::NamedTempFile::new().map_err(|_| AppError::internal("create incoming index"))
    })
    .await?;
    let output = file
        .reopen()
        .map_err(|_| AppError::internal("open incoming index"))?;
    let mut output = tokio::fs::File::from_std(output);
    let stream = body.into_data_stream().map_err(std::io::Error::other);
    let mut input = tokio::io::BufReader::new(tokio_util::io::StreamReader::new(stream));
    let mut line_bytes = 0usize;
    let mut total = 0u64;
    loop {
        let chunk = input
            .fill_buf()
            .await
            .map_err(|_| AppError::usage("incomplete index upload"))?;
        if chunk.is_empty() {
            break;
        }
        for byte in chunk {
            line_bytes += 1;
            total += 1;
            if line_bytes > 8 * 1024 * 1024 || total > 8 * 1024 * 1024 * 1024 {
                return Err(ApiError(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    AppError::usage("index upload exceeds limits"),
                ));
            }
            if *byte == b'\n' {
                line_bytes = 0;
            }
        }
        output
            .write_all(chunk)
            .await
            .map_err(|_| AppError::internal("write incoming index"))?;
        let consumed = chunk.len();
        input.consume(consumed);
    }
    if line_bytes != 0 {
        return Err(AppError::usage("unterminated index event").into());
    }
    output
        .flush()
        .await
        .map_err(|_| AppError::internal("flush incoming index"))?;
    Ok(file)
}
async fn index(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
    request: Request,
) -> Result<Json<protocol::Indexed>, ApiError> {
    let _permit = app
        .uploads
        .clone()
        .try_acquire_owned()
        .map_err(|_| AppError::shared("shared index upload capacity is busy; retry later"))?;
    if request
        .headers()
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        != Some("application/x-ndjson")
    {
        return Err(AppError::usage("index uploads require application/x-ndjson").into());
    }
    let file = spool(request.into_body()).await?;
    Ok(Json(backend::index(&app.pool, &id, file).await?))
}
pub(crate) async fn run(path: &Path) -> Result<(), AppError> {
    let path = path.to_path_buf();
    let config: Configuration = crate::blocking(move || {
        use std::io::Read;
        if !std::fs::metadata(&path)
            .map_err(|_| AppError::configuration("could not inspect server configuration"))?
            .is_file()
        {
            return Err(AppError::configuration(
                "server configuration must be a regular file",
            ));
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|_| AppError::configuration("could not open server configuration"))?
            .take(65537)
            .read_to_end(&mut bytes)
            .map_err(|_| AppError::configuration("could not read server configuration"))?;
        if bytes.len() > 65536 {
            return Err(AppError::configuration(
                "server configuration exceeds 65536 bytes",
            ));
        }
        serde_json::from_slice(&bytes)
            .map_err(|_| AppError::configuration("invalid server configuration"))
    })
    .await?;
    let identities = identities(&config)?;
    let pool = pool(&config)?;
    let groups: HashSet<_> = identities.values().map(|id| id.group_id.clone()).collect();
    backend::initialize(&pool, &groups.into_iter().collect::<Vec<_>>()).await?;
    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .map_err(|_| AppError::configuration("could not bind HTTP listener"))?;
    let address = listener
        .local_addr()
        .map_err(|_| AppError::internal("get HTTP listener address"))?;
    eprintln!("{}", serde_json::json!({"listening":address.to_string()}));
    let app = App {
        pool,
        identities: Arc::new(identities),
        uploads: Arc::new(tokio::sync::Semaphore::new(config.max_connections)),
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
