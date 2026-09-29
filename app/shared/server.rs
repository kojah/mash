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
    pub generation: String,
    pub dimensions: usize,
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
    #[serde(default)]
    embeddings: EmbeddingSpace,
    #[serde(default = "default_listen")]
    listen: SocketAddr,
    #[serde(default)]
    backend: BackendChoice,
    database_url_env: Option<String>,
    redis_url_env: Option<String>,
    #[serde(default = "default_connections")]
    max_connections: usize,
    #[serde(default = "default_redis_node_mib")]
    redis_max_node_mib: usize,
    #[serde(default = "default_uploads")]
    max_concurrent_uploads: usize,
    nodes: Vec<AuthorizedNode>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmbeddingSpace {
    model: String,
    dimensions: usize,
}
impl Default for EmbeddingSpace {
    fn default() -> Self {
        Self {
            model: "sentence-transformers/all-MiniLM-L6-v2".into(),
            dimensions: 384,
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum BackendChoice {
    Postgres,
    #[default]
    Redis,
}

fn default_redis_node_mib() -> usize {
    64
}
fn default_uploads() -> usize {
    4
}
#[derive(Clone)]
enum Store {
    Postgres(Pool),
    Redis(super::redis_index::Store),
}
impl Store {
    async fn freshness(&self, id: &Identity) -> Result<super::Freshness, AppError> {
        match self {
            Self::Postgres(pool) => backend::freshness(pool, id).await,
            Self::Redis(store) => store.freshness(id).await,
        }
    }
    async fn status(&self, id: &Identity) -> Result<crate::storage::Counts, AppError> {
        match self {
            Self::Postgres(pool) => backend::status(pool, id).await,
            Self::Redis(store) => store.status(id).await,
        }
    }
    async fn state(&self, id: &Identity) -> Result<protocol::NodeState, AppError> {
        match self {
            Self::Postgres(pool) => backend::state(pool, id).await,
            Self::Redis(store) => store.state(id).await,
        }
    }
    async fn search(
        &self,
        id: &Identity,
        input: protocol::Search,
    ) -> Result<protocol::Candidates, AppError> {
        match self {
            Self::Postgres(pool) => backend::search(pool, id, input).await,
            Self::Redis(store) => store.search(id, input).await,
        }
    }
    async fn view(
        &self,
        id: &Identity,
        input: protocol::View,
    ) -> Result<Vec<crate::storage::Message>, AppError> {
        match self {
            Self::Postgres(pool) => {
                backend::view(pool, id, &input.node_id, &input.id, input.context).await
            }
            Self::Redis(store) => store.view(id, input).await,
        }
    }
    async fn forget(
        &self,
        id: &Identity,
        input: protocol::Forget,
    ) -> Result<protocol::Forgotten, AppError> {
        match self {
            Self::Postgres(pool) => backend::forget(pool, id, input).await,
            Self::Redis(store) => store.forget(id, input).await,
        }
    }
    async fn index(
        &self,
        id: &Identity,
        input: tempfile::NamedTempFile,
    ) -> Result<protocol::Indexed, AppError> {
        match self {
            Self::Postgres(pool) => backend::index(pool, id, input).await,
            Self::Redis(store) => store.index(id, input).await,
        }
    }
}
fn default_listen() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 8787))
}
const fn default_connections() -> usize {
    16
}

#[derive(Clone)]
struct App {
    store: Store,
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
    if !(1..=16).contains(&config.max_concurrent_uploads) {
        return Err(AppError::configuration(
            "max_concurrent_uploads must be between 1 and 16",
        ));
    }
    if !(1..=4096).contains(&config.redis_max_node_mib) {
        return Err(AppError::configuration(
            "redis_max_node_mib must be between 1 and 4096",
        ));
    }
    if config.version != 1
        || config.nodes.is_empty()
        || !(1..=128).contains(&config.max_connections)
    {
        return Err(AppError::configuration(
            "server requires version 1, configured nodes, and 1..128 connections",
        ));
    }
    if !(1..=2048).contains(&config.embeddings.dimensions)
        || config.embeddings.model.trim().is_empty()
        || config.embeddings.model.len() > 255
        || config.embeddings.model.chars().any(char::is_control)
    {
        return Err(AppError::configuration(
            "invalid server embedding model or dimensions",
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
                        generation: crate::semantic::model_space(
                            &config.embeddings.model,
                            config.embeddings.dimensions,
                        ),
                        dimensions: config.embeddings.dimensions,
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
    let name = config
        .database_url_env
        .as_ref()
        .ok_or_else(|| AppError::configuration("PostgreSQL requires database_url_env"))?;
    env_name(name)?;
    let url = std::env::var(name).map_err(|_| {
        AppError::configuration(format!(
            "set {name} to the server PostgreSQL connection string"
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
    Ok(Json(app.store.freshness(&id).await?))
}
async fn status(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
) -> Result<Json<crate::storage::Counts>, ApiError> {
    Ok(Json(app.store.status(&id).await?))
}
async fn state(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
) -> Result<Json<protocol::NodeState>, ApiError> {
    Ok(Json(app.store.state(&id).await?))
}
async fn search(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
    input: Result<Json<protocol::Search>, JsonRejection>,
) -> Result<Json<protocol::Candidates>, ApiError> {
    Ok(Json(app.store.search(&id, json(input)?).await?))
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
    Ok(Json(app.store.view(&id, input).await?))
}
async fn forget(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
    input: Result<Json<protocol::Forget>, JsonRejection>,
) -> Result<Json<protocol::Forgotten>, ApiError> {
    Ok(Json(app.store.forget(&id, json(input)?).await?))
}
async fn spool(body: Body, max_bytes: u64) -> Result<tempfile::NamedTempFile, ApiError> {
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
            if line_bytes > 8 * 1024 * 1024 || total > max_bytes {
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
    let max_bytes = match &app.store {
        Store::Postgres(_) => 8 * 1024 * 1024 * 1024,
        Store::Redis(store) => store.max_node_bytes(),
    };
    let file = spool(request.into_body(), max_bytes).await?;
    Ok(Json(app.store.index(&id, file).await?))
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
        let contents = std::str::from_utf8(&bytes)
            .map_err(|_| AppError::configuration("server configuration must be UTF-8 TOML"))?;
        toml::from_str(contents)
            .map_err(|_| AppError::configuration("invalid server configuration"))
    })
    .await?;
    let identities = identities(&config)?;
    let groups: HashSet<_> = identities.values().map(|id| id.group_id.clone()).collect();
    let groups = groups.into_iter().collect::<Vec<_>>();
    let store = match config.backend {
        BackendChoice::Postgres => {
            if config.redis_url_env.is_some() {
                return Err(AppError::configuration(
                    "PostgreSQL backend does not accept redis_url_env",
                ));
            }
            let pool = pool(&config)?;
            backend::initialize(&pool, &groups).await?;
            Store::Postgres(pool)
        }
        BackendChoice::Redis => {
            if config.database_url_env.is_some() {
                return Err(AppError::configuration(
                    "Redis backend does not accept database_url_env",
                ));
            }
            let name = config
                .redis_url_env
                .as_ref()
                .ok_or_else(|| AppError::configuration("Redis requires redis_url_env"))?;
            env_name(name)?;
            let url = std::env::var(name).map_err(|_| {
                AppError::configuration(format!("set {name} to the server Redis connection string"))
            })?;
            Store::Redis(
                super::redis_index::Store::connect(
                    &url,
                    &groups,
                    config.embeddings.dimensions,
                    config
                        .redis_max_node_mib
                        .checked_mul(1024 * 1024)
                        .ok_or_else(|| {
                            AppError::configuration(
                                "Redis snapshot limit exceeds platform capacity",
                            )
                        })?,
                )
                .await?,
            )
        }
    };
    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .map_err(|_| AppError::configuration("could not bind HTTP listener"))?;
    let address = listener
        .local_addr()
        .map_err(|_| AppError::internal("get HTTP listener address"))?;
    eprintln!("{}", serde_json::json!({"listening":address.to_string()}));
    let app = App {
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
