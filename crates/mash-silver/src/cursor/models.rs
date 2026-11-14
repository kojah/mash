//! Known JSON storage and Vercel message schemas; tool payloads stay opaque.
use crate::role::Role;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub agent_id: String,
    pub latest_root_blob_id: String,
    pub name: String,
    pub created_at: i64,
    pub subagent_info: Option<Subagent>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Subagent {
    pub parent_agent_id: String,
    pub root_parent_agent_id: String,
    pub tool_call_id: String,
    pub type_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub role: Role,
    pub content: Content,
    pub provider_options: Option<ProviderOptions>,
}

#[derive(Deserialize)]
pub struct ProviderOptions {
    pub cursor: Option<CursorOptions>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorOptions {
    #[serde(default)]
    pub is_summary: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Parts(Vec<Part>),
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Part {
    Text {
        text: String,
    },
    Reasoning {
        text: String,
    },
    RedactedReasoning,
    #[serde(rename_all = "camelCase")]
    ToolCall {
        tool_name: String,
        args: serde_json::Value,
    },
    #[serde(rename_all = "camelCase")]
    ToolResult {
        tool_name: String,
        #[serde(default)]
        result: serde_json::Value,
    },
    File {
        filename: Option<String>,
    },
    Image,
    #[serde(other)]
    Unknown,
}

impl Message {
    pub fn summary(&self) -> bool {
        self.provider_options
            .as_ref()
            .and_then(|p| p.cursor.as_ref())
            .is_some_and(|c| c.is_summary)
    }
}
