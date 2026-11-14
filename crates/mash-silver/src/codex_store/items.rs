//! Typed app-server `ThreadItem` dialogue; provider tool bodies remain opaque context.
//! Shape verified against the installed Codex app-server JSON schema (October 2026).
use crate::{AppError, attachments::Source, role::Role, tools::Activity};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct Item {
    id: String,
    #[serde(flatten)]
    body: Body,
}

#[derive(Deserialize)]
pub struct RealtimeItem {
    id: String,
    #[serde(flatten)]
    body: RealtimeBody,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RealtimeBody {
    TranscriptSegment {
        realtime_session_id: String,
        role: Role,
        text: String,
    },
    RealtimeSessionStarted {
        realtime_session_id: String,
    },
    RealtimeSessionClosed {
        realtime_session_id: String,
        outcome: String,
    },
    BemItemPromoted {
        realtime_session_id: String,
        turn_id: String,
        item_id: String,
        presentation: Presentation,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Presentation {
    WholeItem,
    InlineMarkdown,
    InlineVisualization { index: usize },
}

impl RealtimeItem {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn normalize(self) -> Result<Text, AppError> {
        let canonical = serde_json::to_string(&self.body)
            .map_err(|_| super::invalid("cannot encode realtime context"))?;
        let (canonical, dialogue, role) =
            if let RealtimeBody::TranscriptSegment { role, text, .. } = self.body {
                (text.clone(), text, role)
            } else {
                (canonical, String::new(), Role::System)
            };
        Ok(Text {
            canonical,
            dialogue,
            role,
            attachments: vec![],
            tools: Activity::default(),
        })
    }
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Body {
    UserMessage {
        content: Vec<Input>,
    },
    AgentMessage {
        text: String,
    },
    HookPrompt {
        fragments: Vec<Fragment>,
    },
    Plan {
        text: String,
    },
    Reasoning {
        #[serde(default)]
        summary: Vec<String>,
        #[serde(default)]
        content: Vec<String>,
    },
    FunctionCallOutput(ToolPayload),
    CommandExecution(ToolPayload),
    FileChange(ToolPayload),
    McpToolCall(ToolPayload),
    DynamicToolCall(ToolPayload),
    CollabAgentToolCall(ToolPayload),
    WebSearch(ToolPayload),
    ImageView {
        path: String,
    },
    ImageGeneration(ToolPayload),
    Sleep {
        #[serde(rename = "durationMs")]
        duration_ms: i64,
    },
    SubAgentActivity {
        #[serde(rename = "agentThreadId")]
        agent_thread_id: String,
        #[serde(rename = "agentPath")]
        agent_path: String,
        kind: String,
    },
    EnteredReviewMode {
        review: String,
    },
    ExitedReviewMode {
        review: String,
    },
    ContextCompaction,
}

/// Tool-owned execution context is retained verbatim as canonical context, never
/// searchable dialogue. Only common tool identity/arguments are interpreted here.
#[derive(Deserialize, Serialize)]
struct ToolPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<serde_json::Value>,
    #[serde(flatten)]
    context: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Deserialize, Serialize)]
struct Fragment {
    text: String,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Input {
    Text {
        text: String,
    },
    Image {
        url: Option<String>,
        #[serde(rename = "fileId")]
        file_id: Option<String>,
    },
    LocalImage {
        path: String,
    },
    Audio {
        url: String,
    },
    LocalAudio {
        path: String,
    },
    Skill {
        name: String,
        path: String,
    },
    Mention {
        name: String,
        path: String,
    },
}

pub struct Text {
    pub canonical: String,
    pub dialogue: String,
    pub role: Role,
    pub attachments: Vec<Source>,
    pub tools: Activity,
}

impl Item {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn normalize(self) -> Result<Text, AppError> {
        let mut text = Text {
            canonical: serde_json::to_string(&self.body)
                .map_err(|_| super::invalid("cannot encode item context"))?,
            dialogue: String::new(),
            role: Role::Assistant,
            attachments: vec![],
            tools: Activity::default(),
        };
        match self.body {
            Body::UserMessage { content } => user_text(&mut text, content)?,
            Body::AgentMessage { text: message } => {
                text.canonical = message.clone();
                text.dialogue = message;
            }
            Body::Reasoning { summary, content } => {
                text.canonical = format!(
                    "Reasoning: {}",
                    summary
                        .into_iter()
                        .chain(content)
                        .collect::<Vec<_>>()
                        .join("\n")
                );
            }
            Body::HookPrompt { fragments } => {
                text.role = Role::Developer;
                text.canonical = fragments
                    .into_iter()
                    .map(|f| f.text)
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            Body::FunctionCallOutput(_) => {
                text.role = Role::Tool;
                text.tools = Activity::result(Some(&self.id), None);
            }
            Body::CommandExecution(payload)
            | Body::FileChange(payload)
            | Body::McpToolCall(payload)
            | Body::DynamicToolCall(payload)
            | Body::CollabAgentToolCall(payload)
            | Body::WebSearch(payload)
            | Body::ImageGeneration(payload) => {
                text.tools = Activity::call(
                    payload.tool.as_deref().or(payload.name.as_deref()),
                    Some(&self.id),
                    payload.arguments.as_ref(),
                );
            }
            Body::ImageView { path } => {
                text.attachments
                    .push(crate::attachments::local_image(std::path::Path::new(&path)));
            }
            Body::Plan { .. }
            | Body::Sleep { .. }
            | Body::SubAgentActivity { .. }
            | Body::EnteredReviewMode { .. }
            | Body::ExitedReviewMode { .. }
            | Body::ContextCompaction => {}
        }
        if text.attachments.len() > crate::attachments::MAX_FILES {
            return Err(super::invalid("too many item attachments"));
        }
        Ok(text)
    }
}

fn user_text(text: &mut Text, content: Vec<Input>) -> Result<(), AppError> {
    text.role = Role::User;
    let mut canonical = Vec::new();
    let mut dialogue = Vec::new();
    for input in content {
        match input {
            Input::Text { text } => {
                canonical.push(text.clone());
                dialogue.push(text);
            }
            Input::Image { url, file_id } => {
                let attachment = if let Some(url) = url {
                    crate::attachments::url("image", &url)
                } else if let Some(id) = file_id {
                    Source::External {
                        name: format!("image {id}"),
                        media_type: "application/octet-stream".into(),
                    }
                } else {
                    return Err(super::invalid("image input has no URL or file identity"));
                };
                text.attachments.push(attachment);
                canonical.push("[Image]".into());
            }
            Input::LocalImage { path } => {
                text.attachments
                    .push(crate::attachments::local_image(std::path::Path::new(&path)));
                canonical.push(format!("[Image: {path}]"));
            }
            Input::Audio { url } => {
                text.attachments
                    .push(crate::attachments::url("audio", &url));
                canonical.push("[Audio]".into());
            }
            Input::LocalAudio { path } => {
                text.attachments.push(Source::File {
                    name: "audio".into(),
                    media_type: "application/octet-stream".into(),
                    path: path.clone().into(),
                });
                canonical.push(format!("[Audio: {path}]"));
            }
            Input::Skill { name, path } | Input::Mention { name, path } => {
                canonical.push(format!("{name}: {path}"));
            }
        }
    }
    text.canonical = canonical.join("\n");
    text.dialogue = dialogue.join("\n");

    Ok(())
}
