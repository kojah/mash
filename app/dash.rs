//! `mash dash`: read-only history with optional authenticated service control.
//! It serves the front end built
//! from `dashboard/` (embedded at compile time) and a small JSON API over the
//! same typed core as the CLI and MCP. It binds to loopback by default and
//! answers only requests addressed to the bound host, so other websites cannot
//! reach it through DNS rebinding; it sends no CORS headers.
use crate::AppError;
use crate::core::{self, Operation, Paths, Request};
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use std::net::SocketAddr;
use std::sync::Arc;

mod services;

mod assets {
    include!(concat!(env!("OUT_DIR"), "/dash_assets.rs"));
}

struct Dash {
    paths: Paths,
    hosts: Vec<String>,
    model_work: tokio::sync::Semaphore,
    services: services::Services,
}

pub(crate) async fn run(
    paths: Paths,
    listen: SocketAddr,
    services_config: Option<std::path::PathBuf>,
    fleet_config: Option<std::path::PathBuf>,
    fleet_node: Option<String>,
) -> Result<(), AppError> {
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
    let state = Arc::new(Dash {
        paths,
        hosts: allowed_hosts(address),
        model_work: tokio::sync::Semaphore::new(1),
        services,
    });
    crate::write_json(
        &mut std::io::stderr(),
        &serde_json::json!({ "dashboard": format!("http://{address}/") }),
    )?;
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|_| AppError::internal("dashboard server failed"))
}

fn router(state: Arc<Dash>) -> Router {
    Router::new()
        .route("/api/health", axum::routing::get(health))
        .route("/api/services/access", axum::routing::get(services::access))
        .route("/api/services", axum::routing::get(services::fleet))
        .route("/api/services/local", axum::routing::get(services::local))
        .route(
            "/api/services/fleet",
            axum::routing::get(services::fleet_jobs::catalog),
        )
        .route(
            "/api/services/fleet/jobs",
            axum::routing::get(services::fleet_jobs::history).post(services::fleet_jobs::submit),
        )
        .route("/api/services/log", axum::routing::get(services::logs))
        .route(
            "/api/services/installation",
            axum::routing::get(services::installation),
        )
        .route(
            "/api/services/operations",
            axum::routing::get(services::history),
        )
        .route(
            "/api/services/action",
            axum::routing::post(services::action),
        )
        .route("/api/status", axum::routing::get(status))
        .route("/api/chats", axum::routing::get(list_chats))
        .route("/api/search", axum::routing::get(search))
        .route("/api/view", axum::routing::get(view))
        .route("/api/chat", axum::routing::get(chat))
        .route("/api/records", axum::routing::get(list_records))
        .route("/api/record", axum::routing::get(record))
        .fallback(axum::routing::get(asset))
        .layer(axum::middleware::from_fn_with_state(state.clone(), guard))
        // The chat catalogue is over a megabyte of JSON for a few thousand
        // chats; a phone on a slow link should not have to carry all of it.
        .layer(
            tower_http::compression::CompressionLayer::new()
                .quality(tower_http::CompressionLevel::Fastest),
        )
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
    let service_path = request.uri().path() == "/api/services"
        || request.uri().path().starts_with("/api/services/");
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
    let service_action = matches!(
        request.uri().path(),
        "/api/services/action" | "/api/services/fleet/jobs"
    ) && request.method() == axum::http::Method::POST;
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
    let api = request.uri().path().starts_with("/api/");
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    if api {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
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

async fn health() -> Response {
    axum::Json(serde_json::json!({ "build": env!("MASH_BUILD_REVISION") })).into_response()
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

async fn list_chats(State(state): State<Arc<Dash>>) -> Response {
    let request = Request {
        paths: state.paths.clone(),
        operation: Operation::DashboardCatalog,
    };
    json_result(core::execute(request).await)
}

#[derive(serde::Deserialize, Default)]
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
        shared: params.shared,
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

#[derive(serde::Deserialize)]
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

#[derive(serde::Deserialize, Default)]
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
fn search_operation(params: SearchParams) -> Result<Operation, AppError> {
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
            shared: false,
            diagnostics: false,
        });
    }
    let filters = crate::search_filter::Selection {
        provider,
        node: split(&params.machine),
        project: split(&params.project),
        days: params.days,
        ..Default::default()
    };
    Ok(Operation::Search {
        shared_vector_search: None,
        query: params.q,
        intent: None,
        limit: Some(20),
        offset: params.offset,
        unit: Some(crate::search_results::SearchUnit::Chats),
        fusion: crate::views::config::FusionOverrides::default(),
        ranking: None,
        diagnostics: false,
        filters: Box::new(filters),
        shared: false,
        local: false,
    })
}

async fn search(
    State(state): State<Arc<Dash>>,
    axum::extract::Query(params): axum::extract::Query<SearchParams>,
) -> Response {
    match search_operation(params) {
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

#[derive(serde::Deserialize)]
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

#[derive(serde::Deserialize)]
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

fn json_result(result: Result<core::Response, AppError>) -> Response {
    match result {
        Ok(response) => axum::Json(response).into_response(),
        Err(error) => {
            let status = if error.error.retryable {
                StatusCode::SERVICE_UNAVAILABLE
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
                "<!doctype html><title>Mash</title><p>The dashboard has not been built. Run <code>pnpm install &amp;&amp; pnpm build</code> in <code>dashboard/</code>, then rebuild mash.</p>",
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
    async fn dashboard_shared_chat_source_never_falls_back_to_local_node() {
        let (_directory, paths, _) = record_fixture();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = super::router(std::sync::Arc::new(super::Dash {
            paths,
            hosts: allowed_hosts(address),
            model_work: tokio::sync::Semaphore::new(0),
            services: super::services::Services::load(None).unwrap(),
        }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        for path in ["/api/view?id=source", "/api/chat?id=fixture"] {
            let url = format!("http://{address}{path}&node=local");
            let local = client.get(&url).send().await.unwrap();
            assert_eq!(local.status(), reqwest::StatusCode::OK);
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
            paths,
            hosts: allowed_hosts(address),
            model_work: tokio::sync::Semaphore::new(0),
            services: super::services::Services::load(None).unwrap(),
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
            paths: crate::core::Paths {
                config: Some(config),
                db: Some(database),
                models_dir: None,
            },
            hosts: allowed_hosts(address),
            model_work: tokio::sync::Semaphore::new(1),
            services: super::services::Services::load(None).unwrap(),
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
        let compressed = client
            .get(&url)
            .header("accept-encoding", "gzip")
            .send()
            .await
            .unwrap();
        assert_eq!(compressed.headers()["content-encoding"], "gzip");
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
            paths: crate::core::Paths {
                config: Some(config),
                db: Some(database),
                models_dir: None,
            },
            hosts: allowed_hosts(address),
            model_work: tokio::sync::Semaphore::new(1),
            services: super::services::Services::load(None).unwrap(),
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
                assert_eq!(response.status(), reqwest::StatusCode::OK);
                let body: serde_json::Value = response.json().await.unwrap();
                assert_eq!(body["messages"], serde_json::json!([]));
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
        let listed = search_operation(SearchParams {
            provider: "claude-code, codex".into(),
            machine: "xenia".into(),
            ..Default::default()
        })
        .unwrap();
        let Operation::ListChats { input, .. } = listed else {
            panic!("expected a chat listing")
        };
        assert_eq!(input.provider.len(), 2);
        assert_eq!(input.node, ["xenia"]);
        let searched = search_operation(SearchParams {
            q: "backfill".into(),
            project: "/p".into(),
            days: Some(7),
            ..Default::default()
        })
        .unwrap();
        let Operation::Search { query, filters, .. } = searched else {
            panic!("expected a search")
        };
        assert_eq!(query, "backfill");
        assert_eq!(filters.project, ["/p"]);
        assert_eq!(filters.days, Some(7));
        assert!(
            search_operation(SearchParams {
                provider: "nope".into(),
                ..Default::default()
            })
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
        let tailnet = allowed_hosts("100.97.52.20:8790".parse().unwrap());
        assert_eq!(tailnet, vec!["100.97.52.20:8790".to_owned()]);
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
}
