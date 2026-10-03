//! Native Copilot CLI events and the separate regular VS Code Chat store.
//! CLI event parents describe chronology; each agent has its own context chain.
use super::{Conversation, NormalizedMessage, ParsedFile, SourceIdentity, records};
use crate::{AppError, message_context::MessageContext, provider::Provider, role::Role};
use serde::Deserialize;
use std::{collections::HashMap, fs::File, io::BufReader, path::Path};

#[cfg(test)]
mod tests;

mod chat;
pub(super) fn sources(
    input: Vec<super::HistorySource>,
) -> Result<Vec<super::HistorySource>, AppError> {
    chat::sources(input)
}

pub(super) fn parse_source(source: &super::HistorySource) -> Result<ParsedFile, AppError> {
    if source.identity.session.is_some() {
        chat::parse(source)
    } else {
        parse(&source.identity.path)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Event {
    id: String,
    #[serde(deserialize_with = "Option::deserialize")]
    parent_id: Option<String>,
    timestamp: String,
    #[serde(default)]
    ephemeral: bool,
    agent_id: Option<String>,
    #[serde(flatten)]
    payload: Payload,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum Payload {
    #[serde(rename = "session.start")]
    Start { data: Start },
    #[serde(rename = "session.context_changed")]
    Context { data: Workspace },
    #[serde(rename = "session.resume")]
    Resume { data: Resume },
    #[serde(rename = "session.context_cleared")]
    Cleared { data: Clear },
    #[serde(rename = "user.message")]
    User { data: User },
    #[serde(rename = "assistant.message")]
    Assistant { data: Assistant },
    #[serde(rename = "assistant.reasoning")]
    Reasoning { data: Content },
    #[serde(rename = "system.message")]
    System { data: Content },
    #[serde(rename = "tool.execution_start")]
    ToolStart { data: ToolStart },
    #[serde(rename = "tool.execution_complete")]
    ToolComplete { data: ToolComplete },
    #[serde(rename = "session.compaction_complete")]
    Compaction { data: Compaction },
    // Streaming deltas, usage, hooks and future operational events are not prose.
    #[serde(other)]
    Operational,
}

#[derive(Deserialize)]
struct Resume {
    context: Option<Workspace>,
}

#[derive(Deserialize)]
struct Clear {
    #[serde(rename = "initialMessage")]
    initial_message: Option<String>,
}

#[derive(Deserialize)]
struct Content {
    content: String,
}

#[derive(Deserialize)]
struct Compaction {
    #[serde(rename = "summaryContent")]
    summary_content: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Start {
    session_id: String,
    version: u32,
    #[serde(rename = "startTime")]
    created_at: String,
    context: Option<Workspace>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Workspace {
    cwd: String,
    repository: Option<String>,
    #[serde(default)]
    pending_git_context: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct User {
    content: String,
    source: Option<Source>,
    #[serde(default)]
    is_autopilot_continuation: bool,
}

#[derive(Deserialize)]
#[serde(from = "String")]
enum Source {
    User,
    Internal(String),
}

impl From<String> for Source {
    fn from(value: String) -> Self {
        if value == "user" {
            Self::User
        } else {
            Self::Internal(value)
        }
    }
}

impl Source {
    fn human(&self) -> bool {
        match self {
            Self::User => true,
            Self::Internal(_origin) => false,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Assistant {
    content: String,
    reasoning_text: Option<String>,
    #[serde(default)]
    tool_requests: Vec<ToolRequest>,
    parent_tool_call_id: Option<String>,
}

#[derive(Deserialize)]
struct ToolRequest {
    name: String,
    // Arbitrary tool arguments are the opaque boundary of the native schema.
    arguments: Option<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolStart {
    tool_name: String,
    arguments: Option<serde_json::Value>,
    parent_tool_call_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolComplete {
    result: Option<ToolResult>,
    error: Option<ToolError>,
    parent_tool_call_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolResult {
    content: String,
    detailed_content: Option<String>,
}

#[derive(Deserialize)]
struct ToolError {
    message: String,
}

struct Text {
    role: Role,
    canonical: String,
    projection: String,
}

impl Text {
    fn context(role: Role, canonical: String) -> Self {
        Self {
            role,
            canonical,
            projection: String::new(),
        }
    }

    fn prose(role: Role, canonical: String, searchable: bool) -> Self {
        let projection = if searchable {
            crate::silver::dialogue::visible(&role, &canonical).into_owned()
        } else {
            String::new()
        };
        Self {
            role,
            canonical,
            projection,
        }
    }
}

impl Payload {
    fn workspace(&self) -> Option<&Workspace> {
        match self {
            Self::Context { data }
            | Self::Resume {
                data: Resume {
                    context: Some(data),
                },
            } if !data.pending_git_context => Some(data),
            _ => None,
        }
    }

    fn parent_tool(&self) -> Option<&str> {
        match self {
            Self::Assistant { data } => data.parent_tool_call_id.as_deref(),
            Self::ToolStart { data } => data.parent_tool_call_id.as_deref(),
            Self::ToolComplete { data } => data.parent_tool_call_id.as_deref(),
            _ => None,
        }
    }

    fn text(self, main: bool) -> Option<Text> {
        match self {
            Self::User { data } => Some(Text::prose(
                Role::User,
                data.content,
                main && !data.is_autopilot_continuation
                    && data.source.as_ref().is_none_or(Source::human),
            )),
            Self::Assistant { data } => {
                let mut text = Text::prose(Role::Assistant, data.content, main);
                if let Some(reasoning) = data.reasoning_text.filter(|s| !s.is_empty()) {
                    text.canonical.push_str("\nReasoning: ");
                    text.canonical.push_str(&reasoning);
                }
                for tool in data.tool_requests {
                    text.canonical.push('\n');
                    text.canonical
                        .push_str(&tool_text(&tool.name, tool.arguments));
                }
                Some(text)
            }
            Self::Reasoning {
                data: Content { content },
            }
            | Self::System {
                data: Content { content },
            }
            | Self::Compaction {
                data:
                    Compaction {
                        summary_content: Some(content),
                    },
            }
            | Self::Cleared {
                data: Clear {
                    initial_message: Some(content),
                },
            } => Some(Text::context(Role::System, content)),
            Self::ToolStart { data } => Some(Text::context(
                Role::Tool,
                tool_text(&data.tool_name, data.arguments),
            )),
            Self::ToolComplete { data } => {
                let mut content = data
                    .result
                    .map_or_else(String::new, |r| r.detailed_content.unwrap_or(r.content));
                if let Some(error) = data.error {
                    if !content.is_empty() {
                        content.push('\n');
                    }
                    content.push_str(&error.message);
                }
                Some(Text::context(Role::Tool, content))
            }
            _ => None,
        }
    }
}

fn tool_text(name: &str, arguments: Option<serde_json::Value>) -> String {
    arguments.map_or_else(
        || format!("Tool: {name}"),
        |args| format!("Tool: {name}\n{args}"),
    )
}

fn timestamp(value: &str) -> Result<i64, AppError> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|t| t.timestamp_millis())
        .map_err(|_| AppError::configuration("invalid Copilot event timestamp"))
}

fn event(line: &str) -> Result<Event, AppError> {
    serde_json::from_str(line).map_err(|_| AppError::configuration("invalid Copilot session event"))
}

fn read_header(
    lines: &mut impl Iterator<Item = Result<String, AppError>>,
) -> Result<(String, Start), AppError> {
    let first = event(
        &lines
            .next()
            .ok_or_else(|| AppError::configuration("Copilot session header is missing"))??,
    )?;
    timestamp(&first.timestamp)?;
    if first.ephemeral
        || first.id.trim().is_empty()
        || first.parent_id.is_some()
        || first.agent_id.is_some()
    {
        return Err(AppError::configuration("invalid Copilot session header"));
    }
    let Payload::Start { data: start } = first.payload else {
        return Err(AppError::configuration(
            "Copilot session.start header is missing",
        ));
    };
    if start.version != 1 || start.session_id.trim().is_empty() {
        return Err(AppError::configuration(
            "unsupported Copilot session version or empty identity",
        ));
    }
    Ok((first.id, start))
}

pub(super) fn parse(path: &Path) -> Result<ParsedFile, AppError> {
    let mut lines = records::Lines::new(BufReader::new(File::open(path).map_err(AppError::io)?))
        .filter(|line| match line {
            Ok(line) => !line.trim().is_empty(),
            Err(_) => true,
        });
    let (first_id, start) = read_header(&mut lines)?;
    let created_at = timestamp(&start.created_at)?;
    let mut updated_at = created_at;
    let mut workspace = start.context.filter(|w| !w.pending_git_context);
    let mut ids = super::identities::Identities::default();
    ids.insert(&first_id)?;
    let mut heads = HashMap::<Option<String>, i64>::new();
    let mut messages = super::messages::Builder::default();
    for line in lines {
        let record = event(&line?)?;
        let time = timestamp(&record.timestamp)?;
        if record.id.trim().is_empty()
            || record.parent_id.as_deref() == Some(&record.id)
            || !ids.insert(&record.id)?
        {
            return Err(AppError::configuration(
                "empty, duplicate or self-parented Copilot event identity",
            ));
        }
        if record.ephemeral {
            continue;
        }
        updated_at = updated_at.max(time);
        if matches!(record.payload, Payload::Start { .. }) {
            return Err(AppError::configuration("multiple Copilot session headers"));
        }
        // Legacy logs identify subagents by the parent tool call instead.
        let lane = record
            .agent_id
            .or_else(|| record.payload.parent_tool().map(str::to_owned));
        if lane.is_none() && workspace.is_none() {
            workspace = record.payload.workspace().cloned();
        }
        if matches!(record.payload, Payload::Cleared { .. }) {
            heads.remove(&lane);
        }
        let Some(text) = record
            .payload
            .text(lane.is_none())
            .filter(|t| !t.canonical.trim().is_empty())
        else {
            continue;
        };
        let ordinal = i64::try_from(messages.len())
            .map_err(|_| AppError::internal("too many Copilot messages"))?;
        messages.push(NormalizedMessage {
            id: super::parsers::stable_id(
                "message",
                &[Provider::Copilot.as_str(), &start.session_id, &record.id],
            ),
            ordinal,
            context: heads
                .get(&lane)
                .copied()
                .map_or(MessageContext::Root, |ordinal| MessageContext::Parent {
                    ordinal,
                }),
            role: text.role,
            content: text.canonical,
            dialogue_text: Some(text.projection),
            created_at: Some(time),
        })?;
        heads.insert(lane, ordinal);
    }
    let messages = messages.finish()?;
    let title = messages.summary()?.title;
    let (project, repository) = workspace.map_or((None, None), |w| {
        (
            Some(w.cwd).filter(|s| !s.is_empty()),
            w.repository.filter(|s| !s.is_empty()),
        )
    });
    Ok(ParsedFile {
        conversation: Some(Conversation {
            id: super::parsers::stable_id(
                "session",
                &[Provider::Copilot.as_str(), &start.session_id],
            ),
            provider: Provider::Copilot,
            claude_lineage: None,
            source: SourceIdentity::file(path),
            project,
            repository,
            title,
            created_at: Some(created_at),
            updated_at: Some(updated_at),
            messages,
        }),
        malformed_records: 0,
        first_record_issue: None,
    })
}
