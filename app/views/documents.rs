//! Native document records shared by publication and Qdrant projection.
use crate::provider::Provider;
use serde::{Deserialize, Serialize};

/// Actual model input after source-specific context/title preparation. Identity
/// selects a cached point; the fingerprint determines whether its vector fits.
pub struct EmbeddingInput<'a> {
    pub provider: crate::provider::Provider,
    pub anchor: &'a str,
    pub text: &'a str,
}

impl EmbeddingInput<'_> {
    pub fn fingerprint(&self, generation: &str, dimensions: usize) -> String {
        let mut hash = blake3::Hasher::new();
        hash.update(b"mash-embedding-input\0");
        for value in [generation.as_bytes(), self.text.as_bytes()] {
            hash.update(&(value.len() as u64).to_le_bytes());
            hash.update(value);
        }
        hash.update(&(dimensions as u64).to_le_bytes());
        hash.finalize().to_hex().to_string()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChatRecord {
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
pub struct MessageRecord {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<crate::attachments::Metadata>,
    pub id: String,
    pub chat_id: String,
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

/// Search dependencies only. Canonical content, attachments and tool activity
/// stay in silver and in the publication protocol, never in replay scratch data.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SearchMessage {
    pub id: String,
    pub chat_id: String,
    pub ordinal: i64,
    pub context: crate::message_context::MessageContext,
    pub role: crate::role::Role,
    pub projection: String,
    pub created_at: Option<i64>,
    pub generation: Option<String>,
    pub vector: Option<Vec<f64>>,
}

impl From<MessageRecord> for SearchMessage {
    fn from(message: MessageRecord) -> Self {
        Self {
            id: message.id,
            chat_id: message.chat_id,
            ordinal: message.ordinal,
            context: message.context,
            role: message.role,
            projection: message.projection,
            created_at: message.created_at,
            generation: message.generation,
            vector: message.vector,
        }
    }
}
