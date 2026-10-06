//! History and retrieval records shared by the core without a storage dependency.
use crate::provider::Provider;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub(crate) struct Counts {
    pub(crate) conversations: u64,
    pub(crate) messages: u64,
    pub(crate) searchable_messages: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) searchable_user_messages: Option<u64>,
    pub(crate) embeddings: u64,
}

impl Counts {
    pub(crate) fn expected_dense(&self, build: &crate::gold::config::Build) -> Option<u64> {
        if !build.dense_enabled() {
            Some(0)
        } else if build.users_only() {
            self.searchable_user_messages
                .filter(|users| *users <= self.searchable_messages)
        } else {
            Some(self.searchable_messages)
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct SearchHit {
    pub(crate) id: String,
    pub(crate) conversation_id: String,
    pub(crate) provider: Provider,
    pub(crate) role: crate::role::Role,
    pub(crate) content: String,
    #[serde(default)]
    pub(crate) created_at: Option<i64>,
    #[serde(default)]
    pub(crate) metadata: crate::search_results::ThreadMetadata,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) copies: Vec<crate::search_results::MessageReference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) lexical_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) semantic_score: Option<f32>,
    pub(crate) fusion_score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) rerank_score: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) node_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Message {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) attachments: Vec<crate::attachments::Metadata>,
    pub(crate) id: String,
    pub(crate) ordinal: i64,
    pub(crate) role: crate::role::Role,
    pub(crate) content: String,
    pub(crate) created_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum MessageContent {
    #[default]
    Canonical,
    SearchProjection,
}

impl MessageContent {
    // serde's skip_serializing_if callback takes a reference.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub(crate) fn is_canonical(&self) -> bool {
        matches!(self, Self::Canonical)
    }
}
