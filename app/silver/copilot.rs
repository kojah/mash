//! Native Copilot CLI events and the separate regular VS Code Chat store.
//! CLI event parents describe chronology; each agent has its own context chain.
use super::{Chat, NormalizedMessage, ParsedFile, SourceIdentity, records};
use crate::{AppError, message_context::MessageContext, provider::Provider, role::Role};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::BufReader,
    path::Path,
};

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

/// Report unreadable lines instead of an error they may have caused, such as
/// a missing header when the only line is still being written.
fn guarded(
    parse: impl FnOnce(&mut Unreadable) -> Result<ParsedFile, AppError>,
) -> Result<ParsedFile, AppError> {
    let mut unreadable = Unreadable::default();
    let parsed = parse(&mut unreadable);
    unreadable.rejected().map_or(parsed, Ok)
}

/// Copilot appends to its logs while a chat runs, so the last line can be a
/// partial write. Any line that is not JSON leaves the snapshot unprovable:
/// interpretation stops there, and the source keeps its previous
/// contribution while the unreadable lines are reported.
#[derive(Default)]
struct Unreadable {
    count: u64,
    first: Option<super::diagnostics::RecordIssue>,
}

impl Unreadable {
    /// Non-blank lines before the first unreadable one; later lines are
    /// only checked so that every unreadable line is counted.
    fn lines<'a>(
        &'a mut self,
        path: &'a Path,
        lines: impl Iterator<Item = Result<String, AppError>> + 'a,
    ) -> impl Iterator<Item = Result<String, AppError>> + 'a {
        lines.enumerate().filter_map(move |(index, line)| {
            let line = match line {
                Ok(line) if line.trim().is_empty() => return None,
                Ok(line) => line,
                Err(error) => return Some(Err(error)),
            };
            if let Err(error) = serde_json::from_str::<serde::de::IgnoredAny>(&line) {
                self.count += 1;
                self.first.get_or_insert_with(|| {
                    super::diagnostics::RecordIssue::json(
                        Provider::Copilot,
                        path,
                        index + 1,
                        &error,
                    )
                });
                return None;
            }
            (self.count == 0).then_some(Ok(line))
        })
    }

    fn rejected(self) -> Option<ParsedFile> {
        self.first.map(|first| ParsedFile {
            chat: None,
            malformed_records: self.count,
            first_record_issue: Some(first),
        })
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
#[serde(rename_all = "camelCase")]
struct ToolRequest {
    tool_call_id: Option<String>,
    name: String,
    // Arbitrary tool arguments are the opaque boundary of the native schema.
    arguments: Option<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolStart {
    tool_call_id: Option<String>,
    tool_name: String,
    arguments: Option<serde_json::Value>,
    parent_tool_call_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolComplete {
    tool_call_id: Option<String>,
    success: Option<bool>,
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
    tools: crate::tools::Activity,
}

/// One stored message from an event. An assistant event can hold reasoning,
/// a reply and tool requests; each becomes its own message, and only the reply
/// keeps the event's own identity.
struct Part {
    suffix: Option<String>,
    text: Text,
}

impl Part {
    fn whole(text: Text) -> Self {
        Self { suffix: None, text }
    }
}

impl Text {
    fn context(role: Role, canonical: String) -> Self {
        Self {
            role,
            canonical,
            projection: String::new(),
            tools: crate::tools::Activity::default(),
        }
    }

    /// Model reasoning, shown as thinking rather than as the reply.
    fn thinking(content: &str) -> Self {
        Self::context(Role::Assistant, format!("[Thinking] {content}"))
    }

    fn call(id: Option<&str>, name: &str, arguments: Option<serde_json::Value>) -> Self {
        let tools = crate::tools::Activity::call(Some(name), id, arguments.as_ref());
        Self {
            tools,
            ..Self::context(Role::Assistant, tool_text(name, arguments))
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
            tools: crate::tools::Activity::default(),
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

    /// The messages an event contributes. `requested` holds the tool calls
    /// already shown from assistant requests, so an execution start that
    /// repeats one adds nothing.
    fn parts(self, main: bool, requested: &mut HashSet<String>) -> Vec<Part> {
        match self {
            Self::User { data } => vec![Part::whole(Text::prose(
                Role::User,
                data.content,
                main && !data.is_autopilot_continuation
                    && data.source.as_ref().is_none_or(Source::human),
            ))],
            Self::Assistant { data } => {
                let mut parts = Vec::new();
                if let Some(reasoning) = data.reasoning_text.filter(|s| !s.trim().is_empty()) {
                    parts.push(Part {
                        suffix: Some("reasoning".to_owned()),
                        text: Text::thinking(&reasoning),
                    });
                }
                parts.push(Part::whole(Text::prose(
                    Role::Assistant,
                    data.content,
                    main,
                )));
                for (index, tool) in data.tool_requests.into_iter().enumerate() {
                    let suffix = tool
                        .tool_call_id
                        .as_ref()
                        .map_or_else(|| format!("tool:{index}"), |id| format!("tool:{id}"));
                    if let Some(id) = &tool.tool_call_id {
                        requested.insert(id.clone());
                    }
                    parts.push(Part {
                        suffix: Some(suffix),
                        text: Text::call(tool.tool_call_id.as_deref(), &tool.name, tool.arguments),
                    });
                }
                parts
            }
            Self::Reasoning {
                data: Content { content },
            } => vec![Part::whole(Text::thinking(&content))],
            Self::System {
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
            } => vec![Part::whole(Text::context(Role::System, content))],
            // Logs that record the request already showed this call.
            Self::ToolStart { data }
                if data
                    .tool_call_id
                    .as_ref()
                    .is_some_and(|id| requested.contains(id)) =>
            {
                Vec::new()
            }
            Self::ToolStart { data } => vec![Part::whole(Text::call(
                data.tool_call_id.as_deref(),
                &data.tool_name,
                data.arguments,
            ))],
            Self::ToolComplete { data } => {
                let mut content = data
                    .result
                    .map_or_else(String::new, |r| r.detailed_content.unwrap_or(r.content));
                let failed = data.error.is_some();
                if let Some(error) = data.error {
                    if !content.is_empty() {
                        content.push('\n');
                    }
                    content.push_str(&error.message);
                }
                let is_error = data
                    .success
                    .map(|success| !success)
                    .or(failed.then_some(true));
                vec![Part::whole(Text {
                    tools: crate::tools::Activity::result(data.tool_call_id.as_deref(), is_error),
                    ..Text::context(Role::Tool, content)
                })]
            }
            _ => Vec::new(),
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
    guarded(|unreadable| events(path, unreadable))
}

fn events(path: &Path, unreadable: &mut Unreadable) -> Result<ParsedFile, AppError> {
    let mut lines = unreadable.lines(
        path,
        records::Lines::new(BufReader::new(File::open(path).map_err(AppError::io)?)),
    );
    let (first_id, start) = read_header(&mut lines)?;
    let created_at = timestamp(&start.created_at)?;
    let mut updated_at = created_at;
    let mut workspace = start.context.filter(|w| !w.pending_git_context);
    let mut ids = super::identities::Identities::default();
    ids.insert(&first_id)?;
    let mut heads = HashMap::<Option<String>, String>::new();
    let mut requested = HashSet::new();
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
        for Part { suffix, text } in record.payload.parts(lane.is_none(), &mut requested) {
            if text.canonical.trim().is_empty() {
                continue;
            }
            let ordinal = i64::try_from(messages.len())
                .map_err(|_| AppError::internal("too many Copilot messages"))?;
            let mut identity = vec![Provider::Copilot.as_str(), &start.session_id, &record.id];
            if let Some(suffix) = &suffix {
                identity.push(suffix);
            }
            let id = super::parsers::stable_id("message", &identity);
            messages.push(NormalizedMessage {
                attachments: Vec::new(),
                id: id.clone(),
                ordinal,
                context: heads
                    .get(&lane)
                    .cloned()
                    .map_or(MessageContext::Root, |id| MessageContext::Parent { id }),
                role: text.role,
                content: text.canonical,
                dialogue_text: Some(text.projection),
                created_at: Some(time),
                tools: text.tools,
            })?;
            heads.insert(lane.clone(), id);
        }
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
        chat: Some(Chat {
            id: super::parsers::stable_id(
                "session",
                &[Provider::Copilot.as_str(), &start.session_id],
            ),
            provider: Provider::Copilot,
            title_origin: None,
            timestamp_origins: None,
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
