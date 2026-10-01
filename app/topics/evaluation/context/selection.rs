use super::{Choice, Skip, Source};
use crate::{AppError, message_context::MessageContext, role::Role};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::{BTreeMap, BTreeSet};

struct Record {
    id: String,
    ordinal: i64,
    role: Role,
    parent: MessageContext,
    has_text: bool,
    text: String,
    characters: usize,
}

fn pick(records: &[Record]) -> Result<Result<&Record, Skip>, AppError> {
    let Some(anchor) = records.first() else {
        return Ok(Err(Skip::NoAnswer));
    };
    if !anchor.parent.valid(anchor.ordinal) {
        return Err(AppError::database_data("invalid topic context ancestry"));
    }
    let native = !anchor.parent.is_linear();
    let mut owned = BTreeSet::from([anchor.ordinal]);
    let mut parents = BTreeMap::new();
    let mut candidates = Vec::new();
    let mut closed = records.len() <= 64;
    for record in records.iter().skip(1).take(63) {
        if record.role == Role::User && record.has_text {
            closed = true;
            break;
        }
        if !record.parent.valid(record.ordinal) {
            return Err(AppError::database_data("invalid topic context ancestry"));
        }
        if native == record.parent.is_linear() {
            return Ok(Err(Skip::AmbiguousBranch));
        }
        if native {
            if let MessageContext::Parent { ordinal } = record.parent {
                parents.insert(record.ordinal, ordinal);
                if !owned.contains(&ordinal) {
                    continue;
                }
            } else {
                continue;
            }
        }
        owned.insert(record.ordinal);
        if record.role == Role::Assistant
            && record.text.chars().count() >= 256
            && ![
                "I'll",
                "I’ll",
                "I will",
                "Let me",
                "I'm going to",
                "I’m going to",
            ]
            .iter()
            .any(|p| record.text.trim_start().starts_with(p))
        {
            candidates.push(record);
        }
    }
    if !closed {
        return Ok(Err(Skip::TruncatedWindow));
    }
    if native && let Some(last) = candidates.last() {
        let mut lineage = BTreeSet::from([last.ordinal]);
        let mut current = last.ordinal;
        while let Some(parent) = parents.get(&current) {
            lineage.insert(*parent);
            current = *parent;
        }
        if candidates.iter().any(|r| !lineage.contains(&r.ordinal)) {
            return Ok(Err(Skip::AmbiguousBranch));
        }
    }
    Ok(candidates
        .into_iter()
        .max_by_key(|r| (r.characters, r.ordinal))
        .ok_or(Skip::NoAnswer))
}

pub(super) fn read(
    connection: &Connection,
    thread: &str,
    source: &Source,
) -> Result<Choice, AppError> {
    let Some((id, ordinal)) = source.anchor() else {
        return Ok(Choice::Skip(Skip::Unstored));
    };
    let mut query = connection.prepare_cached(
        "SELECT id,ordinal,role,context_parent,length(trim(COALESCE(search_projection,content)))>0,
         CASE WHEN role='assistant' AND length(CAST(content AS BLOB))<=65536
           THEN substr(COALESCE(search_projection,content),1,2048) ELSE '' END,
         CASE WHEN role='assistant' THEN length(COALESCE(search_projection,content)) ELSE 0 END
         FROM messages WHERE conversation_id=?1 AND ordinal>=?2 ORDER BY ordinal LIMIT 65"
    ).map_err(AppError::database)?;
    let records = query
        .query_map(params![thread, ordinal], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Role>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, bool>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, u32>(6)?,
            ))
        })
        .map_err(AppError::database)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::database)?;
    let records = records
        .into_iter()
        .map(|(id, ordinal, role, parent, has_text, text, characters)| {
            Ok(Record {
                id,
                ordinal,
                role,
                parent: MessageContext::from_parent(parent)?,
                has_text,
                text,
                characters: characters as usize,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    if records
        .first()
        .is_none_or(|r| r.id != id || r.ordinal != ordinal || r.role != Role::User)
    {
        return Err(AppError::database_data("topic user anchor changed"));
    }
    let answer = match pick(&records)? {
        Ok(answer) => answer,
        Err(reason) => return Ok(Choice::Skip(reason)),
    };
    let load = |id: &str, text: String| -> Result<Option<Source>, AppError> {
        connection.query_row("SELECT id AS message_id,ordinal,content AS canonical_content,search_projection IS NOT NULL AS stored_projection
            FROM messages WHERE id=?1 AND conversation_id=?2 AND length(CAST(content AS BLOB))<=65536", params![id, thread], |r| Source::stored(r, text))
            .optional().map_err(AppError::database)
    };
    let Some(anchor) = load(id, String::new())? else {
        return Ok(Choice::Skip(Skip::OversizedAnchor));
    };
    let Some(selected) = load(&answer.id, answer.text.clone())? else {
        return Ok(Choice::Skip(Skip::NoAnswer));
    };
    Ok(Choice::Answer {
        anchor: Box::new(anchor),
        answer: Box::new(selected),
        complete: answer.text.chars().count() == answer.characters,
    })
}
