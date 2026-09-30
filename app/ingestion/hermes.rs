//! Native Hermes state.db histories. Sources are read-only; no upstream repairs run.
use super::{Conversation, HistorySource, NormalizedMessage, ParsedFile, SourceIdentity};
use crate::{AppError, provider::Provider, role::Role};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::path::Path;
use std::time::Duration;

#[cfg(test)]
mod tests;

const MAX_MESSAGE_BYTES: i64 = 8 * 1024 * 1024;

fn open(path: &Path) -> Result<Connection, AppError> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(AppError::database)?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(AppError::database)?;
    connection
        .execute_batch("PRAGMA query_only=ON; BEGIN;")
        .map_err(AppError::database)?;
    Ok(connection)
}

pub(super) fn discover(path: &Path) -> Result<Vec<HistorySource>, AppError> {
    let connection = open(path)?;
    let mut statement = connection
        .prepare("SELECT s.id, MAX(s.started_at,COALESCE(s.ended_at,s.started_at),
            COALESCE(s.last_activity_at,s.started_at),COALESCE((SELECT MAX(timestamp) FROM messages WHERE session_id=s.id),s.started_at)) AS activity,
            COALESCE((SELECT SUM(COALESCE(length(CAST(content AS BLOB)),0)+COALESCE(length(CAST(tool_calls AS BLOB)),0)) FROM messages WHERE session_id=s.id),0) AS bytes
            FROM sessions s ORDER BY s.id")
        .map_err(AppError::database)?;
    let sessions = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>("id")?,
                row.get::<_, f64>("activity")?,
                row.get::<_, i64>("bytes")?,
            ))
        })
        .map_err(AppError::database)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::database)?;
    sessions
        .into_iter()
        .map(|(session, activity, estimated_bytes)| {
            let estimated_bytes = u64::try_from(estimated_bytes)
                .map_err(|_| AppError::configuration("invalid Hermes session byte count"))?;
            if session.trim().is_empty() {
                return Err(AppError::configuration("Hermes session identity is empty"));
            }
            let activity_ns = epoch(Some(activity))?
                .and_then(|ms| ms.checked_mul(1_000_000))
                .ok_or_else(|| {
                    AppError::configuration("Hermes session activity is out of range")
                })?;
            Ok(HistorySource {
                identity: SourceIdentity {
                    path: path.to_path_buf(),
                    session: Some(session),
                },
                activity_ns: Some(activity_ns),
                estimated_bytes: Some(estimated_bytes),
            })
        })
        .collect()
}

struct Session {
    title: Option<String>,
    project: Option<String>,
    repository: Option<String>,
    created_at: Option<i64>,
    updated_at: Option<i64>,
}

fn epoch(value: Option<f64>) -> Result<Option<i64>, AppError> {
    value
        .map(|seconds| {
            let milliseconds = seconds * 1000.0;
            if !milliseconds.is_finite()
                || !(0.0..9_223_372_036_854_775_808.0).contains(&milliseconds)
            {
                return Err(AppError::configuration("Hermes timestamp is out of range"));
            }
            #[allow(
                clippy::cast_possible_truncation,
                reason = "finite bounded milliseconds checked above"
            )]
            Ok(milliseconds as i64)
        })
        .transpose()
}

// This is Hermes's native display projection: prefer the live representative of
// a compaction copy, preserve its original position, and exclude model-only rows.
const MESSAGES: &str = "WITH visible AS (
    SELECT *, COALESCE(display_order,MIN(id) OVER (
        PARTITION BY role,content,timestamp,tool_call_id,tool_calls,tool_name
    )) AS position FROM messages
    WHERE session_id=?1 AND (active=1 OR compacted=1)
      AND COALESCE(json_extract(CASE WHEN json_valid(display_metadata)
          THEN display_metadata ELSE '{}' END, '$.model_only'),0)=0
), ranked AS (
    SELECT *, row_number() OVER(PARTITION BY position ORDER BY active DESC,id DESC) AS representative
      FROM visible
)
SELECT id,role,content,tool_calls,timestamp,position,length(CAST(content AS BLOB)) AS content_bytes,
       length(CAST(tool_calls AS BLOB)) AS tool_bytes
  FROM ranked WHERE representative=1 ORDER BY position";

fn read_session(connection: &Connection, session_id: &str) -> Result<Session, AppError> {
    let session = connection.query_row(
        "SELECT title,cwd,git_repo_root,started_at,COALESCE(ended_at,last_activity_at,started_at) AS updated_at
           FROM sessions WHERE id=?1", [session_id], |row| {
            Ok((row.get::<_, Option<String>>("title")?, row.get::<_, Option<String>>("cwd")?,
                row.get::<_, Option<String>>("git_repo_root")?, row.get::<_, Option<f64>>("started_at")?,
                row.get::<_, Option<f64>>("updated_at")?))
        }).optional().map_err(AppError::database)?
        .ok_or_else(|| AppError::configuration("Hermes session disappeared while indexing; retry index"))?;
    Ok(Session {
        title: session.0,
        project: session.1,
        repository: session.2,
        created_at: epoch(session.3)?,
        updated_at: epoch(session.4)?,
    })
}

fn read_messages(
    connection: &Connection,
    source: &SourceIdentity,
    session_id: &str,
) -> Result<Vec<NormalizedMessage>, AppError> {
    let mut statement = connection.prepare(MESSAGES).map_err(AppError::database)?;
    let mut rows = statement.query([session_id]).map_err(AppError::database)?;
    let mut messages = Vec::new();
    while let Some(row) = rows.next().map_err(AppError::database)? {
        for column in ["content_bytes", "tool_bytes"] {
            if row
                .get::<_, Option<i64>>(column)
                .map_err(AppError::database)?
                .is_some_and(|n| n > MAX_MESSAGE_BYTES)
            {
                return Err(AppError::configuration(
                    "Hermes message exceeds the 8 MiB ingestion limit",
                ));
            }
        }
        let raw: Option<String> = row.get("content").map_err(AppError::database)?;
        let (mut content, projection) = match raw.as_deref().and_then(|s| s.strip_prefix("\0json:"))
        {
            Some(json) => serde_json::from_str::<super::json::Content>(json)
                .map_err(|_| AppError::configuration("invalid structured Hermes message content"))?
                .projected()
                .unwrap_or_default(),
            None => (raw.unwrap_or_default(), None),
        };
        let role: Role = row.get("role").map_err(AppError::database)?;
        let tools: Option<String> = row.get("tool_calls").map_err(AppError::database)?;
        // Tool arguments remain canonical context and never become searchable prose.
        let searchable =
            crate::search_text::project(&role, projection.as_deref().unwrap_or(&content))
                .into_owned();
        if let Some(tools) = tools.filter(|s| !s.is_empty() && s != "[]" && s != "null") {
            serde_json::from_str::<serde_json::Value>(&tools)
                .map_err(|_| AppError::configuration("invalid Hermes tool payload"))?;
            content.push_str("\nTool calls: ");
            content.push_str(&tools);
        }
        if content.trim().is_empty() {
            continue;
        }
        let position: i64 = row.get("position").map_err(AppError::database)?;
        // The logical display position survives compaction copying to a new row.
        let id = super::parsers::stable_id(
            "message",
            &[
                Provider::Hermes.as_str(),
                &source.key(),
                &position.to_string(),
            ],
        );
        messages.push(NormalizedMessage {
            id,
            ordinal: i64::try_from(messages.len())
                .map_err(|_| AppError::internal("too many Hermes messages"))?,
            role,
            content,
            search_projection: Some(searchable),
            created_at: epoch(row.get("timestamp").map_err(AppError::database)?)?,
        });
    }
    Ok(messages)
}

pub(super) fn parse(source: &SourceIdentity) -> Result<ParsedFile, AppError> {
    let session_id = source
        .session
        .as_deref()
        .ok_or_else(|| AppError::configuration("Hermes source requires a session identity"))?;
    let connection = open(&source.path)?;
    let session = read_session(&connection, session_id)?;
    let messages = read_messages(&connection, source, session_id)?;
    let title = session.title.or_else(|| {
        messages
            .iter()
            .filter(|m| m.role == Role::User)
            .find_map(|m| {
                crate::search_text::title(m.search_projection.as_deref().unwrap_or(&m.content))
            })
    });
    let updated_at = messages
        .iter()
        .filter_map(|m| m.created_at)
        .chain(session.updated_at)
        .max();
    Ok(ParsedFile {
        conversation: Some(Conversation {
            id: super::parsers::stable_id(
                "conversation",
                &[Provider::Hermes.as_str(), &source.key()],
            ),
            provider: Provider::Hermes,
            source: source.clone(),
            project: session.project,
            repository: session.repository,
            title,
            created_at: session.created_at,
            updated_at,
            messages,
        }),
        malformed_records: 0,
    })
}
