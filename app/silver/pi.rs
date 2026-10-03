//! Pi v3 append-only JSONL sessions: keep all branches, each native entry once.
use super::{Conversation, NormalizedMessage, ParsedFile, SourceIdentity, json, records};
use crate::{AppError, message_context::MessageContext, provider::Provider, role::Role};
use serde::Deserialize;
use std::{collections::HashMap, fs::File, io::BufReader, path::Path};

#[cfg(test)]
mod tests;

#[derive(Deserialize)]
enum HeaderKind {
    #[serde(rename = "session")]
    Session,
}

#[derive(Deserialize)]
struct Header {
    #[serde(rename = "type")]
    _kind: HeaderKind,
    version: Option<u32>,
    id: String,
    timestamp: String,
    cwd: Option<String>,
}

#[derive(Deserialize)]
struct Record {
    id: String,
    #[serde(rename = "parentId", deserialize_with = "Option::deserialize")]
    parent: Option<String>,
    timestamp: String,
    #[serde(flatten)]
    data: Data,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Data {
    Message {
        message: Message,
    },
    Compaction {
        summary: String,
    },
    BranchSummary {
        summary: String,
    },
    CustomMessage {
        content: json::Content,
        display: bool,
    },
    SessionInfo {
        name: Option<String>,
    },
    // State changes, extension data and context edits do not rewrite history.
    // Their IDs still bridge the native tree to its nearest canonical message.
    #[serde(other)]
    State,
}

#[derive(Deserialize)]
#[serde(tag = "role", rename_all = "camelCase")]
enum Message {
    User {
        content: json::Content,
        timestamp: Option<i64>,
    },
    Assistant {
        content: json::Content,
        timestamp: Option<i64>,
    },
    System {
        content: json::Content,
        timestamp: Option<i64>,
    },
    ToolResult {
        content: json::Content,
        timestamp: Option<i64>,
    },
    Custom {
        content: json::Content,
        display: bool,
        timestamp: Option<i64>,
    },
    BashExecution {
        command: String,
        output: String,
        timestamp: Option<i64>,
    },
    BranchSummary {
        summary: String,
        timestamp: Option<i64>,
    },
    CompactionSummary {
        summary: String,
        timestamp: Option<i64>,
    },
    #[serde(other)]
    Unknown,
}

struct Text {
    role: Role,
    canonical: String,
    projection: String,
    timestamp: Option<i64>,
}

impl Text {
    fn content(role: Role, content: &json::Content, timestamp: Option<i64>) -> Option<Self> {
        let (canonical, projection) = content.projected()?;
        let projection =
            crate::silver::dialogue::visible(&role, projection.as_deref().unwrap_or(&canonical))
                .into_owned();
        Some(Self {
            role,
            canonical,
            projection,
            timestamp,
        })
    }

    fn context(role: Role, canonical: String, timestamp: Option<i64>) -> Self {
        Self {
            role,
            canonical,
            projection: String::new(),
            timestamp,
        }
    }
}

impl Message {
    fn text(self) -> Option<Text> {
        match self {
            Self::User { content, timestamp } => Text::content(Role::User, &content, timestamp),
            Self::Assistant { content, timestamp } => {
                Text::content(Role::Assistant, &content, timestamp)
            }
            Self::ToolResult { content, timestamp } => {
                Text::content(Role::Tool, &content, timestamp)
            }
            Self::System { content, timestamp }
            | Self::Custom {
                content,
                display: true,
                timestamp,
            } => Text::content(Role::System, &content, timestamp),
            Self::BashExecution {
                command,
                output,
                timestamp,
            } => Some(Text::context(
                Role::Tool,
                format!("Shell: {command}\n{output}"),
                timestamp,
            )),
            Self::BranchSummary { summary, timestamp }
            | Self::CompactionSummary { summary, timestamp } => {
                Some(Text::context(Role::System, summary, timestamp))
            }
            Self::Custom { display: false, .. } | Self::Unknown => None,
        }
    }
}

fn timestamp(value: &str) -> Result<i64, AppError> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|t| t.timestamp_millis())
        .map_err(|_| AppError::configuration("invalid Pi entry timestamp"))
}

fn read_header(
    lines: &mut impl Iterator<Item = Result<String, AppError>>,
) -> Result<Header, AppError> {
    let header: Header = serde_json::from_str(
        &lines
            .next()
            .ok_or_else(|| AppError::configuration("Pi session header is missing"))??,
    )
    .map_err(|_| AppError::configuration("invalid Pi session header"))?;
    if header.version != Some(3) {
        return Err(AppError::configuration(
            "unsupported Pi session version; open the session in Pi to migrate it to v3",
        ));
    }
    if header.id.trim().is_empty() {
        return Err(AppError::configuration("Pi session identity is empty"));
    }
    Ok(header)
}

pub(super) fn parse(path: &Path) -> Result<ParsedFile, AppError> {
    let mut lines = records::Lines::new(BufReader::new(File::open(path).map_err(AppError::io)?))
        .filter(|line| match line {
            Ok(line) => !line.trim().is_empty(),
            Err(_) => true,
        });
    let header = read_header(&mut lines)?;
    let created_at = timestamp(&header.timestamp)?;
    let mut messages = Vec::<NormalizedMessage>::new();
    let mut nearest = HashMap::<String, Option<i64>>::new();
    let mut title = None;
    for line in lines {
        let record: Record = serde_json::from_str(&line?)
            .map_err(|_| AppError::configuration("invalid Pi session entry"))?;
        if record.id.trim().is_empty() || nearest.contains_key(&record.id) {
            return Err(AppError::configuration(
                "empty or duplicate Pi entry identity",
            ));
        }
        let mut parent = match record.parent {
            None => None,
            Some(id) => *nearest.get(&id).ok_or_else(|| {
                AppError::configuration("Pi entry references a missing or forward parent")
            })?,
        };
        let entry_time = timestamp(&record.timestamp)?;
        let text = match record.data {
            Data::Message { message } => message.text(),
            Data::Compaction { summary } | Data::BranchSummary { summary } => {
                Some(Text::context(Role::System, summary, None))
            }
            Data::CustomMessage {
                content,
                display: true,
            } => Text::content(Role::System, &content, None),
            Data::SessionInfo { name } => {
                title = name.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
                None
            }
            Data::CustomMessage { display: false, .. } | Data::State => None,
        };
        if let Some(text) = text.filter(|text| !text.canonical.trim().is_empty()) {
            let ordinal = i64::try_from(messages.len())
                .map_err(|_| AppError::internal("too many Pi messages"))?;
            messages.push(NormalizedMessage {
                id: super::parsers::stable_id(
                    "message",
                    &[Provider::Pi.as_str(), &header.id, &record.id],
                ),
                ordinal,
                context: parent.map_or(MessageContext::Root, |ordinal| MessageContext::Parent {
                    ordinal,
                }),
                role: text.role,
                content: text.canonical,
                dialogue_text: Some(text.projection),
                created_at: Some(text.timestamp.unwrap_or(entry_time)),
            });
            parent = Some(ordinal);
        }
        nearest.insert(record.id, parent);
    }
    let title = title.or_else(|| {
        messages
            .iter()
            .filter(|m| m.role == Role::User)
            .find_map(|m| {
                crate::silver::dialogue::title(m.dialogue_text.as_deref().unwrap_or(&m.content))
            })
    });
    let updated_at = messages
        .iter()
        .filter_map(|m| m.created_at)
        .max()
        .unwrap_or(created_at);
    Ok(ParsedFile {
        conversation: Some(Conversation {
            id: super::parsers::stable_id("session", &[Provider::Pi.as_str(), &header.id]),
            provider: Provider::Pi,
            claude_lineage: None,
            source: SourceIdentity::file(path),
            project: header.cwd.filter(|s| !s.is_empty()),
            repository: None,
            title,
            created_at: Some(created_at),
            updated_at: Some(updated_at),
            messages: super::messages::Messages::stage(messages)?,
        }),
        malformed_records: 0,
        first_record_issue: None,
    })
}
