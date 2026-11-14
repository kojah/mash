//! Native per-chat SQLite snapshots; provider processes are never needed.
use super::native::{location, timestamp};
use super::{Chat, HistorySource, ParsedFile, SourceIdentity, diagnostics::RecordIssue, parsers};
use crate::{AppError, dialogue, provider::Provider, role::Role};
use std::path::Path;

pub mod history;
pub mod proto;
pub mod store;
pub mod wire;

fn invalid(message: &str) -> AppError {
    AppError::configuration(format!("Antigravity: {message}"))
}

fn validate_filename(path: &Path, root: &store::Root) -> Result<(), AppError> {
    if path.file_stem().and_then(|s| s.to_str()) != Some(root.cascade.as_str()) {
        return Err(invalid("filename and cascade identity disagree"));
    }
    Ok(())
}
pub fn discover(path: &Path) -> Result<Vec<HistorySource>, AppError> {
    let session = match store::Store::open(path)? {
        store::Opened::Store(store) => {
            let root = store.root()?;
            validate_filename(path, &root)?;
            root.cascade
        }
        // Keep the source so indexing reports it, under the identity a
        // readable snapshot of the same file would have.
        store::Opened::Unsupported(_) => path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.trim().is_empty())
            .ok_or_else(|| invalid("native database filename is not a cascade identity"))?
            .to_owned(),
    };
    Ok(vec![HistorySource {
        content: None,
        identity: SourceIdentity {
            path: path.into(),
            session: Some(session),
        },
        activity_ns: None,
        estimated_bytes: None,
        dependencies: Vec::new(),
    }])
}
/// An unsupported snapshot rejects its own source; the pass stays incomplete
/// and the source's previous contribution is retained.
fn rejected(source: &SourceIdentity, reason: store::Unsupported) -> ParsedFile {
    tracing::warn!(target: "mash", provider = Provider::Antigravity.as_str(),
        source_fingerprint = %blake3::hash(source.path.as_os_str().as_encoded_bytes()).to_hex(),
        %reason, "native snapshot not supported");
    ParsedFile {
        chat: None,
        malformed_records: 0,
        first_record_issue: Some(RecordIssue::unsupported(
            Provider::Antigravity,
            &source.path,
        )),
    }
}
pub fn parse(source: &SourceIdentity) -> Result<ParsedFile, AppError> {
    let mut store = match store::Store::open(&source.path)? {
        store::Opened::Store(store) => store,
        store::Opened::Unsupported(reason) => return Ok(rejected(source, reason)),
    };
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
        chat: Some(Chat {
            id: parsers::stable_id("session", &[Provider::Antigravity.as_str(), &root.cascade]),
            provider: Provider::Antigravity,
            title_origin: None,
            timestamp_origins: None,
            claude_lineage: None,
            source: source.clone(),
            // The deprecated workspace records and their replacement URI list
            // name the same folders; either may be present.
            project: location(
                metadata
                    .workspaces
                    .iter()
                    .map(|w| w.project.as_str())
                    .chain(metadata.workspace_uris.iter().map(String::as_str)),
            ),
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
