//! HTTP transport, publication coordination and shared storage. The server keeps
//! silver in SQLite (`silver_store`) and derived, replayable views in Qdrant
//! (`views::qdrant`); views builders live in `views/shared.rs`.
use serde::{Deserialize, Serialize};

// Full-history upload, validation and staging share one bounded request budget.
// Keep interactive client deadlines separate so ordinary search falls back promptly.
const BULK_TIMEOUT: std::time::Duration = std::time::Duration::from_mins(30);

use crate::views::retrieval::EXACT_SEARCH_BUDGET_ERROR;

pub(crate) mod activity;
pub(crate) mod attachments;
mod client;
mod coverage;
mod exact;
mod lexical_manifest;
mod output;
pub(crate) mod protocol;
pub(crate) mod publication;
#[cfg(test)]
use crate::views::qdrant as qdrant_index;
use crate::views::qdrant::Settings as GroupView;
mod retrieval;
mod reuse;
pub(crate) mod server;
use crate::silver::revision as silver_revision;
pub(crate) mod silver_store;
mod sync;
mod validation;
mod views_manifest;

#[cfg(test)]
pub(crate) use client::query_with_token;
pub(crate) use client::{
    catalog, chat_page, fingerprints, freshness, get_record, list_chats, list_records, query, view,
};
pub(crate) use exact::search as search_exact;
pub(crate) use output::{FreshnessSummary, PublicationSummary};
pub(crate) use publication::Inference;
pub(crate) use publication::write_event;
pub(crate) use publication::{publish, status};
pub(crate) use retrieval::{SearchOutcome, require_dense, search, search_lexical};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PublicationState {
    Pending,
    Published,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
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

/// Duplicate-history report across the group, keyed by canonical chat
/// fingerprint. Fingerprints are content hashes; no plaintext is exposed.
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub(crate) struct FingerprintReport {
    /// Chat identifiers that appear with the same fingerprint in more
    /// than one node.
    pub(crate) duplicated_chats: u64,
    /// Extra copies beyond the first, per duplicated chat.
    pub(crate) redundant_copies: u64,
    /// Messages carried by those redundant copies.
    pub(crate) redundant_messages: u64,
    /// Approximate duplicated vector storage in Qdrant (f32 per message at the
    /// group's embedding width). Text and index overhead are excluded.
    pub(crate) duplicate_vector_bytes: u64,
    pub(crate) nodes: Vec<NodeFingerprintSummary>,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub(crate) struct NodeFingerprintSummary {
    pub(crate) node_id: String,
    pub(crate) chats: u64,
    pub(crate) duplicated: u64,
}

/// Baseline coverage for one node, read from the transfer catalog.
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub(crate) struct BaselineCoverage {
    pub(crate) id: String,
    pub(crate) state: String,
    pub(crate) covered: u64,
    pub(crate) total: u64,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub(crate) struct Freshness {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    activity: Option<activity::Report>,
    /// Most recent progressive baseline for the node, if one was started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) baseline: Option<BaselineCoverage>,
    group_id: String,
    /// Configured group members with no retained publication. None means the
    /// responding service did not supply its configured membership scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unpublished_nodes: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) group_views: Option<Box<GroupView>>,
    pub(crate) nodes: Vec<NodeFreshness>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub(crate) struct NodeFreshness {
    /// None means the service did not report artifact publication states.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) artifacts: Option<crate::views::readiness::Artifacts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lexical_manifest: Option<lexical_manifest::Manifest>,
    #[serde(default)]
    pub(crate) lexical_current: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    views_manifest: Option<views_manifest::Manifest>,
    #[serde(default)]
    pub(crate) views_current: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) silver_revision: Option<silver_revision::State>,
    /// Retained source-node certificate, not the server-owned canonical revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source_silver: Option<crate::silver::certificate::Certificate>,
    pub(crate) node_id: String,
    /// The local database that owns this node's publications.
    pub(crate) instance: String,
    pub(crate) revision: i64,
    pub(crate) last_synced_at: Option<String>,
    pub(crate) last_updated_at: Option<String>,
    pub(crate) chats: i64,
    pub(crate) messages: i64,
}

#[cfg(test)]
mod tests;
