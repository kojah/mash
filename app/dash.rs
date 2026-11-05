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
    use super::{allowed_hosts, content_type};

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
