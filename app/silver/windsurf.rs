//! Native encrypted Cascade snapshots; no hook exports or provider process needed.
use super::native::{location, timestamp};
use super::{Conversation, ParsedFile, SourceIdentity, parsers};
use crate::{AppError, provider::Provider, role::Role, silver::dialogue};
use std::path::Path;

mod history;
mod proto;
mod reader;
#[cfg(test)]
mod tests;

fn invalid(message: &str) -> AppError {
    AppError::configuration(format!("Windsurf: {message}"))
}

pub(super) fn parse(path: &Path) -> Result<ParsedFile, AppError> {
    let root = reader::read(path)?;
    let session = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| invalid("invalid native filename"))?;
    if session.is_empty() || root.id != session || root.id.len() > 1024 {
        return Err(invalid("native file and trajectory identities disagree"));
    }
    let created_at = timestamp(root.metadata.as_ref().and_then(|m| m.created_at.as_ref()))?;
    let mut updated_at = created_at;
    for step in &root.steps {
        updated_at = updated_at.max(timestamp(
            step.metadata.as_ref().and_then(|m| m.created_at.as_ref()),
        )?);
    }
    let messages = history::messages(&root, session)?;
    let title = root.title.as_deref().and_then(dialogue::title).or_else(|| {
        messages
            .iter()
            .filter(|m| m.role == Role::User)
            .find_map(|m| dialogue::title(m.dialogue_text.as_deref().unwrap_or_default()))
    });
    let workspaces = root
        .metadata
        .as_ref()
        .map_or(&[][..], |m| m.workspaces.as_slice());
    Ok(ParsedFile {
        conversation: Some(Conversation {
            id: parsers::stable_id("session", &[Provider::Windsurf.as_str(), session]),
            provider: Provider::Windsurf,
            title_origin: None,
            timestamp_origins: None,
            claude_lineage: None,
            source: SourceIdentity::file(path),
            project: location(workspaces.iter().map(|w| w.project.as_str())),
            repository: location(workspaces.iter().map(|w| w.repository.as_str())),
            title,
            created_at,
            updated_at,
            messages: super::messages::Messages::stage(messages)?,
        }),
        malformed_records: 0,
        first_record_issue: None,
    })
}
