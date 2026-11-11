//! Optional, authenticated service inspection and allowlisted supervisor control.
mod config;
pub(super) mod fleet_jobs;
mod manager;
mod model;
mod operations;
mod probe;
#[cfg(test)]
mod tests;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::{StreamExt, stream};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(super) struct Services {
    config: Option<config::Configuration>,
    token_hash: Option<blake3::Hash>,
    peers: Vec<Peer>,
    directory: Option<PathBuf>,
    client: reqwest::Client,
    fleet_path: Option<PathBuf>,
}

#[derive(Clone)]
struct Peer {
    id: String,
    url: String,
    token: Option<String>,
}

impl Services {
    pub(super) fn load(path: Option<&Path>) -> Result<Self, crate::AppError> {
        let config = path.map(config::Configuration::load).transpose()?;
        Self::configured(path, config)
    }

    pub(super) fn load_fleet(path: &Path, node: Option<&str>) -> Result<Self, crate::AppError> {
        let inventory =
            mash_fleet::inventory::Inventory::load(path).map_err(crate::AppError::configuration)?;
        let config = config::Configuration::from_fleet(&inventory, node)?;
        let mut services = Self::configured(Some(path), Some(config))?;
        services.fleet_path = Some(path.to_path_buf());
        Ok(services)
    }

    fn configured(
        path: Option<&Path>,
        config: Option<config::Configuration>,
    ) -> Result<Self, crate::AppError> {
        let token_hash = config
            .as_ref()
            .and_then(|c| c.token_env.as_deref())
            .map(|name| config::token(name).map(|s| blake3::hash(s.as_bytes())))
            .transpose()?;
        let mut peers = Vec::new();
        if let Some(config) = &config {
            for peer in &config.machines {
                peers.push(Peer {
                    id: peer.id.clone(),
                    url: peer.url.trim_end_matches('/').into(),
                    token: peer.token_env.as_deref().map(config::token).transpose()?,
                });
            }
        }
        let directory = path.map(|path| {
            let mut value = path.as_os_str().to_owned();
            value.push(".operations");
            PathBuf::from(value)
        });
        if let Some(directory) = &directory {
            std::fs::create_dir_all(directory).map_err(|_| {
                crate::AppError::configuration("cannot create service operation directory")
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
                    .map_err(|_| {
                        crate::AppError::configuration("cannot protect service operation directory")
                    })?;
            }
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            // Peers are on a private network where a live machine connects at
            // once; a longer wait only delays the report for one that is away.
            .connect_timeout(std::time::Duration::from_millis(1500))
            .timeout(std::time::Duration::from_secs(45))
            .build()
            .map_err(|_| crate::AppError::internal("build services HTTP client"))?;
        Ok(Self {
            config,
            token_hash,
            peers,
            directory,
            client,
            fleet_path: None,
        })
    }

    pub(super) fn authorized(&self, headers: &HeaderMap) -> bool {
        let Some(expected) = self.token_hash else {
            return true;
        };
        headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "))
            .is_some_and(|token| blake3::hash(token.as_bytes()) == expected)
    }

    fn local_id(&self) -> &str {
        self.config.as_ref().map_or("local", |c| c.node.as_str())
    }

    async fn remote<T: serde::de::DeserializeOwned>(
        &self,
        peer: &Peer,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T, String> {
        let mut request = self
            .client
            .get(format!("{}{path}", peer.url))
            .header("x-mash-service-forwarded", "1")
            .query(query);
        if let Some(token) = &peer.token {
            request = request.bearer_auth(token);
        }
        let response = request
            .send()
            .await
            .map_err(|_| "Machine could not be reached".to_owned())?;
        decode(response).await
    }
}

async fn decode<T: serde::de::DeserializeOwned>(
    mut response: reqwest::Response,
) -> Result<T, String> {
    if !response.status().is_success() {
        return Err(format!(
            "Service endpoint returned HTTP {}",
            response.status().as_u16()
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Could not read service response")?
    {
        if bytes.len() + chunk.len() > 2_097_152 {
            return Err("Service response exceeds its limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| {
        "Service response is incompatible; update the dashboard on that machine".into()
    })
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, axum::Json(crate::AppError::configuration(message))).into_response()
}

pub(super) async fn access(State(state): State<Arc<super::Dash>>) -> Response {
    axum::Json(serde_json::json!({"authentication_required":state.services.token_hash.is_some(), "local_node":state.services.local_id()})).into_response()
}

pub(super) async fn local(State(state): State<Arc<super::Dash>>) -> Response {
    axum::Json(probe::local(state.services.config.as_ref(), &state.paths).await).into_response()
}

pub(super) async fn fleet(State(state): State<Arc<super::Dash>>) -> Response {
    let local = probe::local(state.services.config.as_ref(), &state.paths);
    let remotes = stream::iter(state.services.peers.clone())
        .map(|peer| {
            let state = state.clone();
            async move {
                match state
                    .services
                    .remote::<model::Machine>(&peer, "/api/services/local", &[])
                    .await
                {
                    Ok(machine) if machine.id == peer.id => machine,
                    result => model::Machine {
                        id: peer.id.clone(),
                        platform: "unknown".into(),
                        checked_at: model::now(),
                        reachable: false,
                        services: vec![],
                        warnings: vec![match result {
                            Ok(_) => "Remote machine identity does not match configuration".into(),
                            Err(message) => message,
                        }],
                    },
                }
            }
        })
        .buffered(32)
        .collect::<Vec<_>>();
    let (local, mut machines) = tokio::join!(local, remotes);
    machines.insert(0, local);
    axum::Json(serde_json::json!({"machines":machines})).into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Selection {
    machine: String,
    service: String,
}

pub(super) async fn logs(
    State(state): State<Arc<super::Dash>>,
    headers: HeaderMap,
    Query(selection): Query<Selection>,
) -> Response {
    if selection.machine != state.services.local_id() {
        if headers.contains_key("x-mash-service-forwarded") {
            return error(StatusCode::CONFLICT, "Remote machine identity mismatch");
        }
        let Some(peer) = state
            .services
            .peers
            .iter()
            .find(|p| p.id == selection.machine)
        else {
            return error(StatusCode::NOT_FOUND, "Unknown machine");
        };
        return match state
            .services
            .remote::<Log>(
                peer,
                "/api/services/log",
                &[
                    ("machine", &selection.machine),
                    ("service", &selection.service),
                ],
            )
            .await
        {
            Ok(log) => axum::Json(log).into_response(),
            Err(message) => error(StatusCode::BAD_GATEWAY, &message),
        };
    }
    let Some(service) = state.services.config.as_ref().and_then(|c| {
        c.services
            .iter()
            .find(|s| format!("managed-{}", s.id) == selection.service)
    }) else {
        return error(
            StatusCode::NOT_FOUND,
            "No configured log source for this service",
        );
    };
    match manager::logs(service).await {
        Ok(text) => axum::Json(Log {
            text,
            checked_at: model::now(),
        })
        .into_response(),
        Err(message) => error(StatusCode::BAD_GATEWAY, &message),
    }
}

#[derive(Deserialize, serde::Serialize)]
struct Log {
    text: String,
    checked_at: u64,
}

pub(super) async fn action(
    State(state): State<Arc<super::Dash>>,
    headers: HeaderMap,
    input: Result<axum::Json<operations::ActionRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Ok(axum::Json(request)) = input else {
        return error(StatusCode::BAD_REQUEST, "Invalid service action request");
    };
    if !operations::valid_id(&request.request_id) {
        return error(StatusCode::BAD_REQUEST, "Invalid operation request ID");
    }
    if request.machine != state.services.local_id() {
        if headers.contains_key("x-mash-service-forwarded") {
            return error(StatusCode::CONFLICT, "Remote machine identity mismatch");
        }
        let Some(peer) = state
            .services
            .peers
            .iter()
            .find(|p| p.id == request.machine)
        else {
            return error(StatusCode::NOT_FOUND, "Unknown machine");
        };
        let mut call = state
            .services
            .client
            .post(format!("{}/api/services/action", peer.url))
            .header("x-mash-service-forwarded", "1");
        if let Some(token) = &peer.token {
            call = call.bearer_auth(token);
        }
        let result = call.json(&request).send().await;
        return match result {
            Ok(response) => match decode::<operations::Operation>(response).await {
                Ok(operation) if operation.request == request => {
                    axum::Json(operation).into_response()
                }
                _ => error(
                    StatusCode::BAD_GATEWAY,
                    "Remote action outcome could not be verified. Check operation history and status; do not retry blindly.",
                ),
            },
            Err(_) => error(
                StatusCode::BAD_GATEWAY,
                "Remote action outcome is unknown. Check operation history and status; do not retry blindly.",
            ),
        };
    }
    let Some(service) = state
        .services
        .config
        .as_ref()
        .and_then(|c| {
            c.services
                .iter()
                .find(|s| format!("managed-{}", s.id) == request.service)
        })
        .cloned()
    else {
        return error(
            StatusCode::NOT_FOUND,
            "This service is not a configured control target",
        );
    };
    let Some(directory) = state.services.directory.clone() else {
        return error(StatusCode::FORBIDDEN, "Service control is not configured");
    };
    // The operation survives a disconnected browser request. Its durable receipt
    // makes an interrupted server process distinguishable from a safe retry.
    match tokio::spawn(operations::execute(directory, service, request)).await {
        Ok(Ok(operation)) => axum::Json(operation).into_response(),
        Ok(Err(message)) => error(StatusCode::CONFLICT, &message),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Operation worker stopped; check history before acting again",
        ),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MachineSelection {
    machine: String,
}

/// One machine's agent setup. A peer answers only for itself, and the
/// inspection launches nothing: no agent command, MCP server or hook runs.
pub(super) async fn installation(
    State(state): State<Arc<super::Dash>>,
    headers: HeaderMap,
    Query(selection): Query<MachineSelection>,
) -> Response {
    use crate::core::installation::{Setup, setup};
    if selection.machine != state.services.local_id() {
        if headers.contains_key("x-mash-service-forwarded") {
            return error(StatusCode::CONFLICT, "Remote machine identity mismatch");
        }
        let Some(peer) = state
            .services
            .peers
            .iter()
            .find(|p| p.id == selection.machine)
        else {
            return error(StatusCode::NOT_FOUND, "Unknown machine");
        };
        return match state
            .services
            .remote::<Setup>(
                peer,
                "/api/services/installation",
                &[("machine", &selection.machine)],
            )
            .await
        {
            Ok(setup) => axum::Json(setup).into_response(),
            Err(message) => error(StatusCode::BAD_GATEWAY, &message),
        };
    }
    match tokio::task::spawn_blocking(setup).await {
        Ok(setup) => axum::Json(setup).into_response(),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not inspect agent setup",
        ),
    }
}

pub(super) async fn history(
    State(state): State<Arc<super::Dash>>,
    headers: HeaderMap,
    Query(selection): Query<MachineSelection>,
) -> Response {
    if selection.machine != state.services.local_id() {
        if headers.contains_key("x-mash-service-forwarded") {
            return error(StatusCode::CONFLICT, "Remote machine identity mismatch");
        }
        let Some(peer) = state
            .services
            .peers
            .iter()
            .find(|p| p.id == selection.machine)
        else {
            return error(StatusCode::NOT_FOUND, "Unknown machine");
        };
        return match state
            .services
            .remote::<Vec<operations::Operation>>(
                peer,
                "/api/services/operations",
                &[("machine", &selection.machine)],
            )
            .await
        {
            Ok(records) => axum::Json(records).into_response(),
            Err(message) => error(StatusCode::BAD_GATEWAY, &message),
        };
    }
    let Some(directory) = state.services.directory.clone() else {
        return axum::Json(Vec::<operations::Operation>::new()).into_response();
    };
    match tokio::task::spawn_blocking(move || operations::history(&directory)).await {
        Ok(Ok(records)) => axum::Json(records).into_response(),
        _ => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not read operation history",
        ),
    }
}
