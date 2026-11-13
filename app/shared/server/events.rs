//! Tells listening dashboards which chats a node's publication changed, as
//! Server-Sent Events.
//!
//! Every publication commits silver through one place, which announces the
//! chats it supplied or removed. A listener hears the announcements for its
//! own group and rereads what it shows through the ordinary routes: an event
//! carries node and chat IDs, never text. Nothing is stored. A listener that
//! was away, or that fell behind and was disconnected, rereads everything
//! when it returns.
use super::{ApiError, App, Identity};
use crate::AppError;
use crate::shared::protocol::NodeChange;
use axum::extract::{Extension, State};
use axum::http::{StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{Semaphore, broadcast, watch};

/// Dashboards that may listen at once, across every group.
const MAX_LISTENERS: usize = 32;
/// Chats one event names before it says "everything" instead.
const MAX_CHATS: usize = 100;
/// Announcements a slow listener may leave unread before it is disconnected.
const BUFFER: usize = 64;

pub(crate) struct Hub {
    /// Each change with the group it belongs to.
    changes: broadcast::Sender<(String, NodeChange)>,
    listeners: Arc<Semaphore>,
    stopping: watch::Sender<bool>,
}

impl Default for Hub {
    fn default() -> Self {
        Self {
            changes: broadcast::Sender::new(BUFFER),
            listeners: Arc::new(Semaphore::new(MAX_LISTENERS)),
            stopping: watch::Sender::new(false),
        }
    }
}

impl Hub {
    /// Says that `identity` committed `revision`, which changed `chats` and,
    /// when `records` is set, perhaps its memories.
    pub(crate) fn announce(
        &self,
        identity: &Identity,
        revision: i64,
        chats: impl Iterator<Item = String>,
        records: bool,
    ) {
        let mut chats: Vec<String> = chats.collect();
        if chats.is_empty() && !records {
            return;
        }
        let all = chats.len() > MAX_CHATS;
        if all {
            chats.clear();
        }
        // Nobody listening is not a failure.
        let _ = self.changes.send((
            identity.group_id.clone(),
            NodeChange {
                node: identity.node_id.clone(),
                revision,
                chats,
                all,
                records,
            },
        ));
    }

    /// Ends every stream, so a stopping server is not kept open by them.
    pub(crate) fn stop(&self) {
        self.stopping.send_replace(true);
    }
}

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
pub(super) async fn listen(State(app): State<App>, Extension(id): Extension<Identity>) -> Response {
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
