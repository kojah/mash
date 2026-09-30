//! Native Cline schemas. Unknown operational variants never become search text.
use crate::role::Role;
use serde::{Deserialize, de::DeserializeOwned};

// Bound typed sequence amplification independently of the serialized byte cap.
pub(super) fn sequence<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Visitor<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Visitor<T> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a bounded native history sequence")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<T>, A::Error> {
            let mut out = Vec::new();
            while let Some(value) = seq.next_element()? {
                if out.len() == 16_384 {
                    return Err(serde::de::Error::custom(
                        "Cline sequence exceeds 16384 entries",
                    ));
                }
                out.push(value);
            }
            Ok(out)
        }
    }
    deserializer.deserialize_seq(Visitor(std::marker::PhantomData))
}

#[derive(Deserialize)]
#[serde(transparent)]
pub(super) struct UiHistory(#[serde(deserialize_with = "sequence")] pub Vec<UiMessage>);

#[derive(Deserialize)]
pub(super) struct UiMessage {
    pub ts: i64,
    #[serde(flatten)]
    pub payload: UiPayload,
    pub text: Option<String>,
    #[serde(default)]
    pub partial: bool,
    pub seq: Option<u64>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub(super) enum UiPayload {
    #[serde(rename = "say")]
    Say { say: Say },
    #[serde(rename = "ask")]
    Ask { ask: Ask },
    #[serde(other)]
    Operational,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Say {
    Task,
    UserFeedback,
    Text,
    CompletionResult,
    PlanCompletionResult,
    Reasoning,
    #[serde(other)]
    Operational,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Ask {
    Followup,
    PlanModeRespond,
    CompletionResult,
    #[serde(other)]
    Operational,
}

#[derive(Deserialize)]
pub(super) struct Question {
    pub question: String,
}
#[derive(Deserialize)]
struct Plan {
    response: String,
}

#[derive(Deserialize)]
pub(super) struct Completion {
    pub summary: Option<String>,
    pub result: Option<String>,
}

fn embedded<T: DeserializeOwned>(text: &str) -> Option<T> {
    serde_json::from_str(text).ok()
}

impl UiMessage {
    pub(super) fn normalized(self) -> (Role, String, bool) {
        let text = self.text.unwrap_or_default();
        let (role, visible) = match self.payload {
            UiPayload::Say {
                say: Say::Task | Say::UserFeedback,
            } => (Role::User, Some(text.clone())),
            UiPayload::Say {
                say: Say::Text | Say::CompletionResult | Say::PlanCompletionResult,
            }
            | UiPayload::Ask {
                ask: Ask::CompletionResult,
            } => (Role::Assistant, Some(text.clone())),
            UiPayload::Ask { ask: Ask::Followup } => (
                Role::Assistant,
                embedded::<Question>(&text).map(|q| q.question),
            ),
            UiPayload::Ask {
                ask: Ask::PlanModeRespond,
            } => (Role::Assistant, embedded::<Plan>(&text).map(|p| p.response)),
            _ => (Role::System, None),
        };
        let searchable = visible.is_some() && !self.partial;
        (role, visible.unwrap_or(text), searchable)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SdkHistory {
    pub version: u32,
    pub session_id: String,
    pub agent: Agent,
    #[serde(rename = "updated_at")]
    pub updated_at: String,
    #[serde(deserialize_with = "sequence")]
    pub messages: Vec<SdkMessage>,
}

#[derive(Deserialize)]
pub(super) struct SdkIdentity {
    #[serde(rename = "sessionId")]
    pub id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Agent {
    Lead,
    Subagent,
    Teammate,
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
pub(super) struct SdkMessage {
    pub role: Role,
    pub content: Content,
    pub ts: Option<i64>,
    pub metadata: Option<MessageMetadata>,
}

impl SdkMessage {
    pub(super) fn legacy_boundary(&self) -> bool {
        if self.role != Role::User
            || self.metadata.as_ref().is_some_and(|m| {
                m.user_run_span == Some(0)
                    || matches!(
                        m.display_role,
                        Some(
                            DisplayRole::System
                                | DisplayRole::Status
                                | DisplayRole::Error
                                | DisplayRole::Unknown
                        )
                    )
            })
        {
            return false;
        }
        match &self.content {
            Content::Text(text) => text.contains(super::migration::RESUME_WARNING),
            Content::Blocks(blocks) => blocks.iter().any(|b|matches!(b,Block::Text {text} if text.contains(super::migration::RESUME_WARNING))),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MessageMetadata {
    pub user_run_span: Option<u32>,
    pub display_role: Option<DisplayRole>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum DisplayRole {
    User,
    Assistant,
    System,
    Status,
    Error,
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum Content {
    Text(String),
    Blocks(#[serde(deserialize_with = "sequence")] Vec<Block>),
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum Block {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
    },
    ToolUse {
        id: Option<String>,
        name: String,
        input: Option<serde_json::Value>,
    },
    ToolResult {
        tool_use_id: Option<String>,
        #[serde(default)]
        is_error: bool,
        content: ToolContent,
    },
    File {
        path: String,
    },
    #[serde(other)]
    Operational,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum ToolContent {
    Text(String),
    Blocks(#[serde(deserialize_with = "sequence")] Vec<ToolBlock>),
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ToolBlock {
    Text {
        text: String,
    },
    File {
        path: String,
    },
    #[serde(other)]
    Operational,
}

#[derive(Default, Deserialize)]
pub(super) struct Metadata {
    pub title: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct Manifest {
    pub version: u32,
    pub session_id: String,
    pub workspace_root: String,
    pub started_at: String,
    pub metadata: Option<Metadata>,
}

#[derive(Deserialize)]
#[serde(transparent)]
pub(super) struct TaskHistory(#[serde(deserialize_with = "sequence")] pub Vec<Task>);

#[derive(Deserialize)]
pub(super) struct Task {
    pub id: String,
    #[serde(rename = "task")]
    pub title: String,
    #[serde(rename = "cwdOnTaskInitialization")]
    pub project: Option<String>,
}
