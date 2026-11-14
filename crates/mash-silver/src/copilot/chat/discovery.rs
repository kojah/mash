use super::{
    invalid,
    models::{Catalog, Entry},
    read,
};
use crate::{
    AppError,
    {HistorySource, SourceIdentity, source::Dependency, sqlite_source},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub fn catalog(path: &Path) -> Result<Option<BTreeMap<String, Entry>>, AppError> {
    let db = sqlite_source::open(path)?;
    let exists: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='ItemTable')",
            [],
            |r| r.get(0),
        )
        .map_err(AppError::database)?;
    if !exists {
        return Ok(None);
    }
    let mut query = db.prepare("SELECT value AS data, length(CAST(value AS BLOB)) AS bytes FROM ItemTable WHERE key='chat.ChatSessionStore.index'").map_err(AppError::database)?;
    let mut rows = query.query([]).map_err(AppError::database)?;
    let Some(row) = rows.next().map_err(AppError::database)? else {
        return Ok(None);
    };
    let catalog: Catalog = sqlite_source::bounded_json(row)?;
    if catalog.version != 1 {
        return Err(invalid("unsupported session catalog version"));
    }
    for (id, entry) in &catalog.entries {
        if !entry.is_external
            && (id != &entry.session_id || !valid_id(id) || entry.last_message_date < 0)
        {
            return Err(invalid("invalid catalog session identity or timestamp"));
        }
    }
    Ok(Some(catalog.entries))
}

fn valid_id(id: &str) -> bool {
    !id.trim().is_empty() && id != "." && id != ".." && !id.contains(['/', '\\', '\0'])
}

pub fn directory(db: &Path) -> Result<PathBuf, AppError> {
    let parent = db
        .parent()
        .ok_or_else(|| invalid("missing storage directory"))?;
    Ok(parent.join(
        if parent.file_name().is_some_and(|n| n == "globalStorage") {
            "emptyWindowChatSessions"
        } else {
            "chatSessions"
        },
    ))
}

pub fn transcript(db: &Path, id: &str) -> Result<PathBuf, AppError> {
    let candidates = candidates(db, id)?;
    for candidate in &candidates {
        if candidate.try_exists().map_err(AppError::io)? {
            return Ok(candidate.clone());
        }
    }
    Ok(candidates[1].clone())
}

fn candidates(db: &Path, id: &str) -> Result<Vec<PathBuf>, AppError> {
    if !valid_id(id) {
        return Err(invalid("invalid session filename"));
    }
    let directory = directory(db)?;
    let mut result = vec![
        directory.join(format!("{id}.jsonl")),
        directory.join(format!("{id}.json")),
    ];
    if db
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|n| n == "globalStorage")
    {
        let user = db
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| invalid("missing user-data directory"))?;
        result.push(
            user.join("workspaceStorage/no-workspace/chatSessions")
                .join(format!("{id}.json")),
        );
    }
    Ok(result)
}

pub fn sources(input: Vec<HistorySource>) -> Result<Vec<HistorySource>, AppError> {
    let mut result = Vec::new();
    for source in input {
        if source
            .identity
            .path
            .file_name()
            .is_none_or(|n| n != "state.vscdb")
        {
            result.push(source);
            continue;
        }
        let db = source.identity.path;
        let Some(entries) = catalog(&db)? else {
            continue;
        };
        for (id, entry) in entries {
            if entry.is_external || entry.is_empty {
                continue;
            }
            let file = transcript(&db, &id)?;
            let mut dependencies = vec![Dependency::Required(file.clone())];
            for candidate in candidates(&db, &id)? {
                if candidate == file {
                    break;
                }
                dependencies.push(Dependency::Optional(candidate));
            }
            dependencies.push(Dependency::Optional(
                db.parent().unwrap().join("workspace.json"),
            ));
            result.push(HistorySource {
                content: None,
                identity: SourceIdentity {
                    path: db.clone(),
                    session: Some(format!("chat:{id}")),
                },
                activity_ns: Some(
                    entry
                        .last_message_date
                        .checked_mul(1_000_000)
                        .ok_or_else(|| invalid("timestamp out of range"))?,
                ),
                estimated_bytes: Some(file.metadata().map_err(AppError::io)?.len()),
                dependencies,
            });
        }
    }
    Ok(result)
}

pub fn project(db: &Path) -> Result<Option<String>, AppError> {
    let path = db
        .parent()
        .ok_or_else(|| invalid("missing storage directory"))?
        .join("workspace.json");
    if !path.try_exists().map_err(AppError::io)? {
        return Ok(None);
    }
    let workspace: super::models::Workspace = read(&path)?;
    Ok(workspace.folder.as_deref().and_then(location))
}

/// A workspace URI's folder: local, or on the SSH host, container or
/// Codespace of a remote window, whose chats VS Code stores locally.
pub fn location(uri: &str) -> Option<String> {
    crate::native::location(std::iter::once(uri)).or_else(|| crate::platform::remote_uri_path(uri))
}
