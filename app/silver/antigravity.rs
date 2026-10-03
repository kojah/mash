//! Native per-conversation SQLite snapshots; provider processes are never needed.
use super::native::{location, timestamp};
use super::{Conversation, HistorySource, ParsedFile, SourceIdentity, parsers};
use crate::{AppError, provider::Provider, role::Role, silver::dialogue};
use std::path::Path;

mod history;
mod proto;
mod store;
#[cfg(test)]
pub(super) mod tests;
mod wire;

fn invalid(message: &str) -> AppError {
    AppError::configuration(format!("Antigravity: {message}"))
}

fn validate_filename(path: &Path, root: &store::Root) -> Result<(), AppError> {
    if path.file_stem().and_then(|s| s.to_str()) != Some(root.cascade.as_str()) {
        return Err(invalid("filename and cascade identity disagree"));
    }
    Ok(())
}
pub(super) fn discover(path: &Path) -> Result<Vec<HistorySource>, AppError> {
    let root = store::Store::open(path)?.root()?;
    validate_filename(path, &root)?;
    Ok(vec![HistorySource {
        identity: SourceIdentity {
            path: path.into(),
            session: Some(root.cascade),
        },
        activity_ns: None,
        estimated_bytes: None,
        dependencies: Vec::new(),
    }])
}
pub(super) fn parse(source: &SourceIdentity) -> Result<ParsedFile, AppError> {
    let mut store = store::Store::open(&source.path)?;
    let root = store.root()?;
    validate_filename(&source.path, &root)?;
    if source.session.as_deref() != Some(root.cascade.as_str()) {
        return Err(invalid("logical cascade identity changed"));
    }
    let metadata = store.metadata()?;
    let created_at = timestamp(metadata.created_at.as_ref())?;
    let mut updated_at = created_at;
    let mut messages = Vec::new();
    let mut retained = 0usize;
    store.steps(|index, step| {
        updated_at = updated_at.max(timestamp(
            step.metadata.as_ref().and_then(|m| m.created_at.as_ref()),
        )?);
        if let Some(mut message) = history::normalize(&root, &metadata, index, &step)? {
            retained = retained
                .saturating_add(message.content.capacity())
                .saturating_add(message.dialogue_text.as_ref().map_or(0, String::capacity))
                .saturating_add(512);
            if retained > 32 * 1024 * 1024 {
                return Err(invalid("history exceeds retained replay limit"));
            }
            message.ordinal =
                i64::try_from(messages.len()).map_err(|_| invalid("too many native messages"))?;
            messages.push(message);
        }
        Ok(())
    })?;
    let title = messages
        .iter()
        .filter(|m| m.role == Role::User)
        .find_map(|m| dialogue::title(m.dialogue_text.as_deref().unwrap_or_default()));
    Ok(ParsedFile {
        conversation: Some(Conversation {
            id: parsers::stable_id("session", &[Provider::Antigravity.as_str(), &root.cascade]),
            provider: Provider::Antigravity,
            title_origin: None,
            claude_lineage: None,
            source: source.clone(),
            project: location(metadata.workspaces.iter().map(|w| w.project.as_str())),
            repository: location(metadata.workspaces.iter().map(|w| w.repository.as_str())),
            title,
            created_at,
            updated_at,
            messages: super::messages::Messages::stage(messages)?,
        }),
        malformed_records: 0,
        first_record_issue: None,
    })
}
