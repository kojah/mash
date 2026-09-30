//! The native legacy resume marker separates imported model input from new turns.
pub(super) const RESUME_WARNING: &str = "Warning: this is a legacy conversation, which means tool names may have changed. Please use the most up-to-date tools you are aware of.";

use super::{Session, invalid, models, ui};
use crate::{AppError, ingestion::NormalizedMessage};
use std::path::Path;

pub(super) fn prefix(
    session: &mut Session,
    sdk_messages: &mut Vec<models::SdkMessage>,
    legacy_ui: Option<&Path>,
) -> Result<Vec<NormalizedMessage>, AppError> {
    let messages = if let Some(path) = legacy_ui
        && let Some(boundary) = sdk_messages
            .iter()
            .position(models::SdkMessage::legacy_boundary)
    {
        let (legacy, messages) = ui(path)?;
        if legacy.id != session.id {
            return Err(invalid("legacy UI and SDK identities disagree"));
        }
        sdk_messages.drain(..=boundary);
        if session.project.is_none() {
            session.project = legacy.project;
        }
        if session.title.is_none() {
            session.title = legacy.title;
        }
        if let Some(earliest) = messages.iter().filter_map(|m| m.created_at).min() {
            session.created_at = Some(session.created_at.map_or(earliest, |t| t.min(earliest)));
        }
        messages
    } else {
        Vec::new()
    };
    Ok(messages)
}
