//! Controller-only, allowlisted fleet operations behind existing service access.
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use mash_fleet::inventory::{Action, Inventory};
use mash_fleet::jobs::{self, Admission, Request};
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize, schemars::JsonSchema)]
struct AvailableAction {
    action: Action,
    targets: Vec<String>,
}

/// A target that is a shared server rather than a machine.
#[derive(Serialize, schemars::JsonSchema)]
struct ServerTarget {
    id: String,
    name: String,
    machine: String,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct Catalog {
    enabled: bool,
    revision: Option<String>,
    actions: Vec<AvailableAction>,
    /// Targets not listed here are machines.
    services: Vec<ServerTarget>,
}

async fn inventory(state: &super::super::Dash) -> Result<Option<Inventory>, String> {
    let Some(path) = state.services.fleet_path.clone() else {
        return Ok(None);
    };
    let inventory = tokio::task::spawn_blocking(move || Inventory::load(&path))
        .await
        .map_err(|_| "fleet inventory worker stopped")??;
    Ok((inventory.controller.node == state.services.local_id()).then_some(inventory))
}

pub(in crate::dash) async fn catalog(State(state): State<Arc<super::super::Dash>>) -> Response {
    match inventory(&state).await {
        Ok(Some(inventory)) => {
            let revision = match inventory.revision() {
                Ok(revision) => revision,
                Err(message) => return super::error(StatusCode::INTERNAL_SERVER_ERROR, &message),
            };
            let actions = inventory
                .controller
                .allow_actions
                .iter()
                .filter_map(|action| {
                    inventory
                        .targets(*action, &[])
                        .ok()
                        .map(|targets| AvailableAction {
                            action: *action,
                            targets,
                        })
                })
                .collect();
            let services = inventory
                .servers()
                .map(|(machine, service, _)| ServerTarget {
                    id: service.id.clone(),
                    name: service.name.clone(),
                    machine: machine.id.clone(),
                })
                .collect();
            axum::Json(Catalog {
                enabled: true,
                revision: Some(revision),
                actions,
                services,
            })
            .into_response()
        }
        Ok(None) => axum::Json(Catalog {
            enabled: false,
            revision: None,
            actions: vec![],
            services: vec![],
        })
        .into_response(),
        Err(message) => super::error(StatusCode::INTERNAL_SERVER_ERROR, &message),
    }
}

pub(in crate::dash) async fn history(State(state): State<Arc<super::super::Dash>>) -> Response {
    let inventory = match inventory(&state).await {
        Ok(Some(inventory)) => inventory,
        Ok(None) => {
            return super::error(
                StatusCode::FORBIDDEN,
                "Fleet jobs are only available on the configured controller",
            );
        }
        Err(message) => return super::error(StatusCode::INTERNAL_SERVER_ERROR, &message),
    };
    match tokio::task::spawn_blocking(move || jobs::history(&inventory)).await {
        Ok(Ok(jobs)) => axum::Json(jobs).into_response(),
        Ok(Err(message)) => super::error(StatusCode::INTERNAL_SERVER_ERROR, &message),
        Err(_) => super::error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Fleet history worker stopped",
        ),
    }
}

pub(in crate::dash) async fn submit(
    State(state): State<Arc<super::super::Dash>>,
    headers: HeaderMap,
    axum::Json(request): axum::Json<Request>,
) -> Response {
    if headers.contains_key("x-mash-service-forwarded") {
        return super::error(
            StatusCode::FORBIDDEN,
            "Fleet jobs cannot be forwarded between dashboards",
        );
    }
    let inventory = match inventory(&state).await {
        Ok(Some(inventory)) => inventory,
        Ok(None) => {
            return super::error(
                StatusCode::FORBIDDEN,
                "Fleet jobs are only available on the configured controller",
            );
        }
        Err(message) => return super::error(StatusCode::INTERNAL_SERVER_ERROR, &message),
    };
    // Admission itself survives browser cancellation, just like the admitted worker.
    let worker = tokio::spawn(async move {
        let admitted = tokio::task::spawn_blocking(move || jobs::admit(inventory, request))
            .await
            .map_err(|_| "Fleet admission worker stopped".to_owned())??;
        match admitted {
            Admission::Existing(job) => Ok::<jobs::Job, String>(job),
            Admission::Start(pending) => {
                let job = pending.job();
                tokio::spawn(async move {
                    if let Err(error) = pending.run().await {
                        tracing::warn!(%error, "fleet result not recorded; receipt remains unknown");
                    }
                });
                Ok(job)
            }
        }
    });
    match worker.await {
        Ok(Ok(job)) => (StatusCode::ACCEPTED, axum::Json(job)).into_response(),
        Ok(Err(message)) => super::error(StatusCode::CONFLICT, &message),
        Err(_) => super::error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Fleet admission interrupted; check job history before acting",
        ),
    }
}
