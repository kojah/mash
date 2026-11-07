//! Duplicate checks run under the mutation transaction; suggestions never write.
use super::{Storage, encode, now, owner, read, validate_scope};
use crate::{
    AppError,
    records::{
        Body, DecisionState, Details, Receipt, Scope,
        matches::{Match, Request, Response},
    },
};
use rusqlite::{Connection, params};
use std::collections::BTreeSet;

fn exact(left: &Body, right: &Body) -> bool {
    left.statement == right.statement
        && left.rationale == right.rationale
        && left.review == right.review
        && left.details == right.details
        && left.effective_at == right.effective_at
}

pub(super) fn duplicate(
    connection: &Connection,
    scope: &Scope,
    body: &Body,
    timestamp: i64,
) -> Result<Option<Receipt>, AppError> {
    let mut query = connection.prepare("SELECT r.id FROM records r JOIN record_revisions v ON v.record_id=r.id AND v.revision=r.revision WHERE r.forgotten=0 AND r.owner=?1 AND r.scope=?2 AND r.kind=?3 AND json_extract(v.body,'$.statement')=?4 ORDER BY r.id").map_err(AppError::database)?;
    let rows = query
        .query_map(
            params![
                owner(connection)?,
                encode(scope)?,
                body.details.kind(),
                body.statement
            ],
            |row| row.get::<_, String>(0),
        )
        .map_err(AppError::database)?;
    for id in rows {
        let record = read(connection, &id.map_err(AppError::database)?, false)?;
        let Some(revision) = record.revisions.first() else {
            continue;
        };
        let live_state = revision.body.details.current()
            || matches!(
                revision.body.details,
                Details::Decision {
                    state: DecisionState::Proposed,
                    ..
                }
            );
        let expired = matches!(&revision.body.details, Details::Fact { valid_until: Some(end), .. } if *end < timestamp);
        if live_state && !expired && revision.evidence_current && exact(&revision.body, body) {
            return Ok(Some(Receipt {
                record_id: record.id,
                revision: record.current_revision,
                related: None,
                reused: true,
            }));
        }
    }
    Ok(None)
}

impl Storage {
    pub(crate) fn record_matches(
        &self,
        request: &Request,
        policy: crate::redaction::Policy,
    ) -> Result<Response, AppError> {
        validate_scope(&request.scope)?;
        request.body.validate()?;
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(AppError::database)?;
        let mut query = transaction.prepare("SELECT r.id FROM records r JOIN record_revisions v ON v.record_id=r.id AND v.revision=r.revision WHERE r.forgotten=0 AND r.owner=?1 AND r.scope=?2 AND r.kind=?3 ORDER BY v.recorded_at DESC,r.id LIMIT 257").map_err(AppError::database)?;
        let mut ids = query
            .query_map(
                params![
                    owner(&transaction)?,
                    encode(&request.scope)?,
                    request.body.details.kind()
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(AppError::database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::database)?;
        let scan_limited = ids.len() > 256;
        ids.truncate(256);
        let mut wanted = request.body.clone();
        wanted.redact(policy);
        let words = words(&wanted.statement);
        let timestamp = now()?;
        let mut suggestions = Vec::new();
        for id in ids {
            let mut record = read(&transaction, &id, false)?;
            let is_exact = record
                .revisions
                .first()
                .is_some_and(|revision| exact(&revision.body, &request.body));
            record.redact(policy);
            let Some(revision) = record.revisions.into_iter().next() else {
                continue;
            };
            let other = self::words(&revision.body.statement);
            let intersection = words.intersection(&other).count();
            let union = words.union(&other).count();
            let percent = u32::try_from(intersection * 100 / union.max(1)).unwrap_or(0);
            if !is_exact && percent < 50 {
                continue;
            }
            suggestions.push(Match {
                id,
                revision: record.current_revision,
                statement: revision.body.statement.clone(),
                exact: is_exact,
                current: revision.body.applies_at(timestamp),
                review: revision.body.review,
                evidence_current: revision.evidence_current,
                overlap_percent: percent,
            });
        }
        suggestions.sort_by(|a, b| {
            b.exact
                .cmp(&a.exact)
                .then_with(|| b.overlap_percent.cmp(&a.overlap_percent))
                .then_with(|| a.id.cmp(&b.id))
        });
        let truncated = suggestions.len() > 8;
        suggestions.truncate(8);
        Ok(Response {
            suggestions,
            scan_limited,
            truncated,
        })
    }
}

fn words(statement: &str) -> BTreeSet<String> {
    statement
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}
