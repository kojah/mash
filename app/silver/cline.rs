//! Read native Cline UI snapshots and SDK canonical message artifacts.
use super::{Conversation, NormalizedMessage, ParsedFile, SourceIdentity};
use crate::{AppError, message_context::MessageContext, provider::Provider, role::Role};
use models::{Agent, Manifest, SdkHistory, TaskHistory, UiHistory};
use serde::de::DeserializeOwned;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
pub(crate) use text::visible_projection;

mod discovery;
mod migration;
mod models;
mod text;
use discovery::catalog_session;
pub(super) use discovery::{metadata_path, sources};
use text::{Text, sdk_text};
#[cfg(test)]
mod tests;

fn invalid(message: &str) -> AppError {
    AppError::configuration(format!("Cline: {message}"))
}

fn read<T: DeserializeOwned>(path: &Path) -> Result<T, AppError> {
    if !path.metadata().map_err(AppError::io)?.is_file() {
        return Err(invalid("snapshot must be a regular file"));
    }
    let file = File::open(path).map_err(AppError::io)?;
    if !file.metadata().map_err(AppError::io)?.is_file() {
        return Err(invalid("snapshot must be a regular file"));
    }
    let mut bytes = Vec::new();
    file.take((super::MAX_RECORD_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(AppError::io)?;
    if bytes.len() > super::MAX_RECORD_BYTES {
        return Err(invalid("snapshot exceeds the 8 MiB ingestion limit"));
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| invalid("invalid or oversized native history snapshot"))
}

fn optional<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, AppError> {
    if !path.try_exists().map_err(AppError::io)? {
        return Ok(None);
    }
    read(path).map(Some)
}

fn timestamp(value: &str) -> Result<i64, AppError> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|t| t.timestamp_millis())
        .map_err(|_| invalid("invalid session timestamp"))
}

struct Session {
    id: String,
    project: Option<String>,
    title: Option<String>,
    created_at: Option<i64>,
    updated_at: Option<i64>,
    artifact: PathBuf,
    subagent: bool,
}

fn sdk(
    path: &Path,
    session: Option<Session>,
    legacy_ui: Option<&Path>,
) -> Result<(Session, Vec<NormalizedMessage>), AppError> {
    let mut history: SdkHistory = read(path)?;
    if history.version != 1 || history.session_id.trim().is_empty() {
        return Err(invalid(
            "unsupported SDK snapshot or empty session identity",
        ));
    }
    let updated_at = timestamp(&history.updated_at)?;
    let mut session = if let Some(session) = session {
        if session.id != history.session_id {
            return Err(invalid("catalog and transcript identities disagree"));
        }
        session
    } else {
        let manifest: Option<Manifest> = metadata_path(path)
            .map(|p| optional(&p))
            .transpose()?
            .flatten();
        if manifest
            .as_ref()
            .is_some_and(|m| m.version != 1 || m.session_id != history.session_id)
        {
            return Err(invalid("manifest and transcript identities disagree"));
        }
        Session {
            id: history.session_id,
            project: manifest.as_ref().map(|m| m.workspace_root.clone()),
            title: manifest
                .as_ref()
                .and_then(|m| m.metadata.as_ref()?.title.clone()),
            created_at: manifest
                .as_ref()
                .map(|m| timestamp(&m.started_at))
                .transpose()?,
            updated_at: Some(updated_at),
            artifact: path.into(),
            subagent: false,
        }
    };
    let searchable = matches!(history.agent, Agent::Lead) && !session.subagent;
    let mut messages = migration::prefix(&mut session, &mut history.messages, legacy_ui)?;
    let mut questions = HashSet::new();
    let mut occurrences = HashMap::<String, u64>::new();
    for message in history.messages {
        if message.ts.is_some_and(|t| t < 0) {
            return Err(invalid("negative message timestamp"));
        }
        let Text {
            canonical,
            mut projection,
            interactive_reply,
        } = sdk_text(message.content, &message.role, &mut questions);
        if canonical.trim().is_empty() {
            continue;
        }
        if !searchable
            || message.metadata.is_some_and(|m| {
                (m.user_run_span == Some(0) && !interactive_reply)
                    || matches!(
                        m.display_role,
                        Some(
                            models::DisplayRole::System
                                | models::DisplayRole::Status
                                | models::DisplayRole::Error
                                | models::DisplayRole::Unknown
                        )
                    )
            })
        {
            projection.clear();
        }
        // Provider IDs/timestamps can be regenerated on resume. Distinct identical
        // real turns retain occurrence identities, independent of transport fields.
        let fingerprint =
            super::parsers::stable_id("cline-content", &[message.role.as_str(), &canonical]);
        let occurrence = occurrences.entry(fingerprint.clone()).or_default();
        let id = super::parsers::stable_id(
            "message",
            &[
                Provider::Cline.as_str(),
                &session.id,
                &fingerprint,
                &occurrence.to_string(),
            ],
        );
        *occurrence += 1;
        messages.push(NormalizedMessage {
            id,
            ordinal: i64::try_from(messages.len()).map_err(|_| invalid("too many messages"))?,
            context: MessageContext::Linear,
            role: message.role,
            content: canonical,
            dialogue_text: Some(projection),
            created_at: message.ts,
        });
    }
    Ok((session, messages))
}

fn ui(path: &Path) -> Result<(Session, Vec<NormalizedMessage>), AppError> {
    let id = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|n| n.to_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| invalid("UI history has no task identity"))?
        .to_owned();
    let history: UiHistory = read(path)?;
    let mut positions = HashMap::<i64, usize>::new();
    let mut unique: Vec<models::UiMessage> = Vec::new();
    for message in history.0 {
        if message.ts < 0 {
            return Err(invalid("negative UI message timestamp"));
        }
        if let Some(&index) = positions.get(&message.ts) {
            if message.seq >= unique[index].seq {
                unique[index] = message;
            }
        } else {
            positions.insert(message.ts, unique.len());
            unique.push(message);
        }
    }
    let mut messages = Vec::new();
    for message in unique {
        let ts = message.ts;
        let (role, content, visible) = message.normalized();
        if content.trim().is_empty() {
            continue;
        }
        let projection = if visible {
            visible_projection(&role, &content)
        } else {
            String::new()
        };
        messages.push(NormalizedMessage {
            id: super::parsers::stable_id(
                "message",
                &[Provider::Cline.as_str(), &id, &ts.to_string()],
            ),
            ordinal: i64::try_from(messages.len()).map_err(|_| invalid("too many messages"))?,
            context: MessageContext::Linear,
            role,
            content,
            dialogue_text: Some(projection),
            created_at: Some(ts),
        });
    }
    let metadata: Option<TaskHistory> = metadata_path(path)
        .map(|p| optional(&p))
        .transpose()?
        .flatten();
    let task = metadata.and_then(|h| h.0.into_iter().find(|t| t.id == id));
    Ok((
        Session {
            id,
            project: task.as_ref().and_then(|t| t.project.clone()),
            title: task.map(|t| t.title),
            created_at: None,
            updated_at: None,
            artifact: path.into(),
            subagent: false,
        },
        messages,
    ))
}

#[cfg(test)]
fn parse(source: &SourceIdentity) -> Result<ParsedFile, AppError> {
    parse_native(source, None)
}

pub(super) fn parse_source(source: &super::HistorySource) -> Result<ParsedFile, AppError> {
    let mut legacy = source
        .dependencies
        .iter()
        .map(super::source::Dependency::path)
        .filter(|p| p.file_name().is_some_and(|n| n == "ui_messages.json"));
    let legacy_ui = legacy.next();
    if legacy.next().is_some() {
        return Err(invalid("multiple legacy UI histories for one SDK source"));
    }
    parse_native(&source.identity, legacy_ui)
}

fn parse_native(source: &SourceIdentity, legacy_ui: Option<&Path>) -> Result<ParsedFile, AppError> {
    let (session, messages) = if let Some(id) = &source.session {
        let connection = super::sqlite_source::open(&source.path)?;
        let session = catalog_session(&connection, &source.path, id)?;
        let artifact = session.artifact.clone();
        sdk(&artifact, Some(session), legacy_ui)?
    } else if source
        .path
        .file_name()
        .is_some_and(|n| n == "ui_messages.json")
    {
        ui(&source.path)?
    } else {
        sdk(&source.path, None, legacy_ui)?
    };
    let title = session
        .title
        .as_deref()
        .and_then(crate::silver::dialogue::title)
        .or_else(|| {
            messages
                .iter()
                .filter(|m| m.role == Role::User)
                .find_map(|m| {
                    crate::silver::dialogue::title(m.dialogue_text.as_deref().unwrap_or_default())
                })
        });
    Ok(ParsedFile {
        conversation: Some(Conversation {
            id: super::parsers::stable_id("session", &[Provider::Cline.as_str(), &session.id]),
            provider: Provider::Cline,
            claude_lineage: None,
            source: source.clone(),
            project: session.project,
            repository: None,
            title,
            created_at: session
                .created_at
                .or_else(|| messages.iter().filter_map(|m| m.created_at).min()),
            updated_at: session
                .updated_at
                .or_else(|| messages.iter().filter_map(|m| m.created_at).max()),
            messages: super::messages::Messages::stage(messages)?,
        }),
        malformed_records: 0,
        first_record_issue: None,
    })
}
