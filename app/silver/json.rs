//! Known history fields are typed; provider-owned tool payloads remain opaque JSON.
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    User,
    Assistant,
    System,
    Text,
    ResponseItem,
    EventMsg,
    ItemCompleted,
    TaskStarted,
    TaskComplete,
    SessionMeta,
    Message,
    FunctionCall,
    CustomToolCall,
    FunctionCallOutput,
    CustomToolCallOutput,
    Thinking,
    #[serde(rename = "toolCall")]
    ToolCall,
    ToolUse,
    ToolResult,
    InputText,
    OutputText,
    Image,
    InputImage,
    Document,
    #[default]
    #[serde(other)]
    Unknown,
}

impl Kind {
    const fn is_context(self) -> bool {
        matches!(
            self,
            Self::ToolUse
                | Self::Image
                | Self::InputImage
                | Self::Document
                | Self::ToolCall
                | Self::ToolResult
                | Self::Thinking
                | Self::EventMsg
                | Self::ItemCompleted
                | Self::TaskStarted
                | Self::TaskComplete
                | Self::Unknown
                | Self::System
        )
    }
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::System => "system",
            Self::Text => "text",
            Self::ResponseItem => "response_item",
            Self::EventMsg => "event_msg",
            Self::ItemCompleted => "item_completed",
            Self::TaskStarted => "task_started",
            Self::TaskComplete => "task_complete",
            Self::SessionMeta => "session_meta",
            Self::Message => "message",
            Self::FunctionCall => "function_call",
            Self::CustomToolCall => "custom_tool_call",
            Self::FunctionCallOutput => "function_call_output",
            Self::CustomToolCallOutput => "custom_tool_call_output",
            Self::Thinking => "thinking",
            Self::ToolCall => "toolCall",
            Self::ToolUse => "tool_use",
            Self::ToolResult => "tool_result",
            Self::InputText => "input_text",
            Self::OutputText => "output_text",
            Self::Image => "image",
            Self::InputImage => "input_image",
            Self::Document => "document",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Default, Deserialize)]
pub(super) struct Record {
    #[serde(rename = "type", default)]
    pub kind: Kind,
    pub uuid: Option<String>,
    pub timestamp: Option<String>,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
    #[serde(rename = "agentId")]
    pub agent_id: Option<String>,
    #[serde(rename = "parentUuid", default, deserialize_with = "parent_link")]
    pub parent: ParentLink,
    pub subtype: Option<SystemSubtype>,
    #[serde(rename = "logicalParentUuid")]
    pub logical_parent: Option<String>,
    #[serde(rename = "compactMetadata")]
    pub compact_metadata: Option<CompactMetadata>,
    pub message: Option<Payload>,
    pub payload: Option<Payload>,
    #[serde(flatten)]
    pub metadata: Metadata,
}

#[derive(Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SystemSubtype {
    CompactBoundary,
    StopHookSummary,
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
pub(super) struct CompactMetadata {
    #[serde(rename = "preservedSegment")]
    pub preserved_segment: Option<PreservedSegment>,
}

#[derive(Deserialize)]
pub(super) struct PreservedSegment {
    #[serde(rename = "tailUuid")]
    pub tail: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) enum ParentLink {
    #[default]
    Missing,
    Root,
    Parent(String),
}

fn parent_link<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<ParentLink, D::Error> {
    Option::<String>::deserialize(deserializer)
        .map(|parent| parent.map_or(ParentLink::Root, ParentLink::Parent))
}

#[derive(Default, Deserialize)]
pub(super) struct Metadata {
    pub cwd: Option<String>,
    pub git: Option<Git>,
}

#[derive(Deserialize)]
pub(super) struct Git {
    pub repository_url: Option<String>,
}

#[derive(Default, Deserialize)]
pub(super) struct Payload {
    #[serde(rename = "type", default)]
    pub kind: Kind,
    pub id: Option<String>,
    pub session_id: Option<String>,
    pub call_id: Option<String>,
    pub turn_id: Option<String>,
    pub thread_id: Option<String>,
    pub item: Option<CompletedItem>,
    pub role: Option<crate::role::Role>,
    pub name: Option<String>,
    pub content: Option<Content>,
    #[serde(default, deserialize_with = "opaque")]
    pub arguments: Option<Value>,
    #[serde(default, deserialize_with = "opaque")]
    pub input: Option<Value>,
    #[serde(default, deserialize_with = "opaque")]
    pub output: Option<Value>,
    #[serde(flatten)]
    pub metadata: Metadata,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub(super) enum CompletedItem {
    UserMessage {
        id: String,
        content: Vec<UserText>,
    },
    AgentMessage {
        id: String,
        content: Vec<AgentText>,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub(super) enum UserText {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { image_url: Option<String> },
    #[serde(rename = "local_image")]
    LocalImage { path: std::path::PathBuf },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub(super) enum AgentText {
    Text {
        text: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum Content {
    Text(String),
    Blocks(Vec<Block>),
    Block(Box<Block>),
}

#[derive(Default, Deserialize)]
pub(super) struct Block {
    pub source: Option<MediaSource>,
    pub image_url: Option<String>,
    pub title: Option<String>,
    #[serde(rename = "type", default)]
    pub kind: Kind,
    pub text: Option<String>,
    pub content: Option<Content>,
    pub thinking: Option<String>,
    pub name: Option<String>,
    #[serde(default, deserialize_with = "opaque")]
    pub arguments: Option<Value>,
    #[serde(default, deserialize_with = "opaque")]
    pub input: Option<Value>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum MediaSource {
    Base64 {
        media_type: String,
        data: String,
    },
    Url {
        url: String,
    },
    #[serde(other)]
    Other,
}

impl Content {
    pub fn attachments(&self) -> Vec<crate::attachments::Source> {
        match self {
            Self::Text(_) => Vec::new(),
            Self::Block(block) => block.attachment().into_iter().collect(),
            Self::Blocks(blocks) => blocks
                .iter()
                .filter_map(Block::attachment)
                .take(crate::attachments::MAX_FILES)
                .collect(),
        }
    }
    pub fn flatten(&self) -> Option<String> {
        match self {
            Self::Text(text) => Some(text.clone()),
            Self::Blocks(blocks) => {
                let parts = blocks.iter().filter_map(Block::flatten).collect::<Vec<_>>();
                (!parts.is_empty()).then(|| parts.join("\n"))
            }
            Self::Block(block) => block.flatten(),
        }
    }

    pub fn projected(&self) -> Option<(String, Option<String>)> {
        let content = self.flatten()?;
        match self {
            Self::Block(block) if block.kind.is_context() => Some((content, Some(String::new()))),
            Self::Blocks(blocks) if blocks.iter().any(|block| block.kind.is_context()) => {
                let projection = blocks
                    .iter()
                    .filter(|block| !block.kind.is_context())
                    .filter_map(Block::flatten)
                    .collect::<Vec<_>>()
                    .join("\n");
                Some((content, Some(projection)))
            }
            _ => Some((content, None)),
        }
    }
}

impl Block {
    fn attachment(&self) -> Option<crate::attachments::Source> {
        if !matches!(self.kind, Kind::Image | Kind::InputImage | Kind::Document) {
            return None;
        }
        let name = self
            .title
            .as_deref()
            .unwrap_or(if self.kind == Kind::Document {
                "document"
            } else {
                "image"
            });
        Some(match &self.source {
            Some(MediaSource::Base64 { media_type, data }) => crate::attachments::Source::Base64 {
                name: name.into(),
                media_type: media_type.clone(),
                data: data.clone(),
            },
            Some(MediaSource::Url { url }) => crate::attachments::url(name, url),
            _ => self.image_url.as_deref().map_or_else(
                || crate::attachments::Source::External {
                    name: name.into(),
                    media_type: "application/octet-stream".into(),
                },
                |url| crate::attachments::url(name, url),
            ),
        })
    }
    fn flatten(&self) -> Option<String> {
        if let Some(text) = self.text.as_ref().or(match self.content.as_ref() {
            Some(Content::Text(text)) => Some(text),
            _ => None,
        }) {
            return Some(text.clone());
        }
        match self.kind {
            Kind::Image | Kind::InputImage | Kind::Document => Some(format!(
                "[Attachment: {}]",
                self.title
                    .as_deref()
                    .unwrap_or(if self.kind == Kind::Document {
                        "document"
                    } else {
                        "image"
                    })
            )),
            Kind::Thinking => self
                .thinking
                .as_ref()
                .map(|thinking| format!("[Thinking] {thinking}")),
            Kind::ToolCall | Kind::ToolUse => {
                let input = if self.kind == Kind::ToolCall {
                    &self.arguments
                } else {
                    &self.input
                };
                Some(format!(
                    "Tool {}: {}",
                    self.name.as_deref().unwrap_or("unknown"),
                    input.as_ref().map_or_else(String::new, stringify)
                ))
            }
            Kind::ToolResult => self.content.as_ref().and_then(Content::flatten),
            _ => None,
        }
    }
}

pub(super) fn stringify(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

// A present JSON null is tool data, distinct from an absent field.
fn opaque<'de, D: serde::Deserializer<'de>>(input: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(input).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projections_include_known_prose_and_preserve_unknown_blocks_only_as_context() {
        let content: Content = serde_json::from_str(
            r#"[
            {"type":"text","text":"spoken answer"},
            {"type":"input_text","text":"request"},
            {"type":"output_text","text":"response"},
            {"type":"future_tool_kind","text":"opaque instructions"},
            {"type":"thinking","thinking":"internal reasoning"},
            {"type":"toolCall","name":"shell","arguments":"private command"}
        ]"#,
        )
        .unwrap();
        let (canonical, projection) = content.projected().unwrap();
        assert!(canonical.contains("opaque instructions"));
        assert!(canonical.contains("internal reasoning"));
        assert!(canonical.contains("private command"));
        assert_eq!(
            projection.as_deref(),
            Some("spoken answer\nrequest\nresponse")
        );
    }

    #[test]
    fn typed_history_accepts_evolving_fields_and_preserves_opaque_tool_data() {
        let record: Record = serde_json::from_str(r#"{"type":"response_item","new_field":true,"payload":{"type":"custom_tool_call","input":{"nested":[null,3,true]},"role":"future-role"}}"#).unwrap();
        let payload = record.payload.unwrap();
        assert_eq!(payload.kind, Kind::CustomToolCall);
        assert_eq!(
            payload.role.unwrap(),
            crate::role::Role::Other("future-role".into())
        );
        assert_eq!(
            stringify(payload.input.as_ref().unwrap()),
            r#"{"nested":[null,3,true]}"#
        );
        let record: Record = serde_json::from_str(
            r#"{"type":"future_event","payload":{"type":"future_payload","output":null}}"#,
        )
        .unwrap();
        assert_eq!(record.kind, Kind::Unknown);
        assert_eq!(
            stringify(record.payload.unwrap().output.as_ref().unwrap()),
            "null"
        );
        assert!(
            serde_json::from_str::<Record>(
                r#"{"type":"user","message":{"role":3,"content":"text"}}"#
            )
            .is_err()
        );
    }
}
