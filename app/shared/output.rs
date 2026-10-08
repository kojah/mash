//! Compact status keeps coverage and freshness without model/configuration manifests.
use super::{Freshness, Publication, PublicationState};
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct PublicationSummary<'a> {
    state: PublicationState,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a str>,
}

impl Publication {
    pub(crate) fn summary(&self) -> PublicationSummary<'_> {
        PublicationSummary {
            state: self.state,
            error: self.error.as_deref(),
        }
    }
}

#[derive(Serialize)]
pub(crate) struct FreshnessSummary<'a> {
    group_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    unpublished_nodes: Option<&'a [String]>,
    nodes: Vec<Member<'a>>,
}

#[derive(Serialize)]
struct Member<'a> {
    node_id: &'a str,
    revision: i64,
    last_synced_at: Option<&'a str>,
    last_updated_at: Option<&'a str>,
    conversations: i64,
    messages: i64,
    lexical_current: bool,
    gold_current: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    artifacts: Option<&'a crate::gold::readiness::Artifacts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_silver: Option<&'a crate::silver::certificate::Certificate>,
}

impl Freshness {
    pub(crate) fn summary(&self) -> FreshnessSummary<'_> {
        FreshnessSummary {
            group_id: &self.group_id,
            unpublished_nodes: self.unpublished_nodes.as_deref(),
            nodes: self
                .nodes
                .iter()
                .map(|node| Member {
                    node_id: &node.node_id,
                    revision: node.revision,
                    last_synced_at: node.last_synced_at.as_deref(),
                    last_updated_at: node.last_updated_at.as_deref(),
                    conversations: node.conversations,
                    messages: node.messages,
                    lexical_current: node.lexical_current,
                    gold_current: node.gold_current,
                    artifacts: node.artifacts.as_ref(),
                    source_silver: node.source_silver.as_ref(),
                })
                .collect(),
        }
    }
}
