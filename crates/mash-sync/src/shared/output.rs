//! Compact status keeps coverage and freshness without model/configuration manifests.
use super::{Freshness, Publication, PublicationState};
use serde::Serialize;

#[derive(Serialize)]
pub struct PublicationSummary<'a> {
    pub state: PublicationState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<&'a str>,
}

impl Publication {
    pub fn summary(&self) -> PublicationSummary<'_> {
        PublicationSummary {
            state: self.state,
            error: self.error.as_deref(),
        }
    }
}

#[derive(Serialize)]
pub struct FreshnessSummary<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity: Option<&'a super::activity::Report>,
    pub group_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unpublished_nodes: Option<&'a [String]>,
    pub nodes: Vec<Member<'a>>,
}

#[derive(Serialize)]
pub struct Member<'a> {
    pub node_id: &'a str,
    pub revision: i64,
    pub last_synced_at: Option<&'a str>,
    pub last_updated_at: Option<&'a str>,
    pub chats: i64,
    pub messages: i64,
    pub lexical_current: bool,
    pub views_current: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<&'a crate::views::readiness::Artifacts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_silver: Option<&'a crate::silver::certificate::Certificate>,
}

impl Freshness {
    pub fn summary(&self) -> FreshnessSummary<'_> {
        FreshnessSummary {
            activity: self.activity.as_ref(),
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
                    chats: node.chats,
                    messages: node.messages,
                    lexical_current: node.lexical_current,
                    views_current: node.views_current,
                    artifacts: node.artifacts.as_ref(),
                    source_silver: node.source_silver.as_ref(),
                })
                .collect(),
        }
    }
}
