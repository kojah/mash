//! Regular VS Code Chat snapshots and append-only mutation logs, distinct from CLI events.
use super::super::{
    Conversation, HistorySource, NormalizedMessage, ParsedFile, messages::Builder, records,
};
use crate::{AppError, message_context::MessageContext, provider::Provider, role::Role};
use serde::de::DeserializeOwned;
use std::{collections::HashSet, fs::File, io::Read, path::Path};

mod discovery;
mod models;
mod replay;
#[cfg(test)]
mod tests;
pub(super) fn sources(input: Vec<HistorySource>) -> Result<Vec<HistorySource>, AppError> {
    discovery::sources(input)
}

fn invalid(message: &str) -> AppError {
    AppError::configuration(format!("Copilot Chat: {message}"))
}

fn read<T: DeserializeOwned>(path: &Path) -> Result<T, AppError> {
    if !path.metadata().map_err(AppError::io)?.is_file() {
        return Err(invalid("snapshot must be a regular file"));
    }
    let mut bytes = Vec::new();
    let file = File::open(path).map_err(AppError::io)?;
    if !file.metadata().map_err(AppError::io)?.is_file() {
        return Err(invalid("snapshot must be a regular file"));
    }
    file.take((super::super::MAX_RECORD_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(AppError::io)?;
    if bytes.len() > super::super::MAX_RECORD_BYTES {
        return Err(invalid("snapshot exceeds the 8 MiB ingestion limit"));
    }
    records::decode(&bytes)
}

pub(super) fn parse(source: &HistorySource) -> Result<ParsedFile, AppError> {
    let db = &source.identity.path;
    let id = source
        .identity
        .session
        .as_deref()
        .and_then(|s| s.strip_prefix("chat:"))
        .ok_or_else(|| invalid("missing session identity"))?;
    let entry = discovery::catalog(db)?
        .and_then(|mut e| e.remove(id))
        .filter(|e| !e.is_external && !e.is_empty)
        .ok_or_else(AppError::source_changed)?;
    let path = discovery::transcript(db, id)?;
    if !source
        .dependencies
        .iter()
        .any(|d| matches!(d, super::super::source::Dependency::Required(p) if p == &path))
    {
        return Err(AppError::source_changed());
    }
    let session: models::Snapshot = if path.extension().is_some_and(|e| e == "jsonl") {
        replay::read(&path)?
    } else {
        read(&path)?
    };
    if session.session_id != id
        || !matches!(session.version, None | Some(2 | 3))
        || session.creation_date < 0
    {
        return Err(invalid(
            "unsupported snapshot version, session identity or timestamp",
        ));
    }
    let project = session
        .working_directory
        .as_deref()
        .or(entry.working_directory.as_deref())
        .and_then(|s| super::super::native::location(std::iter::once(s)))
        .or(discovery::project(db)?);
    let repository = session
        .repo_data
        .and_then(|r| r.remote_url)
        .filter(|s| !s.is_empty());
    let messages = normalize(id, session.requests)?;
    let summary = messages.summary()?;
    Ok(ParsedFile {
        conversation: Some(Conversation {
            id: super::super::parsers::stable_id("session", &["copilot-chat", id]),
            provider: Provider::Copilot,
            source: source.identity.clone(),
            claude_lineage: None,
            title: session
                .custom_title
                .or(session.computed_title)
                .filter(|s| !s.trim().is_empty())
                .or(Some(entry.title).filter(|s| !s.trim().is_empty()))
                .or(summary.title),
            created_at: Some(session.creation_date),
            updated_at: Some(entry.last_message_date.max(summary.updated_at.unwrap_or(0))),
            project,
            repository,
            messages,
        }),
        malformed_records: 0,
        first_record_issue: None,
    })
}

fn normalize(
    id: &str,
    requests: Vec<models::Turn>,
) -> Result<super::super::messages::Messages, AppError> {
    let mut messages = Builder::default();
    let mut identities = HashSet::new();
    let mut ordinal = 0i64;
    for request in requests {
        if request.request_id.trim().is_empty()
            || !identities.insert(request.request_id.clone())
            || request.timestamp.is_some_and(|t| t < 0)
            || request.response_timestamp.is_some_and(|t| t < 0)
        {
            return Err(invalid("invalid request identity or timestamp"));
        }
        let visible = !request.hidden_from_transcript
            && !request.is_hidden
            && request.agent.as_ref().is_none_or(models::Agent::copilot);
        let user = request.message.text();
        let mut rows = vec![(
            Role::User,
            user.clone(),
            if visible && !request.prompt_hidden && !request.is_system_initiated {
                crate::silver::dialogue::visible(&Role::User, &user).into_owned()
            } else {
                String::new()
            },
            request.timestamp,
        )];
        let mut canonical = Vec::new();
        let mut prose = Vec::new();
        for part in request
            .response
            .map_or_else(Vec::new, models::Response::parts)
        {
            let (text, projection) = part.text();
            if !text.is_empty() {
                if !projection.is_empty() && visible {
                    prose.push(projection);
                }
                canonical.push(text);
            }
        }
        rows.push((
            Role::Assistant,
            canonical.join("\n"),
            crate::silver::dialogue::visible(&Role::Assistant, &prose.join("\n")).into_owned(),
            request.response_timestamp.or(request.timestamp),
        ));
        for (role, content, projection, created_at) in rows {
            if content.trim().is_empty() {
                continue;
            }
            messages.push(NormalizedMessage {
                id: super::super::parsers::stable_id(
                    "message",
                    &["copilot-chat", id, &request.request_id, role.as_str()],
                ),
                ordinal,
                context: MessageContext::Linear,
                role,
                content,
                dialogue_text: Some(projection),
                created_at,
            })?;
            ordinal = ordinal
                .checked_add(1)
                .ok_or_else(|| invalid("too many messages"))?;
        }
    }
    messages.finish()
}
