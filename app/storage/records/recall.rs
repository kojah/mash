use super::{Storage, encode, now, read, validate_scope};
use crate::{
    AppError,
    records::{
        Review, Scope, bounded,
        recall::{Item, Request, Response, Status},
    },
};
use rusqlite::params;

const CANDIDATES: usize = 256;

impl Storage {
    pub(crate) fn recall_records(
        &self,
        request: &Request,
        policy: crate::redaction::Policy,
    ) -> Result<Response, AppError> {
        validate(request)?;
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(AppError::database)?;
        let mut ids = candidates(&transaction, request)?;
        let scan_limited = ids.len() > CANDIDATES;
        ids.truncate(CANDIDATES);
        let scanned = ids.len();
        let timestamp = now()?;
        let terms = terms(request.query.as_deref().unwrap_or_default());
        let mut items = Vec::new();
        for id in ids {
            let mut record = read(&transaction, &id, false)?;
            record.redact(policy);
            let Some(revision) = record.revisions.into_iter().next() else {
                continue;
            };
            let status = if !revision.evidence_current {
                Status::StaleEvidence
            } else if revision.body.review == Review::Candidate {
                Status::Candidate
            } else if !revision.body.applies_at(timestamp) {
                Status::Inactive
            } else {
                Status::Current
            };
            if status != Status::Current && !request.include_review {
                continue;
            }
            let mut text = format!("{}\n{}", revision.body.statement, revision.body.rationale);
            for evidence in &revision.evidence {
                text.push('\n');
                text.push_str(&evidence.quote);
            }
            if request
                .file
                .as_ref()
                .is_some_and(|file| !mentions_file(&text, file))
            {
                continue;
            }
            let text = text.to_lowercase();
            let matches = terms
                .iter()
                .filter(|term| text.contains(term.as_str()))
                .count();
            if !terms.is_empty() && matches == 0 {
                continue;
            }
            let last_confirmed_at: Option<i64> = transaction.query_row("SELECT max(confirmed_at) FROM record_confirmations WHERE record_id=?1 AND revision=?2", params![id, revision.revision], |row| row.get(0)).map_err(AppError::database)?;
            let age_days = timestamp.saturating_sub(revision.recorded_at).max(0) / 86_400_000;
            let recency = match age_days {
                0..=7 => 8,
                8..=30 => 4,
                31..=90 => 2,
                _ => 0,
            };
            let relevance = u32::try_from(matches).unwrap_or(16) * 16;
            items.push(Item {
                id,
                owner: record.owner,
                scope: record.scope,
                revision: revision.revision,
                body: revision.body,
                status,
                confirmations: revision.confirmations,
                last_confirmed_at,
                score: relevance + revision.confirmations.min(8) * 2 + recency,
            });
        }
        items.sort_by(|a, b| {
            (a.status != Status::Current)
                .cmp(&(b.status != Status::Current))
                .then_with(|| b.score.cmp(&a.score))
                .then_with(|| a.id.cmp(&b.id))
        });
        fit(
            items,
            scanned,
            scan_limited,
            request.limit.unwrap_or(8),
            request.byte_budget.unwrap_or(6000),
        )
    }
}

fn candidates(
    connection: &rusqlite::Connection,
    request: &Request,
) -> Result<Vec<String>, AppError> {
    let mut statement = connection.prepare("SELECT r.id FROM records r JOIN record_revisions v ON v.record_id=r.id AND v.revision=r.revision WHERE r.forgotten=0 AND (r.scope=?1 OR (?2 AND r.scope=?3)) ORDER BY v.recorded_at DESC,r.id LIMIT 257").map_err(AppError::database)?;
    statement
        .query_map(
            params![
                encode(&request.scope)?,
                request.include_personal,
                encode(&Scope::Personal)?
            ],
            |row| row.get::<_, String>(0),
        )
        .map_err(AppError::database)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::database)
}

fn validate(request: &Request) -> Result<(), AppError> {
    validate_scope(&request.scope)?;
    if !(1..=20).contains(&request.limit.unwrap_or(8))
        || !(1024..=32768).contains(&request.byte_budget.unwrap_or(6000))
    {
        return Err(AppError::usage(
            "recall limit must be 1..20 and byte_budget 1024..32768",
        ));
    }
    if let Some(query) = &request.query {
        bounded(query, 1024, "recall query")?;
    }
    if let Some(file) = &request.file {
        bounded(file, 2048, "recall file")?;
        if !matches!(
            request.scope,
            Scope::Repository { .. } | Scope::LocalProject { .. }
        ) || request.include_personal
            || file.starts_with('/')
            || file.contains(['\\', ':'])
            || file.split('/').any(|part| matches!(part, "" | "." | ".."))
        {
            return Err(AppError::usage(
                "file recall requires an exact project-relative path and repository or local_project scope, without include_personal",
            ));
        }
    }
    Ok(())
}

fn terms(query: &str) -> Vec<String> {
    let mut terms = query
        .split_whitespace()
        .map(str::to_lowercase)
        .take(16)
        .collect::<Vec<_>>();
    terms.sort();
    terms.dedup();
    terms
}

fn mentions_file(text: &str, file: &str) -> bool {
    let path_character = |c: char| c.is_alphanumeric() || matches!(c, '/' | '\\' | '.' | '_' | '-');
    text.match_indices(file).any(|(start, _)| {
        text[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !path_character(c))
            && text[start + file.len()..]
                .chars()
                .next()
                .is_none_or(|c| !path_character(c))
    })
}

fn fit(
    items: Vec<Item>,
    scanned: usize,
    scan_limited: bool,
    limit: usize,
    budget: usize,
) -> Result<Response, AppError> {
    let mut response = Response {
        records: Vec::new(),
        scanned,
        scan_limited,
        truncated: false,
    };
    for item in items {
        if response.records.len() == limit {
            response.truncated = true;
            break;
        }
        response.records.push(item);
        // Include the whole JSON packet, not just prose; reserve the larger false spelling.
        if encode(&response)?.len() > budget {
            response.records.pop();
            response.truncated = true;
        }
    }
    Ok(response)
}
