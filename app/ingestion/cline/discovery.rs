//! Discover native task identities and verify their physical dependencies.
use super::models::{Metadata, SdkIdentity};
use super::{Session, invalid, read, timestamp};
use crate::{
    AppError,
    ingestion::{HistorySource, SourceIdentity, source::Dependency, sqlite_source},
};
use rusqlite::{Connection, OptionalExtension};
use std::path::{Path, PathBuf};

pub(in crate::ingestion) fn metadata_path(path: &Path) -> Option<PathBuf> {
    if path.file_name().is_some_and(|n| n == "ui_messages.json") {
        let task = path.parent()?;
        if task.parent()?.file_name().is_some_and(|n| n == "tasks") {
            return Some(task.parent()?.parent()?.join("state/taskHistory.json"));
        }
        return None;
    }
    let name = path.file_name()?.to_str()?.strip_suffix(".messages.json")?;
    Some(path.with_file_name(format!("{name}.json")))
}

pub(super) fn catalog_session(
    connection: &Connection,
    catalog: &Path,
    session: &str,
) -> Result<Session, AppError> {
    struct CatalogRow {
        id: String,
        project: String,
        prompt: Option<String>,
        created: String,
        updated: String,
        artifact: Option<String>,
        subagent: bool,
        metadata: Option<String>,
        metadata_bytes: i64,
    }
    let row = connection.query_row(
        "SELECT session_id,workspace_root,prompt,started_at,updated_at,messages_path,is_subagent,
         CASE WHEN length(CAST(metadata_json AS BLOB))<=?2 THEN metadata_json END AS metadata,
         COALESCE(length(CAST(metadata_json AS BLOB)),0) AS metadata_bytes
         FROM sessions WHERE session_id=?1",
        rusqlite::params![session,sqlite_source::MAX_RECORD_BYTES],
        |row| Ok(CatalogRow {
            id:row.get("session_id")?,project:row.get("workspace_root")?,prompt:row.get("prompt")?,
            created:row.get("started_at")?,updated:row.get("updated_at")?,artifact:row.get("messages_path")?,
            subagent:row.get("is_subagent")?,metadata:row.get("metadata")?,metadata_bytes:row.get("metadata_bytes")?,
        }),
    ).optional().map_err(AppError::database)?.ok_or_else(||invalid("session disappeared; retry index"))?;
    if row.id.trim().is_empty() {
        return Err(invalid("empty catalog session identity"));
    }
    if row.metadata_bytes > sqlite_source::MAX_RECORD_BYTES {
        return Err(invalid("catalog metadata exceeds the ingestion limit"));
    }
    let metadata: Option<Metadata> = row
        .metadata
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|_| invalid("invalid catalog metadata"))?;
    let artifact = if let Some(path) = row.artifact.filter(|s| !s.trim().is_empty()) {
        PathBuf::from(path)
    } else {
        if row.id == "." || row.id == ".." || row.id.contains(['/', '\\']) {
            return Err(invalid("unsafe session artifact identity"));
        }
        catalog
            .parent()
            .ok_or_else(|| invalid("catalog has no directory"))?
            .join(&row.id)
            .join(format!("{}.messages.json", row.id))
    };
    if !artifact.is_absolute() {
        return Err(invalid("catalog artifact path must be absolute"));
    }
    Ok(Session {
        id: row.id,
        project: (!row.project.is_empty()).then_some(row.project),
        title: metadata.and_then(|m| m.title).or(row.prompt),
        created_at: Some(timestamp(&row.created)?),
        updated_at: Some(timestamp(&row.updated)?),
        artifact,
        subagent: row.subagent,
    })
}

pub(in crate::ingestion) fn discover(path: &Path) -> Result<Vec<HistorySource>, AppError> {
    if path.file_name().is_none_or(|n| n != "sessions.db") {
        let mut source = HistorySource::file(path);
        if let Some(metadata) = metadata_path(path) {
            source.dependencies.push(Dependency::Optional(metadata));
        }
        return Ok(vec![source]);
    }
    let connection = sqlite_source::open(path)?;
    let mut statement = connection
        .prepare("SELECT session_id FROM sessions ORDER BY session_id")
        .map_err(AppError::database)?;
    let sessions = statement
        .query_map([], |row| row.get::<_, String>("session_id"))
        .map_err(AppError::database)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::database)?;
    sessions
        .into_iter()
        .map(|id| {
            let session = catalog_session(&connection, path, &id)?;
            Ok(HistorySource {
                identity: SourceIdentity {
                    path: path.into(),
                    session: Some(id),
                },
                activity_ns: session.updated_at.and_then(|t| t.checked_mul(1_000_000)),
                estimated_bytes: Some(session.artifact.metadata().map_err(AppError::io)?.len()),
                dependencies: vec![Dependency::Required(session.artifact)],
            })
        })
        .collect()
}

/// Collapse directory-discovered copies of SDK artifacts already owned by a catalog.
pub(in crate::ingestion) fn sources(
    physical: Vec<HistorySource>,
) -> Result<Vec<HistorySource>, AppError> {
    let expanded = physical
        .into_iter()
        .map(|s| discover(&s.identity.path))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let owned = expanded
        .iter()
        .filter(|s| s.identity.session.is_some())
        .flat_map(|s| s.dependencies.iter())
        .map(|d| d.path().canonicalize().map_err(AppError::io))
        .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
    let sources = expanded
        .into_iter()
        .filter_map(|s| {
            if s.identity.session.is_some() {
                return Some(Ok(s));
            }
            match s.identity.path.canonicalize() {
                Ok(path) if owned.contains(&path) => None,
                Ok(_) => Some(Ok(s)),
                Err(error) => Some(Err(AppError::io(error))),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut groups = std::collections::BTreeMap::<String, Vec<HistorySource>>::new();
    for source in sources {
        let id = if let Some(id) = &source.identity.session {
            id.clone()
        } else if is_ui(&source) {
            source
                .identity
                .path
                .parent()
                .and_then(Path::file_name)
                .and_then(|n| n.to_str())
                .ok_or_else(|| invalid("UI history has no task identity"))?
                .to_owned()
        } else {
            read::<SdkIdentity>(&source.identity.path)?.id
        };
        if id.trim().is_empty() {
            return Err(invalid("empty session identity"));
        }
        groups.entry(id).or_default().push(source);
    }
    groups
        .into_values()
        .map(|mut group| {
            if group.len() == 1 {
                return Ok(group.remove(0));
            }
            if group.len() != 2 || group.iter().filter(|s| is_ui(s)).count() != 1 {
                return Err(invalid(
                    "multiple native sources have the same task identity",
                ));
            }
            let mut sdk = group.remove(
                group
                    .iter()
                    .position(|s| !is_ui(s))
                    .ok_or_else(|| invalid("missing SDK source"))?,
            );
            let legacy = group.remove(0);
            let legacy_bytes = legacy.identity.path.metadata().map_err(AppError::io)?.len();
            sdk.dependencies
                .push(Dependency::Required(legacy.identity.path));
            sdk.dependencies.extend(legacy.dependencies);
            // Include both snapshots in progress estimates, without counting the
            // shared SDK catalog once per logical conversation.
            if let Some(bytes) = sdk.estimated_bytes {
                sdk.estimated_bytes = Some(bytes.saturating_add(legacy_bytes));
            }
            Ok(sdk)
        })
        .collect()
}

fn is_ui(source: &HistorySource) -> bool {
    source
        .identity
        .path
        .file_name()
        .is_some_and(|n| n == "ui_messages.json")
}
