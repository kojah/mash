//! Native document records shared by publication and Qdrant projection.
use crate::provider::Provider;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConversationRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enrichment: Option<crate::silver::enrichment::Evidence>,
    pub id: String,
    pub provider: Provider,
    pub title: Option<String>,
    pub project: Option<String>,
    pub repository: Option<String>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MessageRecord {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<crate::attachments::Metadata>,
    pub id: String,
    pub conversation_id: String,
    pub ordinal: i64,
    #[serde(
        default,
        skip_serializing_if = "crate::message_context::MessageContext::is_linear"
    )]
    pub context: crate::message_context::MessageContext,
    pub role: crate::role::Role,
    pub content: String,
    pub projection: String,
    pub created_at: Option<i64>,
    pub generation: Option<String>,
    pub vector: Option<Vec<f64>>,
    #[serde(default, skip_serializing_if = "crate::tools::Activity::is_empty")]
    pub tools: crate::tools::Activity,
}
