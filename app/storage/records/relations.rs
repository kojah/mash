//! Explicit, identity-level relationships; reads never infer agreement or conflict.
use super::{
    AppError, Connection, Expected, Receipt, Review, conflict, current, journal, params, read,
};
use crate::records::AssertionRelation;

pub(super) fn link(
    connection: &Connection,
    expected: &Expected,
    target: &Expected,
    relation: AssertionRelation,
    timestamp: i64,
) -> Result<Receipt, AppError> {
    let (scope, body, _) = current(connection, expected)?;
    let (target_scope, target_body, _) = current(connection, target)?;
    if expected.id == target.id || scope != target_scope {
        return Err(AppError::usage(
            "record links require distinct records in the same owner and scope",
        ));
    }
    for (endpoint, body) in [(expected, body), (target, target_body)] {
        if body.review != Review::Established
            || !body.applies_at(timestamp)
            || read(connection, &endpoint.id, false)?
                .revisions
                .first()
                .is_none_or(|revision| !revision.evidence_current)
        {
            return Err(AppError::usage(
                "record links require established current records with current evidence",
            ));
        }
    }
    let relation = match relation {
        AssertionRelation::Supports => "supports",
        AssertionRelation::Contradicts => "contradicts",
    };
    let opposite: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM record_links WHERE ((source_id=?1 AND target_id=?2) OR (source_id=?2 AND target_id=?1)) AND relation IN ('supports','contradicts') AND relation<>?3)",
        params![expected.id, target.id, relation], |row| row.get(0),
    ).map_err(AppError::database)?;
    if opposite {
        return Err(conflict(
            "opposite relationship already recorded; retire or supersede the changed claim",
        ));
    }
    let inserted = connection
        .execute(
            "INSERT OR IGNORE INTO record_links(source_id,target_id,relation) VALUES(?1,?2,?3)",
            params![expected.id, target.id, relation],
        )
        .map_err(AppError::database)?;
    if inserted > 0 {
        journal(connection, &expected.id)?;
        journal(connection, &target.id)?;
    }
    Ok(Receipt {
        record_id: expected.id.clone(),
        revision: expected.revision,
        related: Some(target.clone()),
        reused: inserted == 0,
    })
}

/// Conflict is independent of evidence freshness. A stale claim must not erase
/// a declared contradiction. Retiring either claim stops it being an active conflict.
pub(super) fn conflicted(
    connection: &Connection,
    id: &str,
    timestamp: i64,
) -> Result<bool, AppError> {
    connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM record_links l
         JOIN records a ON a.id=l.source_id JOIN records b ON b.id=l.target_id
         JOIN record_revisions av ON av.record_id=a.id AND av.revision=a.revision
         JOIN record_revisions bv ON bv.record_id=b.id AND bv.revision=b.revision
         WHERE l.relation='contradicts' AND (a.id=?1 OR b.id=?1) AND a.forgotten=0 AND b.forgotten=0
         AND json_extract(av.body,'$.review')='established' AND json_extract(bv.body,'$.review')='established'
         AND json_extract(av.body,'$.details.state') IN ('accepted','active','current','open')
         AND json_extract(bv.body,'$.details.state') IN ('accepted','active','current','open')
         AND coalesce(json_extract(av.body,'$.effective_at'),?2)<=?2
         AND coalesce(json_extract(bv.body,'$.effective_at'),?2)<=?2
         AND coalesce(json_extract(av.body,'$.details.valid_until'),?2)>=?2
         AND coalesce(json_extract(bv.body,'$.details.valid_until'),?2)>=?2)",
        params![id, timestamp], |row| row.get(0),
    ).map_err(AppError::database)
}
