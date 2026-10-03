//! Native per-agent `OpenClaw` transcripts, including their compressed cold tier.
use super::{
    Conversation, HistorySource, NormalizedMessage, ParsedFile, SourceIdentity, sqlite_source,
};
use crate::{AppError, provider::Provider, role::Role};
use rusqlite::OptionalExtension;
use serde::Deserialize;
use std::path::Path;

mod archive;
mod navigation;
#[cfg(test)]
mod tests;
pub(crate) use archive::cold_directory;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Session,
    Message,
    ThinkingLevelChange,
    ModelChange,
    Compaction,
    Reset,
    BranchSummary,
    Custom,
    CustomMessage,
    Label,
    SessionInfo,
    Leaf,
    #[serde(other)]
    Unknown,
}
impl Kind {
    fn canonical(self) -> bool {
        !matches!(self, Self::Session | Self::Leaf | Self::Unknown)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
enum Parent {
    #[default]
    Missing,
    Null,
    Id(String),
}
impl<'de> Deserialize<'de> for Parent {
    fn deserialize<D: serde::Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        Option::<String>::deserialize(input).map(|value| value.map_or(Self::Null, Self::Id))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum AppendMode {
    Side,
}

#[derive(Deserialize)]
struct Event {
    #[serde(rename = "type")]
    kind: Kind,
    id: Option<String>,
    #[serde(rename = "parentId", default)]
    parent: Parent,
    #[serde(rename = "targetId", default)]
    target: Parent,
    #[serde(rename = "appendParentId", default)]
    append_parent: Parent,
    #[serde(rename = "appendMode")]
    append_mode: Option<AppendMode>,
    timestamp: Option<String>,
    cwd: Option<String>,
    name: Option<String>,
    message: Option<Message>,
    summary: Option<String>,
    #[serde(rename = "firstKeptEntryId")]
    first_kept: Option<String>,
    content: Option<super::json::Content>,
    display: Option<bool>,
    #[serde(skip)]
    observed_at: i64,
}

#[derive(Deserialize)]
#[serde(tag = "role")]
enum Message {
    #[serde(rename = "user")]
    User { content: super::json::Content },
    #[serde(rename = "assistant")]
    Assistant { content: super::json::Content },
    #[serde(rename = "toolResult")]
    Tool { content: super::json::Content },
    #[serde(rename = "custom")]
    Custom { content: super::json::Content },
    #[serde(rename = "bashExecution")]
    Bash { command: String, output: String },
    #[serde(other)]
    Unknown,
}

pub(super) fn discover(path: &Path) -> Result<Vec<HistorySource>, AppError> {
    let connection = sqlite_source::open(path)?;
    let mut statement = connection.prepare("SELECT session_id,MAX(updated_at,COALESCE(transcript_updated_at,updated_at)) AS activity,
        (SELECT COALESCE(SUM(COALESCE(event_utf8_bytes,length(CAST(event_json AS BLOB)))),0) FROM transcript_events t WHERE t.session_id=w.session_id) AS bytes
        FROM session_windows w ORDER BY session_id").map_err(AppError::database)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>("session_id")?,
                row.get::<_, i64>("activity")?,
                row.get::<_, i64>("bytes")?,
            ))
        })
        .map_err(AppError::database)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::database)?;
    rows.into_iter()
        .map(|(session, activity, bytes)| {
            if session.trim().is_empty() {
                return Err(AppError::configuration(
                    "OpenClaw session identity is empty",
                ));
            }
            let activity_ns = activity
                .checked_mul(1_000_000)
                .filter(|n| *n >= 0)
                .ok_or_else(|| {
                    AppError::configuration("OpenClaw session activity is out of range")
                })?;
            let mut estimated_bytes = u64::try_from(bytes)
                .map_err(|_| AppError::configuration("invalid OpenClaw transcript byte count"))?;
            let mut dependencies = Vec::new();
            if let Some(cold) = archive::cold(&connection, &session)? {
                estimated_bytes = estimated_bytes.saturating_add(cold.estimated_bytes()?);
                dependencies.extend(
                    cold.dependency(path)?
                        .map(super::source::Dependency::Required),
                );
            }
            Ok(HistorySource {
                identity: SourceIdentity {
                    path: path.to_path_buf(),
                    session: Some(session),
                },
                activity_ns: Some(activity_ns),
                estimated_bytes: Some(estimated_bytes),
                dependencies,
            })
        })
        .collect()
}

fn read_events(
    connection: &rusqlite::Connection,
    source: &SourceIdentity,
    session: &str,
) -> Result<Vec<Event>, AppError> {
    if let Some(cold) = archive::cold(connection, session)? {
        let hot: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM transcript_events WHERE session_id=?1)",
                [session],
                |row| row.get(0),
            )
            .map_err(AppError::database)?;
        if hot {
            return Err(AppError::configuration(
                "OpenClaw transcript has conflicting hot and cold owners",
            ));
        }
        cold.events(connection, &source.path, session)
    } else {
        let mut statement = connection.prepare("SELECT event_json,event_zstd,event_utf8_bytes,created_at,
            COALESCE(length(CAST(event_json AS BLOB)),length(event_zstd)) AS bytes FROM transcript_events WHERE session_id=?1 ORDER BY seq")
            .map_err(AppError::database)?;
        let mut rows = statement.query([session]).map_err(AppError::database)?;
        let mut events = Vec::new();
        while let Some(row) = rows.next().map_err(AppError::database)? {
            events.push(archive::hot(row)?);
        }
        Ok(events)
    }
}

fn messages(
    source: &SourceIdentity,
    events: &mut [Event],
    selected: &[usize],
) -> Result<Vec<NormalizedMessage>, AppError> {
    let mut messages = Vec::new();
    for &index in selected {
        let event = &mut events[index];
        let projected = match event.kind {
            Kind::Message => {
                match event
                    .message
                    .take()
                    .ok_or_else(|| AppError::configuration("OpenClaw message has no payload"))?
                {
                    Message::User { content } => content
                        .projected()
                        .map(|(text, projection)| (Role::User, text, projection)),
                    Message::Assistant { content } => content
                        .projected()
                        .map(|(text, projection)| (Role::Assistant, text, projection)),
                    Message::Tool { content } => content
                        .flatten()
                        .map(|text| (Role::Tool, text, Some(String::new()))),
                    Message::Custom { content } => content
                        .flatten()
                        .map(|text| (Role::System, text, Some(String::new()))),
                    Message::Bash { command, output } => Some((
                        Role::Tool,
                        format!("Command: {command}\n{output}"),
                        Some(String::new()),
                    )),
                    Message::Unknown => None,
                }
            }
            Kind::Compaction | Kind::BranchSummary => event
                .summary
                .take()
                .map(|text| (Role::System, text, Some(String::new()))),
            Kind::CustomMessage if event.display == Some(true) => event
                .content
                .take()
                .and_then(|c| c.flatten())
                .map(|text| (Role::System, text, Some(String::new()))),
            _ => None,
        };
        let Some((role, content, projection)) = projected else {
            continue;
        };
        if content.trim().is_empty() {
            continue;
        }
        let id = event
            .id
            .as_deref()
            .ok_or_else(|| AppError::configuration("OpenClaw event has no identity"))?;
        let timestamp = event
            .timestamp
            .as_deref()
            .map(|value| {
                chrono::DateTime::parse_from_rfc3339(value)
                    .map(|time| time.timestamp_millis())
                    .map_err(|_| AppError::configuration("invalid OpenClaw event timestamp"))
            })
            .transpose()?
            .unwrap_or(event.observed_at);
        if timestamp < 0 {
            return Err(AppError::configuration("invalid OpenClaw event timestamp"));
        }
        let search_projection = Some(
            crate::search_text::project(&role, projection.as_deref().unwrap_or(&content))
                .into_owned(),
        );
        messages.push(NormalizedMessage {
            id: super::parsers::stable_id(
                "message",
                &[Provider::Openclaw.as_str(), &source.key(), id],
            ),
            ordinal: i64::try_from(messages.len())
                .map_err(|_| AppError::internal("too many OpenClaw messages"))?,
            context: crate::message_context::MessageContext::Linear,
            role,
            content,
            search_projection,
            created_at: Some(timestamp),
        });
    }
    Ok(messages)
}

struct Session {
    title: Option<String>,
    created_at: i64,
    updated_at: i64,
}

pub(super) fn parse(source: &SourceIdentity) -> Result<ParsedFile, AppError> {
    let session = source
        .session
        .as_deref()
        .ok_or_else(|| AppError::configuration("OpenClaw requires a session identity"))?;
    let connection = sqlite_source::open(&source.path)?;
    let metadata = connection.query_row("SELECT COALESCE(NULLIF(n.label,''),NULLIF(w.display_name,'')) AS title,w.created_at,w.updated_at
        FROM session_windows w JOIN session_nodes n ON n.session_key=w.session_key WHERE w.session_id=?1",[session],|row|Ok(Session {
            title:row.get("title")?,created_at:row.get("created_at")?,updated_at:row.get("updated_at")?}))
        .optional().map_err(AppError::database)?.ok_or_else(||AppError::configuration("OpenClaw session disappeared; retry index"))?;
    if metadata.created_at < 0 || metadata.updated_at < 0 {
        return Err(AppError::configuration(
            "invalid OpenClaw session timestamp",
        ));
    }
    let mut events = read_events(&connection, source, session)?;
    let selected = navigation::select(&events)?;
    let project = events
        .iter()
        .find(|event| event.kind == Kind::Session)
        .and_then(|header| header.cwd.clone());
    let title = metadata.title.or_else(|| {
        selected.iter().rev().find_map(|&i| {
            (events[i].kind == Kind::SessionInfo)
                .then(|| events[i].name.clone())
                .flatten()
        })
    });
    let messages = messages(source, &mut events, &selected)?;
    Ok(ParsedFile {
        conversation: Some(Conversation {
            id: super::parsers::stable_id(
                "conversation",
                &[Provider::Openclaw.as_str(), &source.key()],
            ),
            provider: Provider::Openclaw,
            claude_lineage: None,
            source: source.clone(),
            project,
            repository: None,
            title,
            created_at: Some(metadata.created_at),
            updated_at: Some(metadata.updated_at),
            messages: super::messages::Messages::stage(messages)?,
        }),
        malformed_records: 0,
        first_record_issue: None,
    })
}
