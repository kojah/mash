//! HTTP transport, publication coordination and storage adapters for both layers.
//! Silver/gold snapshot schemas live in snapshot/layers.rs; gold builders live
//! in gold/shared.rs. Backends atomically persist and query their declared views.
use serde::{Deserialize, Serialize};

mod backend;
mod client;
pub(crate) mod protocol;
mod publication;
mod qdrant_index;
mod redis_index;
mod retrieval;
mod routed;
pub(crate) mod server;
mod snapshot;
mod snapshot_budget;

pub(crate) use client::{freshness, list_threads, view};
pub(crate) use publication::write_event;
pub(crate) use publication::{forget_routed, publish, publish_routed, routed_status, status};
pub(crate) use retrieval::search;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum PublicationState {
    Pending,
    Published,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Publication {
    state: PublicationState,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl Publication {
    pub(crate) fn is_pending(&self) -> bool {
        self.state == PublicationState::Pending
    }
    pub(crate) const fn published() -> Self {
        Self {
            state: PublicationState::Published,
            error: None,
        }
    }
    pub(crate) const fn pending() -> Self {
        Self {
            state: PublicationState::Pending,
            error: None,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Freshness {
    group_id: String,
    nodes: Vec<NodeFreshness>,
}

#[derive(Debug, Deserialize, Serialize)]
struct NodeFreshness {
    node_id: String,
    revision: i64,
    last_synced_at: Option<String>,
    last_updated_at: Option<String>,
    conversations: i64,
    messages: i64,
}

#[cfg(test)]
mod tests;
