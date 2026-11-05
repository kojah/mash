//! `mash dash`: a local, read-only web dashboard. It serves the front end built
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

mod assets {
    include!(concat!(env!("OUT_DIR"), "/dash_assets.rs"));
}

struct Dash {
    paths: Paths,
    hosts: Vec<String>,
}

pub(crate) async fn run(paths: Paths, listen: SocketAddr) -> Result<(), AppError> {
    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .map_err(|_| AppError::configuration("could not bind the dashboard address"))?;
    let address = listener
        .local_addr()
        .map_err(|_| AppError::internal("get dashboard address"))?;
    let state = Arc::new(Dash {
        paths,
        hosts: allowed_hosts(address),
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
        .route("/api/chats", axum::routing::get(list_chats))
        .route("/api/search", axum::routing::get(search))
        .route("/api/view", axum::routing::get(view))
        .fallback(axum::routing::get(asset))
        .layer(axum::middleware::from_fn_with_state(state.clone(), guard))
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
    if request.method() != axum::http::Method::GET && request.method() != axum::http::Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
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

async fn list_chats(State(state): State<Arc<Dash>>) -> Response {
    let request = Request {
        paths: state.paths.clone(),
        operation: Operation::ListChats {
            input: Box::new(crate::chat_catalog::Request::default()),
            local: false,
            shared: false,
            diagnostics: false,
        },
    };
    json_result(core::execute(request).await)
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
        limit: None,
        offset: 0,
        unit: None,
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
            core::execute(Request {
                paths: state.paths.clone(),
                operation,
            })
            .await,
        ),
        Err(error) => json_result(Err(error)),
    }
}

#[derive(serde::Deserialize)]
struct ViewParams {
    id: String,
    #[serde(default)]
    context: Option<u32>,
}

/// Messages on each side of the anchor; chunked chat reads are tracked in mash-twg6.
const MAX_VIEW_CONTEXT: u32 = 500;

async fn view(
    State(state): State<Arc<Dash>>,
    axum::extract::Query(params): axum::extract::Query<ViewParams>,
) -> Response {
    let operation = Operation::View {
        id: params.id,
        context: params.context.unwrap_or(50).min(MAX_VIEW_CONTEXT),
        shared: None,
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
