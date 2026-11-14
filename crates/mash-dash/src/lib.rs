//! `mash dash`: read-only history with optional authenticated service control.
//! It serves the front end built
//! from `dashboard/` (embedded at compile time) and a small JSON API over the
//! same typed core as the CLI and MCP. It binds to loopback by default and
//! answers only requests addressed to the bound host, so other websites cannot
//! reach it through DNS rebinding; it sends no CORS headers.
// The search futures this crate drives nest deeper than the default limit
// when the compiler proves them `Send`, as they do in `mash-app`.
#![recursion_limit = "256"]

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use mash_app::core::{self, Operation, Paths, Request};
use mash_app::{
    BUILD_REVISION, chat_catalog, config, mcp, observability, openapi, records, runtime,
    search_filter, search_results, shared, storage, views, watch, write_json,
};
use mash_model::{AppError, ErrorBody, ErrorKind, progress, provider, redaction};
use std::net::SocketAddr;
use std::sync::Arc;

mod access;
mod assistant;
mod contract;
mod live;
mod services;

mod assets {
    include!(concat!(env!("OUT_DIR"), "/dash_assets.rs"));
}

struct Dash {
    paths: Paths,
    hosts: Vec<String>,
    /// Whether only this machine can reach the dashboard.
    loopback: bool,
    access: access::Access,
    /// Read only the shared index: this machine has no agent history itself.
    shared_only: bool,
    model_work: tokio::sync::Semaphore,
    services: services::Services,
    console: crate::mcp::console::Console,
    live: live::Live,
    /// Questions the assistant may be answering at once.
    questions: Arc<tokio::sync::Semaphore>,
}

/// How other devices reach this dashboard.
pub struct Reach {
    pub listen: SocketAddr,
    /// Names a reverse proxy or private network gives the dashboard.
    pub hosts: Vec<String>,
    /// Environment variable holding the token every visitor signs in with.
    pub access_token_env: Option<String>,
    /// Serve only the shared index.
    pub shared_only: bool,
}

pub async fn run(
    paths: Paths,
    reach: Reach,
    services_config: Option<std::path::PathBuf>,
    fleet_config: Option<std::path::PathBuf>,
    fleet_node: Option<String>,
) -> Result<(), AppError> {
    let Reach {
        listen,
        hosts: names,
        access_token_env,
        shared_only,
    } = reach;
    let access = access::Access::load(access_token_env.as_deref())?;
    if !names.is_empty() && !access.required() {
        return Err(AppError::configuration(
            "--allow-host makes the dashboard reachable from other devices; set --access-token-env as well",
        ));
    }
    let services = tokio::task::spawn_blocking(move || {
        if let Some(path) = fleet_config {
            services::Services::load_fleet(&path, fleet_node.as_deref())
        } else {
            services::Services::load(services_config.as_deref())
        }
    })
    .await
    .map_err(|_| AppError::internal("load services configuration"))??;
    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .map_err(|_| AppError::configuration("could not bind the dashboard address"))?;
    let address = listener
        .local_addr()
        .map_err(|_| AppError::internal("get dashboard address"))?;
    let loopback = address.ip().is_loopback() && names.is_empty();
    let mut hosts = allowed_hosts(address);
    hosts.extend(names);
    let mut started = serde_json::json!({ "dashboard": format!("http://{address}/") });
    if !loopback && !access.required() {
        started["warning"] = "anyone who can reach this address can read your chat history; set --access-token-env to require sign-in".into();
    }
    // Live updates watch the local index and, when one is configured, listen
    // to the shared one. A configuration that cannot be read leaves that out;
    // the history routes report what is wrong with it.
    let database =
        (!shared_only).then(|| paths.db.clone().unwrap_or_else(core::default_database_path));
    let config = paths.config.clone();
    let upstream = tokio::task::spawn_blocking(move || crate::config::load(config.as_deref()))
        .await
        .ok()
        .and_then(Result::ok)
        .and_then(|config| config.shared)
        .map(|config| live::Upstream {
            config,
            own: shared_only,
        });
    let state = Arc::new(Dash {
        console: crate::mcp::console::Console::new(paths.clone()),
        live: live::Live::new(database, upstream),
        questions: Arc::new(tokio::sync::Semaphore::new(assistant::MAX_QUESTIONS)),
        paths,
        hosts,
        loopback,
        access,
        shared_only,
        model_work: tokio::sync::Semaphore::new(1),
        services,
    });
    crate::write_json(&mut std::io::stderr(), &started)?;
    let stopping = state.clone();
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            // Open event streams would otherwise hold the server open.
            stopping.live.close();
        })
        .await
        .map_err(|_| AppError::internal("dashboard server failed"))
}

/// Every API route with its description, and the contract document they add
/// up to (see `contract`).
fn api() -> (Router<Arc<Dash>>, aide::openapi::OpenApi) {
    let mut document = aide::openapi::OpenApi {
        info: aide::openapi::Info {
            title: "mash dash".into(),
            description: Some(
                "The local dashboard's API. Generated from the server's routes; do not edit."
                    .into(),
            ),
            version: "1".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let router = history_routes(assistant::routes(live::routes(services::routes(
        aide::axum::ApiRouter::new(),
    ))))
    .finish_api(&mut document);
    (router, document)
}

/// The routes that read history, status and settings.
fn history_routes(router: aide::axum::ApiRouter<Arc<Dash>>) -> aide::axum::ApiRouter<Arc<Dash>> {
    use aide::axum::routing::get_with;
    use contract::{changes, reads};
    router
        .api_route(
            "/api/health",
            get_with(
                health,
                reads::<Health>("/api/health", "The build this dashboard is running."),
            ),
        )
        .api_route(
            "/api/services/settings",
            get_with(
                settings,
                reads::<Settings>(
                    "/api/services/settings",
                    "The settings in this machine's configuration file.",
                ),
            )
            .post_with(
                change_settings,
                changes::<Settings>("/api/services/settings", "Rewrite the configuration file."),
            ),
        )
        .api_route(
            "/api/status",
            get_with(
                status,
                reads::<core::StatusResponse>(
                    "/api/status",
                    "Local readiness and the resolved configuration.",
                ),
            ),
        )
        .api_route(
            "/api/chats",
            get_with(
                list_chats,
                reads::<core::DashboardCatalog>(
                    "/api/chats",
                    "Every chat's metadata, newest first.",
                ),
            ),
        )
        .api_route(
            "/api/search",
            get_with(
                search,
                reads::<core::SearchResponse>("/api/search", "Chats that match a question."),
            ),
        )
        .api_route(
            "/api/view",
            get_with(
                view,
                reads::<core::ViewResponse>("/api/view", "The messages around one message."),
            ),
        )
        .api_route(
            "/api/chat",
            get_with(
                chat,
                reads::<core::ViewResponse>("/api/chat", "One page of a chat's messages."),
            ),
        )
        .api_route(
            "/api/records",
            get_with(
                list_records,
                reads::<crate::records::Page>("/api/records", "Saved memories, newest first."),
            ),
        )
        .api_route(
            "/api/record",
            get_with(
                record,
                reads::<crate::records::Record>("/api/record", "One memory with its history."),
            ),
        )
        .api_route(
            "/api/console/tools",
            get_with(
                console_tools,
                reads::<crate::mcp::console::Catalogue>(
                    "/api/console/tools",
                    "The tools the MCP server offers, as an agent sees them.",
                ),
            ),
        )
        .api_route(
            "/api/console/call",
            get_with(
                console_call,
                reads::<crate::mcp::console::Answer>(
                    "/api/console/call",
                    "Run one read-only tool.",
                ),
            ),
        )
}

/// The contract every route above keeps, as an `OpenAPI` document.
async fn openapi() -> Response {
    static DOCUMENT: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    axum::Json(DOCUMENT.get_or_init(|| contract::document(&api().1))).into_response()
}

fn router(state: Arc<Dash>) -> Router {
    api()
        .0
        .route("/api/openapi.json", axum::routing::get(openapi))
        .route("/api/session", axum::routing::get(session).post(sign_in))
        .route("/api/session/end", axum::routing::post(sign_out))
        .route("/api/session/code", axum::routing::post(sign_in_code))
        .fallback(axum::routing::get(asset))
        .layer(axum::middleware::from_fn_with_state(state.clone(), guard))
        // The chat catalogue is over a megabyte of JSON for a few thousand
        // chats; a phone on a slow link should not have to carry all of it.
        .layer(
            tower_http::compression::CompressionLayer::new()
                .quality(tower_http::CompressionLevel::Fastest),
        )
        .layer(axum::middleware::from_fn(trace))
        .with_state(state)
}

/// The names a browser uses for the bound address; anything else is refused.
fn allowed_hosts(address: SocketAddr) -> Vec<String> {
    let port = address.port();
    let mut hosts = vec![address.to_string()];
    if address.ip().is_loopback() {
        hosts.extend([
            format!("localhost:{port}"),
            format!("127.0.0.1:{port}"),
            format!("[::1]:{port}"),
        ]);
    }
    hosts
}

/// One span per request when tracing is on (`MASH_TRACE=1`). It opens when
/// the request arrives and closes when the response is ready or the browser
/// goes away, so a span that closes without a status is a request the client
/// abandoned. Only the path is recorded: queries can hold search text.
#[tracing::instrument(name = "http_request", skip_all, fields(request_id = crate::observability::request_id(), process_id = std::process::id(), method = %request.method(), path = request.uri().path(), status = tracing::field::Empty, elapsed_ms = tracing::field::Empty))]
async fn trace(request: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let started = std::time::Instant::now();
    let response = next.run(request).await;
    let span = tracing::Span::current();
    span.record("status", response.status().as_u16());
    span.record("elapsed_ms", crate::progress::milliseconds(started));
    response
}

/// How long a browser may keep a response.
#[derive(Clone, Copy)]
enum Cache {
    Api,
    Hashed,
    Image,
    Page,
}

async fn guard(
    State(state): State<Arc<Dash>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !state
        .hosts
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(host))
    {
        return (StatusCode::MISDIRECTED_REQUEST, "unexpected Host header").into_response();
    }
    let path = request.uri().path();
    let service_path = path == "/api/services" || path.starts_with("/api/services/");
    let session_path = matches!(path, "/api/session" | "/api/session/end");
    // The page and its assets say nothing about history; everything under
    // /api does, except the build stamp and signing in. Another dashboard
    // presents the services token instead of a session.
    let peer =
        service_path && state.services.has_token() && state.services.authorized(request.headers());
    if path.starts_with("/api/")
        && path != "/api/health"
        && !session_path
        && !peer
        && !state.access.signed_in(request.headers())
    {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(AppError::configuration(
                "Sign in with the dashboard access token",
            )),
        )
            .into_response();
    }
    if service_path
        && request.uri().path() != "/api/services/access"
        && !state.services.authorized(request.headers())
    {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(AppError::configuration(
                "Unlock services with the dashboard access token",
            )),
        )
            .into_response();
    }
    let service_action = (session_path
        || path == "/api/session/code"
        || matches!(
            request.uri().path(),
            "/api/services/action"
                | "/api/services/fleet/jobs"
                | "/api/services/settings"
                | "/api/assistant"
        ))
        && request.method() == axum::http::Method::POST;
    if service_action && let Some(origin) = request.headers().get(header::ORIGIN) {
        let valid = origin
            .to_str()
            .ok()
            .and_then(|value| reqwest::Url::parse(value).ok())
            .is_some_and(|url| {
                matches!(url.scheme(), "http" | "https")
                    && url.authority().eq_ignore_ascii_case(host)
            });
        if !valid {
            return StatusCode::FORBIDDEN.into_response();
        }
    }
    if !service_action
        && request.method() != axum::http::Method::GET
        && request.method() != axum::http::Method::HEAD
    {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let cache = Cache::of(request.uri().path());
    harden(next.run(request).await, cache)
}

impl Cache {
    fn of(path: &str) -> Self {
        if path.starts_with("/api/") {
            Self::Api
        } else if path.starts_with("/assets/") {
            Self::Hashed
        } else if std::path::Path::new(path)
            .extension()
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("svg") || extension.eq_ignore_ascii_case("png")
            })
        {
            Self::Image
        } else {
            Self::Page
        }
    }
}

/// The caching and browser-safety headers every response carries.
fn harden(mut response: Response, cache: Cache) -> Response {
    let headers = response.headers_mut();
    // Built assets carry a content hash in their name, so a browser may keep
    // them for good. Logos change rarely. The page itself is always rechecked.
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(match cache {
            Cache::Api => "no-store",
            Cache::Hashed => "public, max-age=31536000, immutable",
            Cache::Image => "public, max-age=86400",
            Cache::Page => "no-cache",
        }),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'",
        ),
    );
    response
}

/// Whether this dashboard asks visitors to sign in, and whether this one has.
async fn session(State(state): State<Arc<Dash>>, headers: axum::http::HeaderMap) -> Response {
    axum::Json(serde_json::json!({
        "required": state.access.required(),
        "signed_in": state.access.signed_in(&headers),
    }))
    .into_response()
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SignIn {
    token: String,
}

/// A reverse proxy that ends TLS says so; the cookie is then kept to HTTPS.
fn forwarded_https(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("https"))
}

async fn sign_in(
    State(state): State<Arc<Dash>>,
    headers: axum::http::HeaderMap,
    input: Result<axum::Json<SignIn>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Ok(axum::Json(input)) = input else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match state
        .access
        .sign_in(&input.token, forwarded_https(&headers))
    {
        Ok(cookie) => (
            [(header::SET_COOKIE, cookie)],
            axum::Json(serde_json::json!({ "required": true, "signed_in": true })),
        )
            .into_response(),
        Err(access::Refusal::Wrong) => (
            StatusCode::UNAUTHORIZED,
            axum::Json(AppError::configuration("That access token is not right")),
        )
            .into_response(),
        Err(access::Refusal::TooMany) => (
            StatusCode::TOO_MANY_REQUESTS,
            axum::Json(AppError::configuration(
                "Too many wrong tokens; wait a minute and try again",
            )),
        )
            .into_response(),
    }
}

/// A short code for signing one more browser in; the guard has already
/// required the caller to be signed in.
async fn sign_in_code(State(state): State<Arc<Dash>>) -> Response {
    if !state.access.required() {
        return (
            StatusCode::CONFLICT,
            axum::Json(AppError::configuration(
                "this dashboard has no access token, so there is nothing to sign in to",
            )),
        )
            .into_response();
    }
    match state.access.issue_code() {
        Ok(code) => axum::Json(serde_json::json!({ "code": code, "minutes": 10 })).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(error)).into_response(),
    }
}

async fn sign_out(State(state): State<Arc<Dash>>, headers: axum::http::HeaderMap) -> Response {
    (
        [(
            header::SET_COOKIE,
            access::Access::sign_out(forwarded_https(&headers)),
        )],
        axum::Json(serde_json::json!({
            "required": state.access.required(),
            "signed_in": !state.access.required(),
        })),
    )
        .into_response()
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct Health {
    build: &'static str,
}

async fn health() -> Response {
    axum::Json(Health {
        build: crate::BUILD_REVISION,
    })
    .into_response()
}

/// Local readiness and the resolved configuration, as `mash status` reports
/// them. Read-only: it names credential variables but never their values.
async fn status(State(state): State<Arc<Dash>>) -> Response {
    let request = Request {
        paths: state.paths.clone(),
        operation: Operation::Status {
            shared: false,
            coverage: None,
        },
    };
    json_result(core::execute(request).await)
}

/// The configuration file this dashboard's Mash reads.
fn configuration_path(paths: &Paths) -> Result<std::path::PathBuf, AppError> {
    match &paths.config {
        Some(path) => Ok(path.clone()),
        None => crate::config::default_path(),
    }
}

async fn settings(State(state): State<Arc<Dash>>) -> Response {
    let paths = state.paths.clone();
    let view = tokio::task::spawn_blocking(move || {
        crate::config::settings::view(&configuration_path(&paths)?)
    })
    .await
    .unwrap_or_else(|_| Err(AppError::internal("read settings")));
    settings_result(view, state.loopback || state.services.managed())
}

/// Rewrites the configuration file. A dashboard other machines can reach
/// accepts this only when it was started with a services configuration, the
/// same opt-in that lets it manage services; like every service route it is
/// then subject to that configuration's access token.
async fn change_settings(
    State(state): State<Arc<Dash>>,
    input: Result<
        axum::Json<crate::config::settings::Change>,
        axum::extract::rejection::JsonRejection,
    >,
) -> Response {
    if !state.loopback && !state.services.managed() {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(AppError::configuration(
                "settings can be changed on the machine itself, or from a dashboard started with a services configuration",
            )),
        )
            .into_response();
    }
    let Ok(axum::Json(change)) = input else {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(AppError::configuration("invalid settings change")),
        )
            .into_response();
    };
    let paths = state.paths.clone();
    let view = tokio::task::spawn_blocking(move || {
        crate::config::settings::apply(&configuration_path(&paths)?, &change)
    })
    .await
    .unwrap_or_else(|_| Err(AppError::internal("write settings")));
    settings_result(view, true)
}

/// The settings, with whether this dashboard accepts changes to them.
#[derive(serde::Serialize, schemars::JsonSchema)]
struct Settings {
    #[serde(flatten)]
    view: crate::config::settings::View,
    editable: bool,
}

fn settings_result(
    view: Result<crate::config::settings::View, AppError>,
    editable: bool,
) -> Response {
    match view {
        Ok(view) => axum::Json(Settings { view, editable }).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({ "error": error.error })),
        )
            .into_response(),
    }
}

async fn list_chats(State(state): State<Arc<Dash>>) -> Response {
    let request = Request {
        paths: state.paths.clone(),
        operation: Operation::DashboardCatalog {
            shared_only: state.shared_only,
        },
    };
    json_result(core::execute(request).await)
}

#[derive(serde::Deserialize, Default, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct RecordParams {
    q: Option<String>,
    kind: Option<crate::records::Kind>,
    state: Option<crate::records::Lifecycle>,
    review: Option<crate::records::Review>,
    chat: Option<String>,
    shared: bool,
    node: Option<String>,
    offset: usize,
    cursor_node: Option<String>,
    cursor_offset: usize,
}

async fn list_records(
    State(state): State<Arc<Dash>>,
    axum::extract::Query(params): axum::extract::Query<RecordParams>,
) -> Response {
    let operation = Operation::Records(crate::records::List {
        query: params.q.filter(|query| !query.trim().is_empty()),
        kind: params.kind,
        state: params.state,
        review: params.review,
        chat: params.chat,
        shared: params.shared || state.shared_only,
        node_id: params.node,
        cursor: params.cursor_node.map(|node_id| crate::records::Cursor {
            node_id,
            offset: params.cursor_offset,
        }),
        offset: params.offset,
        include_inactive: true,
        limit: Some(20),
        ..Default::default()
    });
    json_result(
        core::execute(Request {
            paths: state.paths.clone(),
            operation,
        })
        .await,
    )
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct RecordDetailParams {
    id: String,
    node: Option<String>,
    before_revision: Option<i64>,
    #[serde(default)]
    link_offset: usize,
}

async fn record(
    State(state): State<Arc<Dash>>,
    axum::extract::Query(params): axum::extract::Query<RecordDetailParams>,
) -> Response {
    let operation = Operation::Record(crate::records::Get {
        id: params.id,
        node_id: params.node,
        history: true,
        before_revision: params.before_revision,
        link_offset: params.link_offset,
        ..Default::default()
    });
    json_result(
        core::execute(Request {
            paths: state.paths.clone(),
            operation,
        })
        .await,
    )
}

#[derive(serde::Deserialize, Default, schemars::JsonSchema)]
#[serde(default)]
struct SearchParams {
    q: String,
    /// Comma-separated provider names, such as `claude-code,codex`.
    provider: String,
    /// Comma-separated machine (node) IDs.
    machine: String,
    /// Comma-separated recorded working directories.
    project: String,
    days: Option<u32>,
    offset: usize,
    /// relevance (default) or recent: newest first within each page of matching chats.
    sort: crate::search_filter::Sort,
}

fn split(list: &str) -> Vec<String> {
    list.split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

fn providers(list: &str) -> Result<Vec<crate::provider::Provider>, AppError> {
    split(list)
        .into_iter()
        .map(|name| {
            serde_json::from_value(serde_json::Value::String(name))
                .map_err(|_| AppError::usage("unknown provider"))
        })
        .collect()
}

/// A query searches messages; an empty one lists the matching chats, newest first.
fn search_operation(params: SearchParams, shared_only: bool) -> Result<Operation, AppError> {
    let provider = providers(&params.provider)?;
    if params.q.trim().is_empty() {
        let input = crate::chat_catalog::Request {
            provider,
            node: split(&params.machine),
            ..Default::default()
        };
        return Ok(Operation::ListChats {
            input: Box::new(input),
            local: false,
            shared: shared_only,
            diagnostics: false,
        });
    }
    let filters = crate::search_filter::Selection {
        provider,
        node: split(&params.machine),
        project: split(&params.project),
        days: params.days,
        sort: params.sort,
        ..Default::default()
    };
    Ok(Operation::Search {
        shared_vector_search: None,
        query: params.q,
        intent: None,
        limit: Some(10),
        offset: params.offset,
        unit: Some(crate::search_results::SearchUnit::Chats),
        fusion: crate::views::config::FusionOverrides::default(),
        ranking: None,
        diagnostics: false,
        filters: Box::new(filters),
        shared: shared_only,
        local: false,
    })
}

async fn search(
    State(state): State<Arc<Dash>>,
    axum::extract::Query(params): axum::extract::Query<SearchParams>,
) -> Response {
    match search_operation(params, state.shared_only) {
        Ok(operation) => json_result(
            core::execute_with_model_limit(
                Request {
                    paths: state.paths.clone(),
                    operation,
                },
                &state.model_work,
            )
            .await,
        ),
        Err(error) => json_result(Err(error)),
    }
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ViewParams {
    id: String,
    #[serde(default)]
    context: Option<u32>,
    node: Option<String>,
    #[serde(default)]
    shared: bool,
}

/// Messages on each side of the anchor; chunked chat reads are tracked in mash-twg6.
const MAX_VIEW_CONTEXT: u32 = 500;

async fn view(
    State(state): State<Arc<Dash>>,
    axum::extract::Query(params): axum::extract::Query<ViewParams>,
) -> Response {
    let operation = if params.shared {
        if params.node.as_ref().is_none_or(String::is_empty) {
            return json_result(Err(AppError::usage(
                "shared chat reads require a source node",
            )));
        }
        Operation::View {
            id: params.id,
            context: params.context.unwrap_or(50).min(MAX_VIEW_CONTEXT),
            shared: params.node,
        }
    } else {
        Operation::ChatContext {
            id: params.id,
            context: params.context.unwrap_or(50).min(MAX_VIEW_CONTEXT),
            node: params.node,
        }
    };
    json_result(
        core::execute(Request {
            paths: state.paths.clone(),
            operation,
        })
        .await,
    )
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ChatParams {
    id: String,
    #[serde(default)]
    offset: u32,
    #[serde(default)]
    limit: Option<u32>,
    node: Option<String>,
    #[serde(default)]
    shared: bool,
}

/// Messages per page of a chat; whole small chats fit in one.
const MAX_CHAT_PAGE: u32 = 500;

async fn chat(
    State(state): State<Arc<Dash>>,
    axum::extract::Query(params): axum::extract::Query<ChatParams>,
) -> Response {
    let operation = Operation::ChatMessages {
        chat_id: params.id,
        offset: params.offset,
        node: params.node,
        shared: params.shared,
        limit: params
            .limit
            .unwrap_or(MAX_CHAT_PAGE)
            .clamp(1, MAX_CHAT_PAGE),
    };
    json_result(
        core::execute(Request {
            paths: state.paths.clone(),
            operation,
        })
        .await,
    )
}

async fn console_tools() -> Response {
    match crate::mcp::console::Console::catalogue() {
        Ok(catalogue) => axum::Json(catalogue).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(error)).into_response(),
    }
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ConsoleCall {
    name: String,
    /// The tool's arguments as a JSON object; absent means none.
    arguments: Option<String>,
}

/// Runs one read-only tool. A GET like every other history route: the console
/// refuses tools that change data, so nothing here writes.
async fn console_call(
    State(state): State<Arc<Dash>>,
    axum::extract::Query(call): axum::extract::Query<ConsoleCall>,
) -> Response {
    let arguments = match call.arguments.as_deref().filter(|text| !text.is_empty()) {
        None => serde_json::Map::new(),
        Some(text) => match serde_json::from_str(text) {
            Ok(serde_json::Value::Object(arguments)) => arguments,
            _ => {
                return (
                    StatusCode::BAD_REQUEST,
                    axum::Json(AppError::usage("arguments must be a JSON object")),
                )
                    .into_response();
            }
        },
    };
    match state.console.call(&call.name, arguments).await {
        Ok(reply) => axum::Json(reply).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(error)).into_response(),
    }
}

fn json_result(result: Result<core::Response, AppError>) -> Response {
    match result {
        Ok(response) => axum::Json(response).into_response(),
        Err(error) => {
            let status = if error.error.retryable {
                StatusCode::SERVICE_UNAVAILABLE
            } else if error.error.kind == crate::ErrorKind::NotFound {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::BAD_REQUEST
            };
            (
                status,
                axum::Json(serde_json::json!({ "error": error.error })),
            )
                .into_response()
        }
    }
}

async fn asset(uri: Uri) -> Response {
    let path = match uri.path() {
        "/" => "/index.html",
        other => other,
    };
    let Some((_, body)) = assets::ASSETS.iter().find(|(route, _)| *route == path) else {
        if assets::ASSETS.is_empty() {
            return (
                [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                "<!doctype html><title>Mash</title><p>The dashboard has not been built. Run <code>pnpm install &amp;&amp; pnpm build</code> in <code>dashboard/</code>, then rebuild mash. Installed with cargo? See <a href=\"https://getmash.io/installation/#install-the-cli\">the installation guide</a>.</p>",
            )
                .into_response();
        }
        return StatusCode::NOT_FOUND.into_response();
    };
    ([(header::CONTENT_TYPE, content_type(path))], *body).into_response()
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, extension)| extension) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::{Operation, SearchParams, allowed_hosts, content_type, search_operation};

    fn record_fixture() -> (tempfile::TempDir, crate::core::Paths, String) {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.toml");
        std::fs::write(&config, "version=1\nproviders={}\n").unwrap();
        let database = directory.path().join("index.sqlite3");
        drop(crate::storage::Storage::open(&database).unwrap());
        let connection = rusqlite::Connection::open(&database).unwrap();
        crate::storage::packed::register(&connection).unwrap();
        connection.execute_batch("INSERT INTO chats(id,provider) VALUES('fixture','codex'); INSERT INTO messages(id,chat_id,ordinal,role,content_packed) VALUES('source','fixture',0,'user','The fixture uses port 8080.');").unwrap();
        drop(connection);
        let body = crate::records::Body {
            statement: "Fixture uses port 8080".into(),
            rationale: "Reported in chat".into(),
            review: crate::records::Review::Established,
            details: crate::records::Details::Fact {
                state: crate::records::FactState::Current,
                reported_by: "fixture user".into(),
                valid_until: None,
            },
            effective_at: None,
            recorded_by: "HTTP test".into(),
        };
        let mut storage = crate::storage::Storage::open_existing(&database).unwrap();
        let receipt = storage
            .mutate_record(&crate::records::Mutation {
                operation_id: "fixture-create".into(),
                change: crate::records::Change::Create {
                    scope: crate::records::Scope::Personal,
                    body,
                    evidence: vec![crate::records::Citation {
                        message_id: "source".into(),
                        session_id: None,
                        quote: "port 8080".into(),
                        purpose: crate::records::EvidenceUse::Report,
                        expected_fingerprint: None,
                    }],
                    on_duplicate: crate::records::DuplicatePolicy::Report,
                },
            })
            .unwrap();
        (
            directory,
            crate::core::Paths {
                config: Some(config),
                db: Some(database),
                models_dir: None,
            },
            receipt.record_id,
        )
    }

    #[tokio::test]
    async fn only_a_signed_in_caller_mints_a_code_that_signs_another_browser_in() {
        const TOKEN: &str = "0123456789abcdef0123456789abcdef";
        let (_directory, paths, _) = record_fixture();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = super::router(std::sync::Arc::new(super::Dash {
            console: crate::mcp::console::Console::new(crate::core::Paths::default()),
            paths,
            hosts: allowed_hosts(address),
            loopback: false,
            access: super::access::Access::new(Some(TOKEN)),
            shared_only: false,
            model_work: tokio::sync::Semaphore::new(0),
            services: super::services::Services::load(None).unwrap(),
            live: super::live::Live::new(None, None),
            questions: std::sync::Arc::new(tokio::sync::Semaphore::new(2)),
        }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        let url = |path: &str| format!("http://{address}{path}");
        let stranger = client.post(url("/api/session/code")).send().await.unwrap();
        assert_eq!(stranger.status(), reqwest::StatusCode::UNAUTHORIZED);
        let minted: serde_json::Value = client
            .post(url("/api/session/code"))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let code = minted["code"].as_str().unwrap().to_lowercase();
        let with_code = client
            .post(url("/api/session"))
            .json(&serde_json::json!({ "token": code }))
            .send()
            .await
            .unwrap();
        assert_eq!(with_code.status(), reqwest::StatusCode::OK);
        assert!(with_code.headers().contains_key("set-cookie"));
        // The code is spent.
        let again = client
            .post(url("/api/session"))
            .json(&serde_json::json!({ "token": code }))
            .send()
            .await
            .unwrap();
        assert_eq!(again.status(), reqwest::StatusCode::UNAUTHORIZED);
        server.abort();
    }

    #[tokio::test]
    async fn a_reachable_dashboard_serves_history_only_after_sign_in() {
        const TOKEN: &str = "0123456789abcdef0123456789abcdef";
        let (_directory, paths, _) = record_fixture();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut hosts = allowed_hosts(address);
        hosts.push("mash.example.ts.net".into());
        let app = super::router(std::sync::Arc::new(super::Dash {
            console: crate::mcp::console::Console::new(crate::core::Paths::default()),
            paths,
            hosts,
            loopback: false,
            access: super::access::Access::new(Some(TOKEN)),
            shared_only: false,
            model_work: tokio::sync::Semaphore::new(0),
            services: super::services::Services::load(None).unwrap(),
            live: super::live::Live::new(None, None),
            questions: std::sync::Arc::new(tokio::sync::Semaphore::new(2)),
        }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        let url = |path: &str| format!("http://{address}{path}");
        // The page and build stamp are public; nothing that reads history is.
        for path in ["/", "/api/health", "/api/session"] {
            let status = client.get(url(path)).send().await.unwrap().status();
            assert_ne!(status, reqwest::StatusCode::UNAUTHORIZED, "{path}");
        }
        for path in [
            "/api/status",
            "/api/chats",
            "/api/search?q=port",
            "/api/view?id=source",
            "/api/records",
            "/api/console/tools",
            "/api/services",
            "/api/services/access",
            "/api/openapi.json",
        ] {
            let status = client.get(url(path)).send().await.unwrap().status();
            assert_eq!(status, reqwest::StatusCode::UNAUTHORIZED, "{path}");
        }
        let sign_in = |token: &'static str, origin: Option<&'static str>| {
            let mut request = client
                .post(url("/api/session"))
                .json(&serde_json::json!({ "token": token }));
            if let Some(origin) = origin {
                request = request.header("Origin", origin);
            }
            request.send()
        };
        assert_eq!(
            sign_in("wrong", None).await.unwrap().status(),
            reqwest::StatusCode::UNAUTHORIZED
        );
        // Another website cannot sign a visitor's browser in.
        assert_eq!(
            sign_in(TOKEN, Some("https://evil.invalid"))
                .await
                .unwrap()
                .status(),
            reqwest::StatusCode::FORBIDDEN
        );
        let accepted = sign_in(TOKEN, None).await.unwrap();
        assert_eq!(accepted.status(), reqwest::StatusCode::OK);
        let set = accepted.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(set.contains("HttpOnly") && !set.contains(TOKEN));
        let cookie = set.split(';').next().unwrap().to_owned();
        for path in ["/api/status", "/api/view?id=source&node=local"] {
            let response = client
                .get(url(path))
                .header("Cookie", &cookie)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::OK, "{path}");
        }
        // The proxy's name is accepted; the session works under it too.
        let named = client
            .get(url("/api/session"))
            .header("Host", "mash.example.ts.net")
            .header("Cookie", &cookie)
            .send()
            .await
            .unwrap();
        assert_eq!(named.status(), reqwest::StatusCode::OK);
        let named: serde_json::Value = named.json().await.unwrap();
        assert_eq!(named["signed_in"], true);
        let ended = client.post(url("/api/session/end")).send().await.unwrap();
        assert!(
            ended.headers()["set-cookie"]
                .to_str()
                .unwrap()
                .contains("Max-Age=0")
        );
        server.abort();
    }

    #[tokio::test]
    async fn dashboard_shared_chat_source_never_falls_back_to_local_node() {
        let (_directory, paths, _) = record_fixture();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = super::router(std::sync::Arc::new(super::Dash {
            console: crate::mcp::console::Console::new(crate::core::Paths::default()),
            paths,
            hosts: allowed_hosts(address),
            loopback: address.ip().is_loopback(),
            access: super::access::Access::new(None),
            shared_only: false,
            model_work: tokio::sync::Semaphore::new(0),
            services: super::services::Services::load(None).unwrap(),
            live: super::live::Live::new(None, None),
            questions: std::sync::Arc::new(tokio::sync::Semaphore::new(2)),
        }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        for path in ["/api/view?id=source", "/api/chat?id=fixture"] {
            let url = format!("http://{address}{path}&node=local");
            let local = client.get(&url).send().await.unwrap();
            assert_eq!(local.status(), reqwest::StatusCode::OK);
            if path.starts_with("/api/chat?") {
                // A page states the chat's length so a reader can jump to its end.
                let page: serde_json::Value = local.json().await.unwrap();
                let returned = page["messages"].as_array().unwrap().len() as u64;
                assert!(returned > 0);
                assert!(page["message_count"].as_u64().unwrap() >= returned);
            }
            let shared = client
                .get(format!("{url}&shared=true"))
                .send()
                .await
                .unwrap();
            assert_eq!(shared.status(), reqwest::StatusCode::BAD_REQUEST);
            assert!(
                shared
                    .text()
                    .await
                    .unwrap()
                    .contains("shared index is not configured")
            );
            let no_node = client
                .get(format!("http://{address}{path}&shared=true"))
                .send()
                .await
                .unwrap();
            assert_eq!(no_node.status(), reqwest::StatusCode::BAD_REQUEST);
            assert!(
                no_node
                    .text()
                    .await
                    .unwrap()
                    .contains("require a source node")
            );
        }
        server.abort();
    }

    #[tokio::test]
    async fn dashboard_records_expose_real_filtered_history_without_mutation() {
        let (_directory, paths, id) = record_fixture();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = super::router(std::sync::Arc::new(super::Dash {
            console: crate::mcp::console::Console::new(crate::core::Paths::default()),
            paths,
            hosts: allowed_hosts(address),
            loopback: address.ip().is_loopback(),
            access: super::access::Access::new(None),
            shared_only: false,
            model_work: tokio::sync::Semaphore::new(0),
            services: super::services::Services::load(None).unwrap(),
            live: super::live::Live::new(None, None),
            questions: std::sync::Arc::new(tokio::sync::Semaphore::new(2)),
        }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        let list = format!(
            "http://{address}/api/records?kind=fact&state=current&review=established&chat=fixture&q=8080"
        );
        let response = client.get(&list).send().await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let page: serde_json::Value = response.json().await.unwrap();
        assert_eq!(page["records"].as_array().unwrap().len(), 1);
        assert_eq!(page["records"][0]["id"], id);
        let detail = format!("http://{address}/api/record?id={id}");
        let record: serde_json::Value = client
            .get(&detail)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(record["current_revision"], 1);
        assert_eq!(
            record["revisions"][0]["evidence"][0]["message_id"],
            "source"
        );
        assert_eq!(record["revisions"][0]["evidence_current"], true);
        assert_eq!(
            client.post(&detail).send().await.unwrap().status(),
            reqwest::StatusCode::METHOD_NOT_ALLOWED
        );
        assert_eq!(
            client
                .get(&detail)
                .header("Host", "evil.invalid")
                .send()
                .await
                .unwrap()
                .status(),
            reqwest::StatusCode::MISDIRECTED_REQUEST
        );
        for path in [
            "/api/records?kind=unknown",
            "/api/records?state=unknown",
            "/api/records?node=remote&shared=true",
            "/api/record?id=missing&node=remote",
            "/api/record?id=missing&unknown=true",
        ] {
            assert_eq!(
                client
                    .get(format!("http://{address}{path}"))
                    .send()
                    .await
                    .unwrap()
                    .status(),
                reqwest::StatusCode::BAD_REQUEST,
                "{path}"
            );
        }
        let after: serde_json::Value = client
            .get(&detail)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(after, record);
        server.abort();
    }

    #[tokio::test]
    async fn dashboard_reports_status_read_only_and_compresses_responses() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.toml");
        std::fs::write(&config, "version = 1\nproviders = {}\n").unwrap();
        let database = directory.path().join("index.sqlite3");
        let mut storage = crate::storage::Storage::open_writer(&database).unwrap();
        storage.commit_writer().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = super::router(std::sync::Arc::new(super::Dash {
            console: crate::mcp::console::Console::new(crate::core::Paths::default()),
            paths: crate::core::Paths {
                config: Some(config),
                db: Some(database),
                models_dir: None,
            },
            hosts: allowed_hosts(address),
            loopback: address.ip().is_loopback(),
            access: super::access::Access::new(None),
            shared_only: false,
            model_work: tokio::sync::Semaphore::new(1),
            services: super::services::Services::load(None).unwrap(),
            live: super::live::Live::new(None, None),
            questions: std::sync::Arc::new(tokio::sync::Semaphore::new(2)),
        }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        // A client that leaves the body as sent shows the encoding on the wire.
        let client = reqwest::Client::builder().no_gzip().build().unwrap();
        let url = format!("http://{address}/api/status");
        // Settings and readiness are a read of what `mash status` reports.
        let response = client.get(&url).send().await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let status: serde_json::Value = response.json().await.unwrap();
        assert_eq!(status["configuration"]["loaded"], true);
        assert_eq!(status["configuration"]["providers"], serde_json::json!({}));
        assert_eq!(status["chats"], 0);
        assert_eq!(
            client.post(&url).send().await.unwrap().status(),
            reqwest::StatusCode::METHOD_NOT_ALLOWED
        );
        // The console lists every tool an agent sees and runs only readers.
        let tools: serde_json::Value = client
            .get(format!("http://{address}/api/console/tools"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(
            tools["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["name"] == "search" && tool["inputSchema"].is_object())
        );
        let call = |query: &'static str| {
            let request = client.get(format!("http://{address}/api/console/call?{query}"));
            async move { request.send().await.unwrap() }
        };
        let refused: serde_json::Value =
            call("name=forget&arguments=%7B%22chat_id%22%3A%22x%22%7D")
                .await
                .json()
                .await
                .unwrap();
        assert_eq!(refused["reply"]["error"]["kind"], "not-read-only");
        assert_eq!(
            call("name=help&arguments=%5B%5D").await.status(),
            reqwest::StatusCode::BAD_REQUEST
        );
        let help: serde_json::Value = call("name=help").await.json().await.unwrap();
        assert_eq!(help["failed"], false);
        let compressed = client
            .get(&url)
            .header("accept-encoding", "gzip")
            .send()
            .await
            .unwrap();
        assert_eq!(compressed.headers()["content-encoding"], "gzip");
        assert_eq!(compressed.headers()["cache-control"], "no-store");
        // The page is rechecked on every visit; a logo may be kept for a day.
        for (path, policy) in [
            ("/", "no-cache"),
            ("/mash-mark.svg", "public, max-age=86400"),
        ] {
            let response = client
                .get(format!("http://{address}{path}"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.headers()["cache-control"], policy, "{path}");
        }
        server.abort();
    }

    #[tokio::test]
    async fn dashboard_http_guards_host_methods_and_serves_real_catalogue() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.toml");
        std::fs::write(&config, "version = 1\nproviders = {}\n").unwrap();
        let database = directory.path().join("index.sqlite3");
        let mut storage = crate::storage::Storage::open_writer(&database).unwrap();
        storage.commit_writer().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = super::router(std::sync::Arc::new(super::Dash {
            console: crate::mcp::console::Console::new(crate::core::Paths::default()),
            paths: crate::core::Paths {
                config: Some(config),
                db: Some(database),
                models_dir: None,
            },
            hosts: allowed_hosts(address),
            loopback: address.ip().is_loopback(),
            access: super::access::Access::new(None),
            shared_only: false,
            model_work: tokio::sync::Semaphore::new(1),
            services: super::services::Services::load(None).unwrap(),
            live: super::live::Live::new(None, None),
            questions: std::sync::Arc::new(tokio::sync::Semaphore::new(2)),
        }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        let url = format!("http://{address}/api/chats");
        let response = client.get(&url).send().await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none()
        );
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert_eq!(response.headers()["cache-control"], "no-store");
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(body["local_node"], "local");
        assert_eq!(body["chats"], serde_json::json!([]));
        // Resolving the local node must not require shared configuration or a
        // catalogue request. Remote nodes must still fail rather than fall back.
        for node in ["", "local"] {
            let response = client
                .get(format!("http://{address}/api/view?id=missing&node={node}"))
                .send()
                .await
                .unwrap();
            if node.is_empty() {
                // An explicitly empty name is not the local identity.
                assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
            } else {
                // The local node resolves, and a missing message is not found.
                assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
                let body: serde_json::Value = response.json().await.unwrap();
                assert_eq!(body["error"]["kind"], "not-found");
            }
        }
        assert_eq!(
            client
                .get(format!("http://{address}/api/view?id=missing&node=remote"))
                .send()
                .await
                .unwrap()
                .status(),
            reqwest::StatusCode::BAD_REQUEST
        );
        assert_eq!(
            client
                .get(&url)
                .header("Host", "attacker.invalid")
                .send()
                .await
                .unwrap()
                .status(),
            reqwest::StatusCode::MISDIRECTED_REQUEST
        );
        assert_eq!(
            client.post(&url).send().await.unwrap().status(),
            reqwest::StatusCode::METHOD_NOT_ALLOWED
        );
        server.abort();
    }

    #[test]
    fn empty_queries_list_chats_and_queries_search_with_filters() {
        let params = || SearchParams {
            provider: "claude-code, codex".into(),
            machine: "xenia".into(),
            ..Default::default()
        };
        let listed = search_operation(params(), false).unwrap();
        let Operation::ListChats { input, shared, .. } = listed else {
            panic!("expected a chat listing")
        };
        assert!(!shared);
        assert_eq!(input.provider.len(), 2);
        assert_eq!(input.node, ["xenia"]);
        let query = || SearchParams {
            q: "backfill".into(),
            project: "/p".into(),
            days: Some(7),
            sort: crate::search_filter::Sort::Recent,
            ..Default::default()
        };
        let searched = search_operation(query(), false).unwrap();
        let Operation::Search {
            query: text,
            filters,
            shared,
            ..
        } = searched
        else {
            panic!("expected a search")
        };
        assert!(!shared);
        let query_text = text;
        // A machine with no history of its own reads only the shared index.
        assert!(matches!(
            search_operation(params(), true).unwrap(),
            Operation::ListChats { shared: true, .. }
        ));
        assert!(matches!(
            search_operation(query(), true).unwrap(),
            Operation::Search { shared: true, .. }
        ));
        assert_eq!(query_text, "backfill");
        assert_eq!(filters.project, ["/p"]);
        assert_eq!(filters.days, Some(7));
        assert_eq!(filters.sort, crate::search_filter::Sort::Recent);
        assert!(
            search_operation(
                SearchParams {
                    provider: "nope".into(),
                    ..Default::default()
                },
                false
            )
            .is_err()
        );
    }

    #[test]
    fn loopback_accepts_its_own_names_and_nothing_else() {
        let hosts = allowed_hosts("127.0.0.1:8790".parse().unwrap());
        for host in ["127.0.0.1:8790", "localhost:8790", "[::1]:8790"] {
            assert!(hosts.iter().any(|allowed| allowed == host), "{host}");
        }
        for host in ["evil.example:8790", "localhost:9999", "127.0.0.1"] {
            assert!(!hosts.iter().any(|allowed| allowed == host), "{host}");
        }
        let tailnet = allowed_hosts("100.64.0.20:8790".parse().unwrap());
        assert_eq!(tailnet, vec!["100.64.0.20:8790".to_owned()]);
    }

    #[test]
    fn known_assets_get_their_media_types() {
        assert_eq!(content_type("/index.html"), "text/html; charset=utf-8");
        assert_eq!(
            content_type("/assets/index.js"),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(content_type("/agents/claude.svg"), "image/svg+xml");
        assert_eq!(content_type("/unknown"), "application/octet-stream");
    }
    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn settings_are_changed_only_where_the_dashboard_is_private() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.toml");
        // Configured roots must be absolute on this host.
        std::fs::write(
            &config,
            format!(
                "version = 1\n\n[providers.codex]\nroots = ['{}']\n\n[index]\nall_history = true\n",
                directory.path().join("codex").display()
            ),
        )
        .unwrap();
        let paths = crate::core::Paths {
            config: Some(config.clone()),
            ..crate::core::Paths::default()
        };
        let mut addresses = Vec::new();
        for loopback in [true, false] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let app = super::router(std::sync::Arc::new(super::Dash {
                console: crate::mcp::console::Console::new(crate::core::Paths::default()),
                paths: paths.clone(),
                hosts: allowed_hosts(address),
                loopback,
                access: super::access::Access::new(None),
                shared_only: false,
                model_work: tokio::sync::Semaphore::new(0),
                services: super::services::Services::load(None).unwrap(),
                live: super::live::Live::new(None, None),
                questions: std::sync::Arc::new(tokio::sync::Semaphore::new(2)),
            }));
            tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            addresses.push(address);
        }
        let client = reqwest::Client::new();
        let private = format!("http://{}/api/services/settings", addresses[0]);
        let view: serde_json::Value = client
            .get(&private)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(view["agents"]["codex"]["indexed"], true);
        assert_eq!(view["editable"], true);
        let shown: serde_json::Value = client
            .get(format!("http://{}/api/services/settings", addresses[1]))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(shown["editable"], false);
        assert_eq!(view["since_days"], serde_json::Value::Null);
        let change = |revision: &serde_json::Value| {
            serde_json::json!({
                "revision": revision,
                "agents": {},
                "since_days": 30,
                "destination": "local",
                "memory_capture": false,
                "hide_credentials": { "output": true, "publication": true },
            })
        };
        // A dashboard other machines can reach, without the services opt-in, refuses.
        let open = format!("http://{}/api/services/settings", addresses[1]);
        let refused = client
            .post(&open)
            .json(&change(&view["revision"]))
            .send()
            .await
            .unwrap();
        assert_eq!(refused.status(), reqwest::StatusCode::FORBIDDEN);
        // Another site cannot submit the change through the reader's browser.
        let foreign = client
            .post(&private)
            .header("Origin", "http://evil.invalid")
            .json(&change(&view["revision"]))
            .send()
            .await
            .unwrap();
        assert_eq!(foreign.status(), reqwest::StatusCode::FORBIDDEN);
        assert!(
            std::fs::read_to_string(&config)
                .unwrap()
                .contains("all_history = true")
        );
        let saved: serde_json::Value = client
            .post(&private)
            .json(&change(&view["revision"]))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(saved["since_days"], 30);
        assert_eq!(saved["memory_capture"], false);
        // Unknown fields, such as a path from the browser, are not accepted.
        let mut extra = change(&saved["revision"]);
        extra["roots"] = serde_json::json!(["/etc"]);
        let rejected = client.post(&private).json(&extra).send().await.unwrap();
        assert_eq!(rejected.status(), reqwest::StatusCode::BAD_REQUEST);
    }
}
