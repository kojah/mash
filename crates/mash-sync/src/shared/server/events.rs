//! Tells listening dashboards which chats a node's publication changed, as
//! Server-Sent Events.
//!
//! Every publication commits silver through one place, which announces the
//! chats it supplied or removed. A listener hears the announcements for its
//! own group and rereads what it shows through the ordinary routes: an event
//! carries node and chat IDs, never text. Nothing is stored. A listener that
//! was away, or that fell behind and was disconnected, rereads everything
//! when it returns.
use super::Identity;
use crate::shared::protocol::NodeChange;
use std::sync::Arc;
use tokio::sync::{Semaphore, broadcast, watch};

/// Dashboards that may listen at once, across every group.
const MAX_LISTENERS: usize = 32;
/// Chats one event names before it says "everything" instead.
const MAX_CHATS: usize = 100;
/// Announcements a slow listener may leave unread before it is disconnected.
const BUFFER: usize = 64;

pub struct Hub {
    /// Each change with the group it belongs to.
    pub changes: broadcast::Sender<(String, NodeChange)>,
    pub listeners: Arc<Semaphore>,
    pub stopping: watch::Sender<bool>,
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
    pub fn announce(
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
    pub fn stop(&self) {
        self.stopping.send_replace(true);
    }
}
