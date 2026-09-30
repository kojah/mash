//! `OpenCode`'s native session/project database and its two upstream message layouts.
use super::{
    Conversation, HistorySource, NormalizedMessage, ParsedFile, SourceIdentity, sqlite_source,
};
use crate::{AppError, provider::Provider, role::Role};
use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use std::path::Path;

mod limits;
use limits::Budget;
#[cfg(test)]
mod tests;

fn has_table(connection: &Connection, table: &str) -> Result<bool, AppError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?1)",
            [table],
            |row| row.get(0),
        )
        .map_err(AppError::database)
}

pub(super) fn discover(path: &Path) -> Result<Vec<HistorySource>, AppError> {
    let connection = sqlite_source::open(path)?;
    let mut statement = connection
        .prepare("SELECT id,time_updated FROM session ORDER BY id")
        .map_err(AppError::database)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>("id")?,
                row.get::<_, i64>("time_updated")?,
            ))
        })
        .map_err(AppError::database)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::database)?;
    rows.into_iter().map(|(session, activity)| {
        if session.trim().is_empty() { return Err(AppError::configuration("OpenCode session identity is empty")); }
        let activity_ns = activity.checked_mul(1_000_000).filter(|n| *n >= 0)
            .ok_or_else(|| AppError::configuration("OpenCode session activity is out of range"))?;
        let bytes: i64 = if uses_current(&connection, &session)? {
            connection.query_row("SELECT COALESCE(SUM(length(CAST(data AS BLOB))),0) FROM session_message WHERE session_id=?1",
                [&session], |row| row.get(0)).map_err(AppError::database)?
        } else {
            connection.query_row("SELECT (SELECT COALESCE(SUM(length(CAST(data AS BLOB))),0) FROM message WHERE session_id=?1)
                +(SELECT COALESCE(SUM(length(CAST(data AS BLOB))),0) FROM part WHERE session_id=?1)",
                [&session], |row| row.get(0)).map_err(AppError::database)?
        };
        Ok(HistorySource { identity: SourceIdentity {path:path.to_path_buf(),session:Some(session)},
            activity_ns:Some(activity_ns), dependencies:Vec::new(), estimated_bytes:Some(u64::try_from(bytes)
                .map_err(|_| AppError::configuration("invalid OpenCode session byte count"))?) })
    }).collect()
}

fn uses_current(connection: &Connection, session: &str) -> Result<bool, AppError> {
    if !has_table(connection, "session_message")? {
        return Ok(false);
    }
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM session_message WHERE session_id=?1)",
            [session],
            |row| row.get(0),
        )
        .map_err(AppError::database)
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Kind {
    User,
    Assistant,
    Synthetic,
    System,
    Shell,
    Compaction,
    AgentSwitched,
    ModelSwitched,
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
struct Text {
    text: String,
}
#[derive(Deserialize)]
struct User {
    text: String,
    #[serde(default)]
    files: Vec<Attachment>,
}
#[derive(Deserialize)]
struct Attachment {
    uri: String,
    name: Option<String>,
    description: Option<String>,
}
#[derive(Deserialize)]
struct Assistant {
    content: Vec<Part>,
}
#[derive(Deserialize)]
struct Shell {
    command: String,
    output: String,
}
#[derive(Deserialize)]
struct Compaction {
    summary: String,
    recent: String,
}
#[derive(Deserialize)]
struct LegacyMessage {
    role: Role,
}

struct Session {
    title: String,
    project: String,
    repository: Option<String>,
    created_at: i64,
    updated_at: i64,
}

// Both layouts use typed text/reasoning parts. Tool state is deliberately opaque
// provider-owned context; it never enters the searchable projection.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum Part {
    Text {
        text: String,
        #[serde(default)]
        synthetic: bool,
        #[serde(default)]
        ignored: bool,
    },
    Reasoning {
        text: String,
    },
    Tool {
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        tool: Option<String>,
        state: serde_json::Value,
    },
    File {
        #[serde(default)]
        filename: Option<String>,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        uri: Option<String>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Default)]
struct Content {
    canonical: Vec<String>,
    prose: Vec<String>,
    retained: usize,
}
impl Content {
    fn append(&mut self, canonical: String, prose: Option<String>) -> Result<(), AppError> {
        let bytes = canonical.len()
            + std::mem::size_of::<String>()
            + prose
                .as_ref()
                .map_or(0, |text| text.len() + std::mem::size_of::<String>());
        let retained = self
            .retained
            .checked_add(bytes)
            .ok_or_else(limits::invalid)?;
        if retained > limits::MAX_RETAINED_BYTES {
            return Err(limits::invalid());
        }
        self.retained = retained;
        self.canonical.push(canonical);
        self.prose.extend(prose);
        Ok(())
    }

    fn part(&mut self, part: Part) -> Result<(), AppError> {
        let (canonical, prose) = match part {
            Part::Text {
                text,
                synthetic,
                ignored,
            } => {
                let prose = (!synthetic && !ignored).then(|| text.clone());
                (text, prose)
            }
            Part::Reasoning { text } => (format!("Reasoning: {text}"), None),
            Part::Tool { name, tool, state } => (
                format!("Tool {}: {state}", name.or(tool).unwrap_or_default()),
                None,
            ),
            Part::File {
                filename,
                name,
                url,
                uri,
            } => (
                format!(
                    "File {}: {}",
                    filename.or(name).unwrap_or_default(),
                    url.or(uri).unwrap_or_default()
                ),
                None,
            ),
            Part::Unknown => return Ok(()),
        };
        self.append(canonical, prose)
    }
    fn text(text: String, searchable: bool) -> Result<Self, AppError> {
        let mut content = Self::default();
        let prose = searchable.then(|| text.clone());
        content.append(text, prose)?;
        Ok(content)
    }
}

fn normalize(
    source: &SourceIdentity,
    id: &str,
    role: Role,
    created: i64,
    content: &Content,
    messages: &mut Vec<NormalizedMessage>,
    budget: &mut Budget,
) -> Result<(), AppError> {
    if created < 0 {
        return Err(AppError::configuration(
            "invalid OpenCode message timestamp",
        ));
    }
    let canonical = content.canonical.join("\n");
    if canonical.trim().is_empty() {
        return Ok(());
    }
    let message = NormalizedMessage {
        id: super::parsers::stable_id("message", &[Provider::Opencode.as_str(), &source.key(), id]),
        ordinal: i64::try_from(messages.len())
            .map_err(|_| AppError::internal("too many OpenCode messages"))?,
        context: crate::message_context::MessageContext::Linear,
        search_projection: Some(
            crate::search_text::project(&role, &content.prose.join("\n")).into_owned(),
        ),
        role,
        content: canonical,
        created_at: Some(created),
    };
    budget.retain(
        std::mem::size_of::<NormalizedMessage>()
            + message.id.len()
            + message.content.len()
            + message.search_projection.as_ref().map_or(0, String::len),
    )?;
    messages.push(message);
    Ok(())
}

fn current(
    connection: &Connection,
    source: &SourceIdentity,
    session: &str,
    budget: &mut Budget,
) -> Result<Vec<NormalizedMessage>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT id,type,time_created,data,length(CAST(data AS BLOB)) AS bytes,
             length(CAST(data AS BLOB))+length(CAST(id AS BLOB))+length(CAST(type AS BLOB)) AS input_bytes
        FROM session_message WHERE session_id=?1 ORDER BY seq,id",
        )
        .map_err(AppError::database)?;
    let mut rows = statement.query([session]).map_err(AppError::database)?;
    let mut messages = Vec::new();
    while let Some(row) = rows.next().map_err(AppError::database)? {
        budget.read(row.get("input_bytes").map_err(AppError::database)?)?;
        let kind: String = row.get("type").map_err(AppError::database)?;
        let kind = Kind::deserialize(
            serde::de::value::StrDeserializer::<serde::de::value::Error>::new(&kind),
        )
        .map_err(|_| AppError::configuration("invalid OpenCode message type"))?;
        let (role, content) = match kind {
            Kind::User => {
                let user: User = sqlite_source::bounded_json(row)?;
                let mut content = Content::text(user.text, true)?;
                for file in user.files {
                    content.append(
                        format!(
                            "File {}: {}\n{}",
                            file.name.unwrap_or_default(),
                            file.uri,
                            file.description.unwrap_or_default()
                        ),
                        None,
                    )?;
                }
                (Role::User, content)
            }
            Kind::Assistant => {
                let mut content = Content::default();
                for part in sqlite_source::bounded_json::<Assistant>(row)?.content {
                    content.part(part)?;
                }
                (Role::Assistant, content)
            }
            Kind::Synthetic => (
                Role::User,
                Content::text(sqlite_source::bounded_json::<Text>(row)?.text, false)?,
            ),
            Kind::System => (
                Role::System,
                Content::text(sqlite_source::bounded_json::<Text>(row)?.text, false)?,
            ),
            Kind::Shell => {
                let shell = sqlite_source::bounded_json::<Shell>(row)?;
                (
                    Role::Tool,
                    Content::text(
                        format!("Command: {}\n{}", shell.command, shell.output),
                        false,
                    )?,
                )
            }
            Kind::Compaction => {
                let compaction = sqlite_source::bounded_json::<Compaction>(row)?;
                (
                    Role::System,
                    Content::text(
                        format!("Compaction: {}\n{}", compaction.summary, compaction.recent),
                        false,
                    )?,
                )
            }
            Kind::AgentSwitched | Kind::ModelSwitched | Kind::Unknown => continue,
        };
        let id: String = row.get("id").map_err(AppError::database)?;
        normalize(
            source,
            &id,
            role,
            row.get("time_created").map_err(AppError::database)?,
            &content,
            &mut messages,
            budget,
        )?;
    }
    Ok(messages)
}

fn legacy(
    connection: &Connection,
    source: &SourceIdentity,
    session: &str,
    budget: &mut Budget,
) -> Result<Vec<NormalizedMessage>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT id,time_created,data,length(CAST(data AS BLOB)) AS bytes,
             length(CAST(data AS BLOB))+length(CAST(id AS BLOB)) AS input_bytes
        FROM message WHERE session_id=?1 ORDER BY time_created,id",
        )
        .map_err(AppError::database)?;
    let mut parts = connection
        .prepare(
            "SELECT data,length(CAST(data AS BLOB)) AS bytes FROM part
        WHERE message_id=?1 AND session_id=?2 ORDER BY id",
        )
        .map_err(AppError::database)?;
    let mut rows = statement.query([session]).map_err(AppError::database)?;
    let mut messages = Vec::new();
    while let Some(row) = rows.next().map_err(AppError::database)? {
        budget.read(row.get("input_bytes").map_err(AppError::database)?)?;
        let id: String = row.get("id").map_err(AppError::database)?;
        let message: LegacyMessage = sqlite_source::bounded_json(row)?;
        let mut content = Content::default();
        let mut fragments = parts
            .query([id.as_str(), session])
            .map_err(AppError::database)?;
        while let Some(fragment) = fragments.next().map_err(AppError::database)? {
            budget.read(fragment.get("bytes").map_err(AppError::database)?)?;
            content.part(sqlite_source::bounded_json(fragment)?)?;
        }
        normalize(
            source,
            &id,
            message.role,
            row.get("time_created").map_err(AppError::database)?,
            &content,
            &mut messages,
            budget,
        )?;
    }
    Ok(messages)
}

pub(super) fn parse(source: &SourceIdentity) -> Result<ParsedFile, AppError> {
    let session = source
        .session
        .as_deref()
        .ok_or_else(|| AppError::configuration("OpenCode requires a session identity"))?;
    let connection = sqlite_source::open(&source.path)?;
    let mut budget = Budget::default();
    let metadata_bytes: i64 = connection
        .query_row(
            "SELECT COALESCE(length(CAST(s.title AS BLOB)),0)
          +COALESCE(length(CAST(s.directory AS BLOB)),0)
          +COALESCE(length(CAST(p.worktree AS BLOB)),0)
          +COALESCE(length(CAST(p.vcs AS BLOB)),0)
        FROM session s JOIN project p ON p.id=s.project_id WHERE s.id=?1",
            [session],
            |row| row.get(0),
        )
        .optional()
        .map_err(AppError::database)?
        .ok_or_else(|| AppError::configuration("OpenCode session disappeared; retry index"))?;
    if metadata_bytes > sqlite_source::MAX_RECORD_BYTES {
        return Err(limits::invalid());
    }
    budget.read(metadata_bytes)?;
    let metadata = connection
        .query_row(
            "SELECT s.title,s.directory,p.worktree,p.vcs,s.time_created,s.time_updated
        FROM session s JOIN project p ON p.id=s.project_id WHERE s.id=?1",
            [session],
            |row| {
                let vcs: Option<String> = row.get("vcs")?;
                Ok(Session {
                    title: row.get("title")?,
                    project: row.get("directory")?,
                    repository: if vcs.as_deref() == Some("git") {
                        Some(row.get("worktree")?)
                    } else {
                        None
                    },
                    created_at: row.get("time_created")?,
                    updated_at: row.get("time_updated")?,
                })
            },
        )
        .optional()
        .map_err(AppError::database)?
        .ok_or_else(|| AppError::configuration("OpenCode session disappeared; retry index"))?;
    if metadata.created_at < 0 || metadata.updated_at < 0 {
        return Err(AppError::configuration(
            "invalid OpenCode session timestamp",
        ));
    }
    let messages = if uses_current(&connection, session)? {
        current(&connection, source, session, &mut budget)?
    } else {
        legacy(&connection, source, session, &mut budget)?
    };
    Ok(ParsedFile {
        conversation: Some(Conversation {
            id: super::parsers::stable_id(
                "conversation",
                &[Provider::Opencode.as_str(), &source.key()],
            ),
            provider: Provider::Opencode,
            source: source.clone(),
            title: Some(metadata.title),
            project: (!metadata.project.is_empty()).then_some(metadata.project),
            repository: metadata.repository,
            created_at: Some(metadata.created_at),
            updated_at: Some(metadata.updated_at),
            messages,
        }),
        malformed_records: 0,
    })
}
