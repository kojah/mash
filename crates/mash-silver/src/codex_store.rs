//! Codex paginated history: read-only `thread_history_1.sqlite` + `state_5.sqlite`.
//! The metadata database is a documented sibling dependency, never a write target.
use super::{Chat, HistorySource, NormalizedMessage, ParsedFile, SourceIdentity, sqlite_source};
use crate::{AppError, provider::Provider};
use rusqlite::{Connection, OptionalExtension};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub mod items;
// Codex stores whole file-change/tool results as single items. Real histories
// contain valid 13 MiB patches; keep a finite bound without dropping that context.
// Structural JSON limits and normalized-message disk spooling still apply.
pub const MAX_ITEM_BYTES: usize = 64 * 1024 * 1024;

fn invalid(message: &str) -> AppError {
    AppError::configuration(format!("Codex paginated history: {message}"))
}

pub fn state_path(path: &Path) -> PathBuf {
    path.with_file_name("state_5.sqlite")
}

struct Metadata {
    title: String,
    cwd: String,
    repository: Option<String>,
    created: i64,
    updated: i64,
    created_field: super::enrichment::TimestampField,
    updated_field: super::enrichment::TimestampField,
    rollout: PathBuf,
    mode: Mode,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Legacy,
    Paginated,
}

fn metadata(connection: &Connection, id: &str) -> Result<Metadata, AppError> {
    let (bytes, mode): (i64, String) = connection
        .query_row(
            "SELECT length(CAST(title AS BLOB))+length(CAST(cwd AS BLOB))+
         COALESCE(length(CAST(git_origin_url AS BLOB)),0)+length(CAST(rollout_path AS BLOB)),history_mode
         FROM threads WHERE id=?1",
            [id],
            |row| Ok((row.get(0)?,row.get(1)?)),
        )
        .optional()
        .map_err(AppError::database)?
        .ok_or_else(|| {
            invalid("thread metadata is missing; retry after Codex finishes migration")
        })?;
    if bytes > sqlite_source::MAX_RECORD_BYTES {
        return Err(invalid("thread metadata exceeds the ingestion limit"));
    }
    let mode = match mode.as_str() {
        "legacy" => Mode::Legacy,
        "paginated" => Mode::Paginated,
        _ => {
            return Err(invalid(
                "unsupported history mode; source selection is unverified",
            ));
        }
    };
    connection.query_row(
        "SELECT title,cwd,git_origin_url,COALESCE(created_at_ms,created_at*1000),
         COALESCE(updated_at_ms,updated_at*1000),rollout_path,created_at_ms IS NOT NULL,updated_at_ms IS NOT NULL FROM threads WHERE id=?1",
        [id], |row| Ok(Metadata {
            title:row.get(0)?, cwd:row.get(1)?, repository:row.get(2)?,
            created:row.get(3)?,updated:row.get(4)?,rollout:PathBuf::from(row.get::<_,String>(5)?),
            created_field: if row.get::<_,bool>(6)? { super::enrichment::TimestampField::ThreadCreatedAtMs } else { super::enrichment::TimestampField::ThreadCreatedAt },
            updated_field: if row.get::<_,bool>(7)? { super::enrichment::TimestampField::ThreadUpdatedAtMs } else { super::enrichment::TimestampField::ThreadUpdatedAt },
            mode,
        })).map_err(AppError::database)
}

/// What the store records about one thread's rows.
struct Stored {
    bytes: i64,
    /// Absent for a store that predates Codex's per-item update counter.
    counters: Option<[i64; 5]>,
}

/// One row per thread with stored items, or only `thread` when given. Codex
/// gives an item a new `updated_at_ordinal` whenever it rewrites it, so with
/// the row count, byte total and highest ordinals a thread whose summary is
/// unchanged has the rows that were read before.
fn stored(history: &Connection, thread: Option<&str>) -> Result<Vec<(String, Stored)>, AppError> {
    let tracked: bool = history
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('thread_items') WHERE name='updated_at_ordinal')",
            [],
            |row| row.get(0),
        )
        .map_err(AppError::database)?;
    let updated = if tracked { "updated_at_ordinal" } else { "0" };
    let filter = if thread.is_some() {
        "WHERE thread_id=?1"
    } else {
        ""
    };
    let mut statement = history
        .prepare(&format!(
            "WITH history AS (
                SELECT thread_id,length(CAST(item_json AS BLOB)) AS bytes,rollout_ordinal AS ordinal,{updated} AS updated,0 AS realtime
                FROM thread_items {filter}
                UNION ALL SELECT thread_id,length(CAST(item_json AS BLOB)),rollout_ordinal,0,1
                FROM thread_realtime_items {filter})
             SELECT thread_id,SUM(bytes),COUNT(*),SUM(realtime),MAX(ordinal),MAX(updated),SUM(updated)
             FROM history GROUP BY thread_id ORDER BY thread_id"
        ))
        .map_err(AppError::database)?;
    let rows = statement
        .query_map(rusqlite::params_from_iter(thread), |row| {
            let counters = [
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ];
            Ok((
                row.get::<_, String>(0)?,
                Stored {
                    bytes: row.get(1)?,
                    counters: tracked.then_some(counters),
                },
            ))
        })
        .map_err(AppError::database)?;
    rows.collect::<Result<_, _>>().map_err(AppError::database)
}

/// Everything `parse` reads for a thread, reduced to a token: its stored rows
/// as Codex counts them and the metadata row beside them.
fn content(stored: &Stored, meta: &Metadata) -> Option<super::SessionContent> {
    let counters = stored.counters?;
    let mut hash = blake3::Hasher::new();
    for value in [stored.bytes, meta.created, meta.updated]
        .into_iter()
        .chain(counters)
    {
        hash.update(&value.to_le_bytes());
    }
    hash.update(&[
        u8::from(meta.created_field == super::enrichment::TimestampField::ThreadCreatedAtMs),
        u8::from(meta.updated_field == super::enrichment::TimestampField::ThreadUpdatedAtMs),
        u8::from(meta.repository.is_some()),
    ]);
    for text in [
        meta.title.as_bytes(),
        meta.cwd.as_bytes(),
        meta.repository.as_deref().unwrap_or_default().as_bytes(),
        meta.rollout.as_os_str().as_encoded_bytes(),
    ] {
        hash.update(&(text.len() as u64).to_le_bytes());
        hash.update(text);
    }
    Some(super::SessionContent {
        bytes: stored.bytes,
        // The prefix keeps a thread token distinct from a fingerprint of files.
        token: format!("thread:{}", hash.finalize().to_hex()),
    })
}

/// Prefer the authoritative paginated source only for threads with stored items.
/// JSONL paths come from state metadata, never filenames or text matching.
pub fn sources(physical: Vec<HistorySource>) -> Result<Vec<HistorySource>, AppError> {
    let mut sources = Vec::new();
    let mut replaced = BTreeSet::new();
    for source in &physical {
        if source
            .identity
            .path
            .file_name()
            .is_none_or(|n| n != "thread_history_1.sqlite")
        {
            continue;
        }
        let path = &source.identity.path;
        let history = sqlite_source::open(path)?;
        let state_file = state_path(path);
        let state = sqlite_source::open(&state_file)?;
        for (id, stored) in stored(&history, None)? {
            if id.is_empty() {
                return Err(invalid("empty thread identity"));
            }
            let meta = metadata(&state, &id)?;
            if meta.mode != Mode::Paginated {
                continue;
            }
            sources.push(paginated(path, &state_file, id, &stored, &meta)?);
            replaced.insert(meta.rollout);
        }
    }
    sources.extend(physical.into_iter().filter(|s| {
        s.identity.path.extension().is_some_and(|e| e == "jsonl")
            && !replaced.contains(&s.identity.path)
    }));
    Ok(sources)
}

fn paginated(
    path: &Path,
    state_file: &Path,
    id: String,
    stored: &Stored,
    meta: &Metadata,
) -> Result<HistorySource, AppError> {
    let activity = meta
        .updated
        .checked_mul(1_000_000)
        .filter(|v| *v >= 0)
        .ok_or_else(|| invalid("thread timestamp is out of range"))?;
    let mut wal = state_file.as_os_str().to_os_string();
    wal.push("-wal");
    Ok(HistorySource {
        identity: SourceIdentity {
            path: path.to_path_buf(),
            session: Some(id),
        },
        activity_ns: Some(activity),
        estimated_bytes: Some(
            u64::try_from(stored.bytes).map_err(|_| invalid("invalid thread byte count"))?,
        ),
        content: content(stored, meta),
        dependencies: vec![
            super::source::Dependency::Required(state_file.to_path_buf()),
            super::source::Dependency::Optional(wal.into()),
        ],
    })
}

/// One thread's source, selected as `sources` selects it but without scanning
/// the store: its paginated history when it has stored items, otherwise the
/// rollout file its metadata names. `None` when the store does not know it.
pub fn session_source(path: &Path, id: &str) -> Result<Option<HistorySource>, AppError> {
    let state_file = state_path(path);
    let state = sqlite_source::open(&state_file)?;
    let known: bool = state
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM threads WHERE id=?1)",
            [id],
            |row| row.get(0),
        )
        .map_err(AppError::database)?;
    if !known {
        return Ok(None);
    }
    let meta = metadata(&state, id)?;
    let stored = if meta.mode == Mode::Paginated {
        stored(&sqlite_source::open(path)?, Some(id))?.pop()
    } else {
        None
    };
    Ok(Some(match stored {
        Some((_, stored)) => paginated(path, &state_file, id.to_owned(), &stored, &meta)?,
        None => HistorySource::file(meta.rollout),
    }))
}

pub fn parse(source: &SourceIdentity) -> Result<ParsedFile, AppError> {
    let session = source
        .session
        .as_deref()
        .ok_or_else(|| invalid("missing thread identity"))?;
    let history = sqlite_source::open(&source.path)?;
    let state = sqlite_source::open(&state_path(&source.path))?;
    let meta = metadata(&state, session)?;
    if meta.mode != Mode::Paginated || meta.created < 0 || meta.updated < 0 {
        return Err(invalid("thread metadata changed or has invalid timestamps"));
    }
    let mut statement = history.prepare(
        "SELECT turn_id,item_id,created_at_ms,item_json AS data,length(CAST(item_json AS BLOB)) AS bytes,0 AS realtime,rollout_ordinal
         FROM thread_items WHERE thread_id=?1
         UNION ALL
         SELECT '',item_id,created_at_ms,item_json,length(CAST(item_json AS BLOB)),1,rollout_ordinal
         FROM thread_realtime_items WHERE thread_id=?1
         ORDER BY rollout_ordinal,item_id"
    ).map_err(AppError::database)?;
    let mut rows = statement.query([session]).map_err(AppError::database)?;
    let mut messages = super::messages::Builder::default();
    while let Some(row) = rows.next().map_err(AppError::database)? {
        let id: String = row.get("item_id").map_err(AppError::database)?;
        let turn: String = row.get("turn_id").map_err(AppError::database)?;
        let created: i64 = row.get("created_at_ms").map_err(AppError::database)?;
        if id.is_empty() || created < 0 {
            return Err(invalid("item identity or timestamp is invalid"));
        }
        let realtime: bool = row.get("realtime").map_err(AppError::database)?;
        let text = if realtime {
            let item: items::RealtimeItem =
                sqlite_source::bounded_json_with_limit(row, MAX_ITEM_BYTES)?;
            if item.id() != id {
                return Err(invalid("realtime item identity disagrees with its row"));
            }
            item.normalize()?
        } else {
            let item: items::Item = sqlite_source::bounded_json_with_limit(row, MAX_ITEM_BYTES)?;
            if item.id() != id {
                return Err(invalid("item identity disagrees with its row"));
            }
            item.normalize()?
        };
        messages.push(NormalizedMessage {
            id: super::parsers::stable_id(
                if realtime {
                    "realtime-message"
                } else {
                    "message"
                },
                &[Provider::Codex.as_str(), session, &turn, &id],
            ),
            ordinal: i64::try_from(messages.len()).map_err(|_| invalid("too many items"))?,
            context: crate::message_context::MessageContext::Linear,
            dialogue_text: Some(super::dialogue::visible(&text.role, &text.dialogue).into_owned()),
            role: text.role,
            content: text.canonical,
            created_at: Some(created),
            attachments: text.attachments,
            tools: text.tools,
        })?;
    }
    Ok(ParsedFile {
        chat: Some(Chat {
            id: session.into(),
            provider: Provider::Codex,
            source: source.clone(),
            claude_lineage: None,
            title_origin: (!meta.title.is_empty()).then_some(
                super::enrichment::Origin::SourceMetadata {
                    field: super::enrichment::SourceField::ThreadTitle,
                },
            ),
            timestamp_origins: Some(super::enrichment::Timestamps {
                created_at: Some(super::enrichment::TimestampOrigin::SourceMetadata {
                    field: meta.created_field,
                    rule_version: super::enrichment::VERSION,
                }),
                updated_at: Some(super::enrichment::TimestampOrigin::SourceMetadata {
                    field: meta.updated_field,
                    rule_version: super::enrichment::VERSION,
                }),
            }),
            project: (!meta.cwd.is_empty()).then(|| super::parsers::folder(&meta.cwd)),
            repository: meta.repository,
            title: (!meta.title.is_empty()).then_some(meta.title),
            created_at: Some(meta.created),
            updated_at: Some(meta.updated),
            messages: messages.finish()?,
        }),
        malformed_records: 0,
        first_record_issue: None,
    })
}
