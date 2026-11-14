//! Native desktop v18 root and visible bubble schemas. Model input is not prose.
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Root {
    #[serde(rename = "_v")]
    pub version: u32,
    pub composer_id: String,
    pub full_conversation_headers_only: Vec<Header>,
    pub name: Option<String>,
    pub created_at: Option<i64>,
    pub last_updated_at: Option<i64>,
    pub workspace_identifier: Option<Workspace>,
    #[serde(default)]
    pub tracked_git_repos: Vec<Repository>,
    pub conversation_state: Option<String>,
    pub subagent_info: Option<Subagent>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Subagent {
    pub parent_composer_id: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Header {
    pub bubble_id: String,
    #[serde(rename = "type")]
    pub kind: Kind,
    pub created_at: Option<String>,
    #[serde(default)]
    pub is_display_only: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(from = "i32", into = "i32")]
pub enum Kind {
    Unspecified,
    Human,
    Assistant,
    Other(i32),
}

impl From<i32> for Kind {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Unspecified,
            1 => Self::Human,
            2 => Self::Assistant,
            value => Self::Other(value),
        }
    }
}
impl From<Kind> for i32 {
    fn from(value: Kind) -> Self {
        match value {
            Kind::Unspecified => 0,
            Kind::Human => 1,
            Kind::Assistant => 2,
            Kind::Other(value) => value,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bubble {
    #[serde(rename = "bubbleId")]
    pub id: String,
    #[serde(rename = "type")]
    pub kind: Kind,
    #[serde(default)]
    pub text: String,
    pub created_at: Option<String>,
    pub started_at_ms: Option<i64>,
    pub completed_at_ms: Option<i64>,
    #[serde(default)]
    pub is_display_only: bool,
    #[serde(default)]
    pub is_simulated_msg: bool,
    pub simulated_msg_reason: Option<i32>,
    pub simulated_message_metadata: Option<Simulated>,
    #[serde(default)]
    pub is_placeholder: bool,
    pub service_status_update: Option<ProtoRecord<Status>>,
    pub event_notification: Option<Notification>,
    pub capability_type: Option<i32>,
    pub thinking: Option<Thinking>,
    // Provider-owned tool arguments/results are intentionally opaque.
    pub tool_former_data: Option<Tool>,
    pub sent_by_agent_id: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
pub enum ProtoRecord<T> {
    Json(String),
    Object(T),
}
#[derive(Deserialize, Serialize)]
pub struct Status {
    #[serde(default)]
    pub message: String,
}
#[derive(Deserialize, Serialize)]
pub struct Tool {
    #[serde(rename = "tool")]
    pub id: i32,
    pub params: Option<serde_json::Value>,
    pub result: Option<serde_json::Value>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Simulated {
    #[serde(default)]
    pub suppress_ui_display: bool,
}
#[derive(Deserialize, Serialize)]
pub struct Thinking {
    pub text: String,
}
#[derive(Deserialize, Serialize)]
pub struct Notification {
    pub headline: String,
    pub detail: Option<String>,
}
#[derive(Deserialize, Serialize)]
pub struct Workspace {
    pub uri: Option<Uri>,
}
#[derive(Deserialize, Serialize)]
pub struct Uri {
    pub scheme: String,
    pub path: String,
    pub authority: Option<String>,
}
#[derive(Deserialize, Serialize)]
pub struct Repository {
    #[serde(rename = "repoPath")]
    pub path: String,
}
