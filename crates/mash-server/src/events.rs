//! The Server-Sent Events route over the store's change hub.
use crate::{ApiError, App, Identity};
use axum::extract::{Extension, State};
use axum::http::{StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use mash_app::shared::protocol::NodeChange;
use mash_model::AppError;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{broadcast, watch};

/// The next change in `group`, or none when the stream should end.
async fn next(
    group: &str,
    changes: &mut broadcast::Receiver<(String, NodeChange)>,
    stopping: &mut watch::Receiver<bool>,
) -> Option<NodeChange> {
    loop {
        tokio::select! {
            _ = stopping.wait_for(|stopping| *stopping) => return None,
            announced = changes.recv() => match announced {
                Ok((scope, change)) if scope == group => return Some(change),
                Ok(_) => {}
                // What was missed is unknown; the listener reconnects and rereads.
                Err(RecvError::Lagged(_) | RecvError::Closed) => return None,
            },
        }
    }
}

/// A stream of `change` events for the caller's group, with comment lines in
/// between so that proxies keep an idle connection open.
pub(crate) async fn listen(State(app): State<App>, Extension(id): Extension<Identity>) -> Response {
    let hub = app.store.events();
    let Ok(permit) = hub.listeners.clone().try_acquire_owned() else {
        let mut response = ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            AppError::shared("too many dashboards are listening for changes"),
        )
        .into_response();
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, header::HeaderValue::from_static("30"));
        return response;
    };
    let stream = futures_util::stream::unfold(
        (
            hub.changes.subscribe(),
            hub.stopping.subscribe(),
            permit,
            id.group_id,
        ),
        |(mut changes, mut stopping, permit, group)| async move {
            let change = next(&group, &mut changes, &mut stopping).await?;
            let event = Event::default().event("change").json_data(&change).ok()?;
            Some((
                Ok::<_, std::convert::Infallible>(event),
                (changes, stopping, permit, group),
            ))
        },
    );
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}
