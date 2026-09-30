//! Native encrypted Cascade snapshots; no hook exports or provider process needed.
use super::{Conversation, ParsedFile, SourceIdentity, parsers};
use crate::{AppError, provider::Provider, role::Role, search_text};
use std::{collections::BTreeSet, path::Path};

mod history;
mod proto;
mod reader;
#[cfg(test)]
mod tests;

fn invalid(message: &str) -> AppError {
    AppError::configuration(format!("Windsurf: {message}"))
}

fn timestamp(value: Option<&proto::Timestamp>) -> Result<Option<i64>, AppError> {
    value
        .map(|t| {
            if !(0..=253_402_300_799).contains(&t.seconds) || !(0..1_000_000_000).contains(&t.nanos)
            {
                return Err(invalid("invalid native timestamp"));
            }
            t.seconds
                .checked_mul(1000)
                .and_then(|v| v.checked_add(i64::from(t.nanos) / 1_000_000))
                .ok_or_else(|| invalid("native timestamp out of range"))
        })
        .transpose()
}

fn location<'a>(uris: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut paths = BTreeSet::new();
    for uri in uris.filter(|u| !u.is_empty()) {
        let path = reqwest::Url::parse(uri).ok()?.to_file_path().ok()?;
        paths.insert(path.to_str()?.to_owned());
    }
    if paths.len() == 1 {
        paths.pop_first()
    } else {
        None
    }
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
    let title = root
        .title
        .as_deref()
        .and_then(search_text::title)
        .or_else(|| {
            messages
                .iter()
                .filter(|m| m.role == Role::User)
                .find_map(|m| {
                    search_text::title(m.search_projection.as_deref().unwrap_or_default())
                })
        });
    let workspaces = root
        .metadata
        .as_ref()
        .map(|m| m.workspaces.as_slice())
        .unwrap_or_default();
    Ok(ParsedFile {
        conversation: Some(Conversation {
            id: parsers::stable_id("session", &[Provider::Windsurf.as_str(), session]),
            provider: Provider::Windsurf,
            source: SourceIdentity::file(path),
            project: location(workspaces.iter().map(|w| w.project.as_str())),
            repository: location(workspaces.iter().map(|w| w.repository.as_str())),
            title,
            created_at,
            updated_at,
            messages,
        }),
        malformed_records: 0,
    })
}
