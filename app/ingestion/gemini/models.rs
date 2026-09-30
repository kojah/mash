//! Gemini's native recording schema. Only tool-owned arguments/results are opaque.
use crate::role::Role;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum Record {
    Rewind {
        #[serde(rename = "$rewindTo")]
        target: String,
    },
    Set {
        #[serde(rename = "$set")]
        update: Update,
    },
    Message(Message),
    Header(Header),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Header {
    pub session_id: String,
    pub project_hash: String,
    #[serde(flatten)]
    pub update: Update,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Update {
    pub session_id: Option<String>,
    pub project_hash: Option<String>,
    pub start_time: Option<String>,
    pub last_updated: Option<String>,
    pub summary: Option<String>,
    pub kind: Option<SessionKind>,
    pub messages: Option<Vec<Message>>,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum SessionKind {
    #[default]
    Main,
    Subagent,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum MessageKind {
    User,
    Gemini,
    Info,
    Error,
    Warning,
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Message {
    pub id: String,
    pub timestamp: String,
    #[serde(rename = "type")]
    kind: MessageKind,
    content: Content,
    display_content: Option<Content>,
    #[serde(default)]
    thoughts: Vec<Thought>,
    #[serde(default)]
    tool_calls: Vec<ToolCall>,
}

#[derive(Deserialize)]
struct Thought {
    subject: Option<String>,
    description: String,
}

#[derive(Deserialize)]
struct ToolCall {
    name: String,
    args: serde_json::Value,
    result: Option<Content>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Content {
    Text(String),
    Parts(Vec<Content>),
    Part(Box<Part>),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Part {
    text: Option<String>,
    #[serde(default)]
    thought: bool,
    function_call: Option<FunctionCall>,
    function_response: Option<FunctionResponse>,
    inline_data: Option<InlineData>,
    file_data: Option<FileData>,
    executable_code: Option<ExecutableCode>,
    code_execution_result: Option<CodeResult>,
}

#[derive(Deserialize)]
struct FunctionCall {
    name: String,
    args: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct FunctionResponse {
    name: String,
    response: Option<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InlineData {
    mime_type: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileData {
    file_uri: String,
}

#[derive(Deserialize)]
struct ExecutableCode {
    code: String,
}

#[derive(Deserialize)]
struct CodeResult {
    output: Option<String>,
}

#[derive(Default)]
pub(super) struct Text {
    pub canonical: String,
    pub projection: String,
}

impl Text {
    fn context(&mut self, value: &str) {
        if !self.canonical.is_empty() {
            self.canonical.push('\n');
        }
        self.canonical.push_str(value);
    }
}

impl Content {
    fn text(&self, output: &mut Text) {
        match self {
            Self::Text(text) => {
                output.canonical.push_str(text);
                output.projection.push_str(text);
            }
            Self::Parts(parts) => {
                for part in parts {
                    part.text(output);
                }
            }
            Self::Part(part) => part.text(output),
        }
    }
}

impl Part {
    fn text(&self, output: &mut Text) {
        // Prose is opt-in; unknown binary/SDK fields cannot become search evidence.
        if let Some(text) = &self.text {
            if self.thought {
                output.context(&format!("Thought: {text}"));
            } else {
                output.canonical.push_str(text);
                output.projection.push_str(text);
            }
        }
        if let Some(call) = &self.function_call {
            output.context(&format!(
                "Tool: {}\n{}",
                call.name,
                call.args
                    .as_ref()
                    .map_or(String::new(), ToString::to_string)
            ));
        }
        if let Some(response) = &self.function_response {
            output.context(&format!(
                "Tool result: {}\n{}",
                response.name,
                response
                    .response
                    .as_ref()
                    .map_or(String::new(), ToString::to_string)
            ));
        }
        if let Some(data) = &self.inline_data {
            output.context(&format!(
                "Attachment: {}",
                data.mime_type.as_deref().unwrap_or("binary")
            ));
        }
        if let Some(data) = &self.file_data {
            output.context(&format!("File: {}", data.file_uri));
        }
        if let Some(code) = &self.executable_code {
            output.context(&format!("Executed code: {}", code.code));
        }
        if let Some(result) = &self.code_execution_result {
            output.context(result.output.as_deref().unwrap_or("Code execution result"));
        }
    }
}

impl Message {
    pub(super) fn normalized(&self, kind: SessionKind) -> (Role, Text) {
        let role = match self.kind {
            MessageKind::User => Role::User,
            MessageKind::Gemini => Role::Assistant,
            _ => Role::System,
        };
        let mut output = Text::default();
        self.content.text(&mut output);
        if let Some(display) = &self.display_content {
            let mut shown = Text::default();
            display.text(&mut shown);
            output.projection = shown.projection;
        }
        for thought in &self.thoughts {
            output.context(&format!(
                "Thought: {}{}",
                thought
                    .subject
                    .as_deref()
                    .map_or(String::new(), |s| format!("{s}: ")),
                thought.description
            ));
        }
        for call in &self.tool_calls {
            output.context(&format!("Tool: {}\n{}", call.name, call.args));
            if let Some(result) = &call.result {
                let mut result_text = Text::default();
                result.text(&mut result_text);
                output.context(&result_text.canonical);
            }
        }
        if !matches!(kind, SessionKind::Main)
            || (role == Role::User && output.projection.trim_start().starts_with(['/', '?']))
        {
            output.projection.clear();
        } else {
            let clean = crate::search_text::without_context_blocks(
                &output.projection,
                &["session_context", "hook_context", "state_snapshot"],
            );
            output.projection = crate::search_text::project(&role, &clean).into_owned();
        }
        (role, output)
    }
}
