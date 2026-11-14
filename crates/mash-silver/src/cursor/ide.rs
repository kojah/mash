//! Native IDE v18 composer roots own the ordered visible bubble history.
use super::{Chat, HistorySource, ParsedFile, SourceIdentity, proto};
use crate::{AppError, provider::Provider, records, role::Role};
use base64::{Engine, engine::general_purpose::STANDARD};
use prost::Message;
use std::path::Path;

mod history;
pub mod models;
pub mod store;
use models::Root;
use store::Store;

fn invalid(message: &str) -> AppError {
    AppError::configuration(format!("Cursor IDE: {message}"))
}

pub fn discover(path: &Path) -> Result<Vec<HistorySource>, AppError> {
    let store = Store::open(path)?;
    Ok(store
        .sessions()?
        .into_iter()
        .map(|id| HistorySource {
            content: None,
            identity: SourceIdentity {
                path: path.into(),
                session: Some(format!("ide:{id}")),
            },
            activity_ns: None,
            estimated_bytes: None,
            dependencies: Vec::new(),
        })
        .collect())
}

fn timestamp(value: Option<i64>) -> Result<Option<i64>, AppError> {
    if value.is_some_and(|v| v < 0) {
        return Err(invalid("invalid native timestamp"));
    }
    Ok(value)
}
pub fn state(root: &Root) -> Result<proto::Root, AppError> {
    let Some(value) = root.conversation_state.as_deref() else {
        return Ok(proto::Root::default());
    };
    let bytes = if let Some(value) = value.strip_prefix('~') {
        STANDARD
            .decode(value)
            .map_err(|_| invalid("invalid native state encoding"))?
    } else {
        hex::decode(value).map_err(|_| invalid("invalid native state encoding"))?
    };
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(invalid("oversized native state"));
    }
    super::store::inspect(&bytes, &[21], &mut 65_536)?;
    proto::Root::decode(bytes.as_slice()).map_err(|_| invalid("invalid native state protobuf"))
}

fn project(root: &Root, state: &proto::Root) -> Option<String> {
    let mut paths = Vec::new();
    if let Some(uri) = root
        .workspace_identifier
        .as_ref()
        .and_then(|w| w.uri.as_ref())
    {
        if uri.scheme != "file"
            || uri
                .authority
                .as_deref()
                .is_some_and(|a| !a.is_empty() && a != "localhost")
        {
            return None;
        }
        paths.push(crate::platform::uri_path(&uri.path)?);
    }
    for uri in &state.workspaces {
        paths.push(crate::platform::file_uri_path(uri)?);
    }
    super::unique(paths.into_iter())
}

fn repository(root: &Root, state: &proto::Root) -> Option<String> {
    let paths = root
        .tracked_git_repos
        .iter()
        .map(|r| &r.path)
        .chain(state.repositories.iter().map(|r| &r.path))
        .collect::<Vec<_>>();
    // Native paths, `/`-rooted ones (WSL and remote repositories, which
    // Windows does not call absolute), or Windows drive paths read elsewhere
    // (Windows-mounted histories under WSL, imported indexes).
    if paths.iter().any(|p| {
        !(Path::new(p).is_absolute() || p.starts_with('/') || crate::platform::is_drive_path(p))
    }) {
        return None;
    }
    super::unique(paths.into_iter().cloned())
}

pub fn parse(source: &SourceIdentity) -> Result<ParsedFile, AppError> {
    let id = source
        .session
        .as_deref()
        .and_then(|s| s.strip_prefix("ide:"))
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| invalid("invalid logical source identity"))?;
    let mut store = Store::open(&source.path)?;
    let root: Root = store.json(&format!("composerData:{id}"))?;
    if root.version != 18 || root.composer_id != id {
        return Err(invalid(
            "unsupported root version or mismatched composer identity",
        ));
    }
    if root
        .subagent_info
        .as_ref()
        .is_some_and(|s| s.parent_composer_id.trim().is_empty())
    {
        return Err(invalid("invalid native subagent metadata"));
    }
    let state = state(&root)?;
    let messages = history::read(&mut store, &root, id)?;
    let created_at = timestamp(root.created_at)?.or(super::evidence::timestamp(state.started_at)?);
    let updated_at = timestamp(root.last_updated_at)?
        .or_else(|| messages.iter().filter_map(|m| m.created_at).max())
        .or(created_at);
    let project = project(&root, &state);
    let repository = repository(&root, &state);
    let title = root
        .name
        .as_deref()
        .and_then(crate::dialogue::title)
        .or_else(|| {
            messages
                .iter()
                .filter(|m| m.role == Role::User)
                .find_map(|m| {
                    crate::dialogue::title(m.dialogue_text.as_deref().unwrap_or_default())
                })
        });
    Ok(ParsedFile {
        chat: Some(Chat {
            id: crate::parsers::stable_id("session", &[Provider::Cursor.as_str(), "ide", id]),
            provider: Provider::Cursor,
            title_origin: None,
            timestamp_origins: None,
            claude_lineage: None,
            source: source.clone(),
            project,
            repository,
            title,
            created_at,
            updated_at,
            messages: super::super::messages::Messages::stage(messages)?,
        }),
        malformed_records: 0,
        first_record_issue: None,
    })
}
