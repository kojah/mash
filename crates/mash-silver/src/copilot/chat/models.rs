//! VS Code's serializable chat schemas; unrelated editor fields are ignored.
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
pub struct Catalog {
    pub version: u32,
    pub entries: BTreeMap<String, Entry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub session_id: String,
    pub title: String,
    pub last_message_date: i64,
    pub working_directory: Option<String>,
    #[serde(default)]
    pub is_external: bool,
    #[serde(default)]
    pub is_empty: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub version: Option<u32>,
    pub session_id: String,
    pub creation_date: i64,
    pub custom_title: Option<String>,
    pub computed_title: Option<String>,
    pub working_directory: Option<String>,
    pub repo_data: Option<Repo>,
    pub requests: Vec<Turn>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repo {
    pub remote_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
// Independent flags mirror the upstream wire schema.
#[allow(clippy::struct_excessive_bools)]
pub struct Turn {
    pub request_id: String,
    pub message: Prompt,
    pub timestamp: Option<i64>,
    pub response_timestamp: Option<i64>,
    pub response: Option<Response>,
    /// Current VS Code records the response's completion here instead of
    /// `responseTimestamp`.
    pub model_state: Option<ModelState>,
    pub agent: Option<Agent>,
    #[serde(default)]
    pub is_hidden: bool,
    #[serde(default)]
    pub hidden_from_transcript: bool,
    #[serde(default)]
    #[serde(rename = "requestHiddenFromTranscript")]
    pub prompt_hidden: bool,
    #[serde(default)]
    pub is_system_initiated: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelState {
    pub completed_at: Option<i64>,
}

impl Turn {
    pub fn completed_at(&self) -> Option<i64> {
        self.response_timestamp
            .or_else(|| self.model_state.as_ref().and_then(|s| s.completed_at))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub extension_id: Option<ExtensionId>,
}

#[derive(Deserialize)]
pub struct ExtensionId {
    pub value: String,
}

impl Agent {
    pub fn copilot(&self) -> bool {
        self.extension_id.as_ref().is_none_or(|e| {
            e.value.eq_ignore_ascii_case("github.copilot-chat")
                || e.value.eq_ignore_ascii_case("github.copilot")
        })
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum Prompt {
    Text(String),
    Parsed { text: String },
}

impl Prompt {
    pub fn text(self) -> String {
        match self {
            Self::Text(s) | Self::Parsed { text: s } => s,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PartKind {
    MarkdownContent,
    MarkdownVuln,
    Thinking,
    ToolInvocationSerialized,
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
pub struct Markdown {
    pub value: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum DisplayText {
    Plain(String),
    Markdown(Markdown),
}

impl DisplayText {
    fn text(self) -> String {
        match self {
            Self::Plain(s) => s,
            Self::Markdown(m) => m.value,
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum Text {
    String(String),
    Chunks(Vec<String>),
}

impl Text {
    fn text(self) -> String {
        match self {
            Self::String(s) => s,
            Self::Chunks(s) => s.join("\n"),
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ResponseInput {
    Text(String),
    Parts(Vec<Part>),
}

#[derive(Default, Deserialize)]
#[serde(from = "ResponseInput")]
pub struct Response {
    parts: Vec<Part>,
}

impl From<ResponseInput> for Response {
    fn from(input: ResponseInput) -> Self {
        Self {
            parts: match input {
                ResponseInput::Text(s) => vec![Part::Text(s)],
                ResponseInput::Parts(p) => p,
            },
        }
    }
}

impl Response {
    pub fn parts(self) -> Vec<Part> {
        self.parts
    }

    pub fn parts_mut(&mut self) -> &mut Vec<Part> {
        &mut self.parts
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum Part {
    Text(String),
    Structured(Box<PartData>),
}

impl Part {
    pub fn text(self) -> (String, String) {
        match self {
            Self::Text(s) => (s.clone(), s),
            Self::Structured(part) => part.text(),
        }
    }
}

#[derive(Deserialize)]
pub struct PartData {
    pub kind: Option<PartKind>,
    pub value: Option<Text>,
    pub content: Option<Markdown>,
    pub text: Option<String>,
    #[serde(rename = "invocationMessage")]
    pub invocation_message: Option<DisplayText>,
    #[serde(rename = "pastTenseMessage")]
    pub past_tense_message: Option<DisplayText>,
    #[serde(rename = "resultDetails")]
    pub result_details: Option<serde_json::Value>,
    // Tool details have an extensible provider-owned schema, never searchable.
    #[serde(rename = "toolSpecificData")]
    pub tool_specific_data: Option<serde_json::Value>,
}

impl PartData {
    pub fn text(self) -> (String, String) {
        let searchable = self.kind.is_none()
            || matches!(
                self.kind,
                Some(PartKind::MarkdownContent | PartKind::MarkdownVuln)
            );
        let mut text = self
            .value
            .map(Text::text)
            .or_else(|| self.content.map(|m| m.value))
            .or(self.text)
            .unwrap_or_default();
        let projection = if searchable {
            text.clone()
        } else {
            String::new()
        };
        for context in [
            self.invocation_message.map(DisplayText::text),
            self.past_tense_message.map(DisplayText::text),
            self.result_details.map(|v| v.to_string()),
            self.tool_specific_data.map(|v| v.to_string()),
        ]
        .into_iter()
        .flatten()
        {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&context);
        }
        (text, projection)
    }
}

#[derive(Deserialize)]
pub struct Workspace {
    pub folder: Option<String>,
}
