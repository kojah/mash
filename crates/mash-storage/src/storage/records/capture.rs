//! Read-only discovery. Extraction receipts remain the only durable review marks.
use super::{Storage, bounded};
use crate::{
    AppError,
    records::{
        Scope,
        capture::{Policy, Request, Response, Task},
        extraction::Inspect,
    },
};
use rusqlite::{Row, params};

fn task(row: &Row<'_>) -> rusqlite::Result<Task> {
    let chat_id: String = row.get(0)?;
    let repository: Option<String> = row.get(1)?;
    let project: Option<String> = row.get(2)?;
    let scope = if let Some(identity) = repository.filter(|value| !value.is_empty()) {
        Scope::Repository { identity }
    } else if let Some(path) = project.filter(|value| std::path::Path::new(value).is_absolute()) {
        Scope::LocalProject { path }
    } else {
        Scope::Chat {
            id: chat_id.clone(),
        }
    };
    Ok(Task {
        chat_id,
        scope,
        source_fingerprint: row.get(3)?,
    })
}

impl Storage {
    pub fn record_capture(&self, policy: &Policy, request: &Request) -> Result<Response, AppError> {
        if request.offset > 100_000 {
            return Err(AppError::usage("capture offset must be at most 100000"));
        }
        if let Some(session) = &request.session_id {
            bounded(session, 512, "capture session ID")?;
        }
        let mut response = Response {
            enabled: policy.enabled,
            backfill_since: policy.backfill_since,
            current: None,
            backfill: None,
            next_offset: None,
            scan_limited: false,
        };
        if !policy.enabled {
            return Ok(response);
        }
        if let Some(session) = &request.session_id {
            let mut statement = self.connection.prepare("SELECT id,repository,project,source_fingerprint FROM chats WHERE id=?1 OR source_session=?1 ORDER BY id LIMIT 2").map_err(AppError::database)?;
            let tasks = statement
                .query_map([session], task)
                .map_err(AppError::database)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(AppError::database)?;
            if tasks.len() > 1 {
                return Err(AppError::usage(
                    "capture session matches multiple chats; specify its canonical chat ID",
                ));
            }
            response.current = tasks.into_iter().next();
        }
        if !request.backfill {
            return Ok(response);
        }
        let Some(since) = policy.backfill_since else {
            return Ok(response);
        };
        // Scan bounded pages of metadata, never decompressing chat bodies here.
        // A stale discovery can only cause a retry: extract_records validates the
        // source fingerprint again in its existing atomic write transaction.
        let mut statement = self.connection.prepare("SELECT id,repository,project,source_fingerprint FROM chats WHERE COALESCE(updated_at,created_at,0)>=?1 AND (?2 IS NULL OR (id<>?2 AND COALESCE(source_session,'')<>?2)) ORDER BY COALESCE(updated_at,created_at,0),id LIMIT 101 OFFSET ?3").map_err(AppError::database)?;
        let mut tasks = statement
            .query_map(
                params![
                    since,
                    request.session_id,
                    i64::try_from(request.offset)
                        .map_err(|_| AppError::usage("capture offset out of range"))?
                ],
                task,
            )
            .map_err(AppError::database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::database)?;
        let more = tasks.len() > 100;
        tasks.truncate(100);
        for (index, mut candidate) in tasks.into_iter().enumerate() {
            let checkpoint = self.extraction_checkpoint(&Inspect {
                scope: candidate.scope.clone(),
                chat_id: candidate.chat_id.clone(),
            })?;
            if !checkpoint.up_to_date {
                candidate.source_fingerprint = checkpoint.source_fingerprint;
                response.backfill = Some(candidate);
                // A scan cursor is not a completion mark. Advancing past an
                // oversized/failed review lets later chats make progress; the
                // next scan wraps and offers it again until extraction commits.
                response.next_offset = Some((request.offset + index + 1).min(100_000));
                return Ok(response);
            }
        }
        response.scan_limited = more && request.offset + 100 > 100_000;
        response.next_offset = (more && !response.scan_limited).then_some(request.offset + 100);
        Ok(response)
    }
}
