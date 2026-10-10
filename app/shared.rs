//! HTTP transport, publication coordination and shared storage. The server keeps
//! silver in SQLite (`silver_store`) and derived, replayable views in Qdrant
//! (`qdrant_index`); views builders live in `views/shared.rs`.
use serde::{Deserialize, Serialize};

// Full-history upload, validation and staging share one bounded request budget.
// Keep interactive client deadlines separate so ordinary search falls back promptly.
const BULK_TIMEOUT: std::time::Duration = std::time::Duration::from_mins(30);

pub(crate) mod attachments;
mod client;
mod coverage;
mod exact;
mod lexical_manifest;
mod output;
pub(crate) mod protocol;
mod publication;
mod qdrant_index;
mod retrieval;
mod reuse;
pub(crate) mod server;
mod silver_revision;
mod silver_store;
mod validation;
mod views_manifest;

pub(crate) use client::{freshness, list_chats, view};
pub(crate) use exact::search as search_exact;
pub(crate) use output::{FreshnessSummary, PublicationSummary};
pub(crate) use publication::Inference;
#[cfg(test)]
pub(crate) use publication::write_event;
pub(crate) use publication::{publish, status};
pub(crate) use retrieval::{SearchOutcome, require_dense, search};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PublicationState {
    Pending,
    Published,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Publication {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preparation: Option<publication::SnapshotWork>,
    state: PublicationState,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl Publication {
    pub(crate) fn attachment_failure(&mut self, error: crate::AppError) {
        self.state = PublicationState::Pending;
        self.error = Some(error.error.message);
    }
    pub(crate) fn is_pending(&self) -> bool {
        self.state == PublicationState::Pending
    }
    pub(crate) const fn published() -> Self {
        Self {
            preparation: None,
            state: PublicationState::Published,
            error: None,
        }
    }
    pub(crate) const fn pending() -> Self {
        Self {
            preparation: None,
            state: PublicationState::Pending,
            error: None,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Freshness {
    group_id: String,
    /// Configured group members with no retained publication. None means the
    /// responding service did not supply its configured membership scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unpublished_nodes: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    group_views: Option<Box<GroupView>>,
    nodes: Vec<NodeFreshness>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GroupView {
    version: String,
    build: crate::views::config::Build,
    generation: String,
    dimensions: usize,
    #[serde(default)]
    vector_search: qdrant_index::VectorIndex,
}

#[derive(Debug, Deserialize, Serialize)]
struct NodeFreshness {
    /// None means the service did not report artifact publication states.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    artifacts: Option<crate::views::readiness::Artifacts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lexical_manifest: Option<lexical_manifest::Manifest>,
    #[serde(default)]
    lexical_current: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    views_manifest: Option<views_manifest::Manifest>,
    #[serde(default)]
    views_current: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    silver_revision: Option<silver_revision::State>,
    /// Retained source-node certificate, not the server-owned canonical revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_silver: Option<crate::silver::certificate::Certificate>,
    node_id: String,
    /// The local database that owns this node's publications.
    instance: String,
    revision: i64,
    last_synced_at: Option<String>,
    last_updated_at: Option<String>,
    conversations: i64,
    messages: i64,
}

#[cfg(test)]
mod tests;
