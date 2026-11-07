use super::{
    AppError, Connection, Evidence, OptionalExtension, Record, bounded, confirmation_history,
    decode, encode, params, records, source_current,
};

// A maximum-sized revision (256 quoted sources, including JSON escaping)
// fits in this budget. Leave space for links and envelope fields.
pub(super) const PAGE_BYTES: usize = 16 * 1024 * 1024;
// Reserve four MiB below a 32 MiB list envelope for shared-node labels and JSON.
pub(super) const LIST_RECORD_BYTES: usize = 28 * 1024 * 1024;
const REVISION_BYTES: usize = 15 * 1024 * 1024;
const LINK_LIMIT: usize = 128;

pub(crate) fn read(connection: &Connection, id: &str, history: bool) -> Result<Record, AppError> {
    read_page(
        connection,
        &records::Get {
            id: id.into(),
            history,
            ..Default::default()
        },
    )
}

pub(crate) fn read_page(
    connection: &Connection,
    request: &records::Get,
) -> Result<Record, AppError> {
    bounded(&request.id, 512, "record ID")?;
    let limit = request.limit.unwrap_or(8);
    if !(1..=20).contains(&limit)
        || request.before_revision.is_some_and(|value| value < 1)
        || (!request.history && (request.before_revision.is_some() || request.limit.is_some()))
        || request.link_offset > 100_000
    {
        return Err(AppError::usage(
            "history pagination requires history=true, limit 1..20 and a positive before_revision; link_offset must be at most 100000",
        ));
    }
    let (owner, scope, current_revision): (String, String, i64) = connection
        .query_row(
            "SELECT owner,scope,revision FROM records WHERE id=?1 AND forgotten=0",
            [&request.id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(AppError::database)?
        .ok_or_else(|| AppError::usage("record does not exist or was forgotten"))?;
    let mut statement = connection.prepare("SELECT revision,body,recorded_at FROM record_revisions WHERE record_id=?1 AND (?2 OR revision=?3) AND (?4 IS NULL OR revision<?4) ORDER BY revision DESC LIMIT ?5").map_err(AppError::database)?;
    let mut rows = statement
        .query(params![
            request.id,
            request.history,
            current_revision,
            request.before_revision,
            i64::try_from(limit + 1).map_err(|_| AppError::usage("history limit out of range"))?
        ])
        .map_err(AppError::database)?;
    let mut revisions: Vec<records::RecordRevision> = Vec::new();
    let mut bytes = 0;
    let mut next_before_revision = None;
    while let Some(row) = rows.next().map_err(AppError::database)? {
        if revisions.len() == limit {
            next_before_revision = revisions.last().map(|item| item.revision);
            break;
        }
        let item = revision(
            connection,
            &request.id,
            row.get(0).map_err(AppError::database)?,
            &row.get::<_, String>(1).map_err(AppError::database)?,
            row.get(2).map_err(AppError::database)?,
        )?;
        let size = encode(&item)?.len();
        if size > REVISION_BYTES {
            return Err(AppError::usage(
                "record revision exceeds the supported read size",
            ));
        }
        if bytes + size > REVISION_BYTES {
            next_before_revision = revisions.last().map(|item| item.revision);
            break;
        }
        bytes += size;
        revisions.push(item);
    }
    let mut links = links(connection, &request.id, request.link_offset)?;
    let next_link_offset = (links.len() > LINK_LIMIT).then_some(request.link_offset + LINK_LIMIT);
    links.truncate(LINK_LIMIT);
    let result = Record {
        node_id: None,
        id: request.id.clone(),
        owner,
        scope: decode(&scope)?,
        current_revision,
        revisions,
        links,
        conflicted: super::relations::conflicted(connection, &request.id, super::now()?)?,
        next_before_revision,
        next_link_offset,
    };
    if encode(&result)?.len() > PAGE_BYTES {
        return Err(AppError::usage(
            "record page exceeds the supported read size",
        ));
    }
    Ok(result)
}

fn revision(
    connection: &Connection,
    id: &str,
    revision: i64,
    body: &str,
    recorded_at: i64,
) -> Result<records::RecordRevision, AppError> {
    let mut statement = connection.prepare("SELECT e.chat_id,e.message_id,e.fingerprint,e.quote,m.content,e.purpose,e.source_role,m.role FROM record_evidence e LEFT JOIN messages m ON m.id=e.message_id AND m.chat_id=e.chat_id WHERE e.record_id=?1 AND e.revision=?2 ORDER BY e.position").map_err(AppError::database)?;
    let mut rows = statement
        .query(params![id, revision])
        .map_err(AppError::database)?;
    let mut evidence = Vec::new();
    let mut evidence_current = true;
    while let Some(row) = rows.next().map_err(AppError::database)? {
        let source = Evidence {
            chat_id: row.get(0).map_err(AppError::database)?,
            message_id: row.get(1).map_err(AppError::database)?,
            fingerprint: row.get(2).map_err(AppError::database)?,
            quote: row.get(3).map_err(AppError::database)?,
            purpose: row.get(5).map_err(AppError::database)?,
            source_role: row.get(6).map_err(AppError::database)?,
        };
        let content: Option<String> = row.get(4).map_err(AppError::database)?;
        let role: Option<crate::role::Role> = row.get(7).map_err(AppError::database)?;
        evidence_current &= source_current(&source, content.as_deref(), role.as_ref());
        evidence.push(source);
    }
    evidence_current &= !evidence.is_empty();
    let confirmation_history = confirmation_history(connection, id, revision)?;
    let confirmations = u32::try_from(confirmation_history.len())
        .map_err(|_| AppError::internal("confirmation count out of range"))?;
    Ok(records::RecordRevision {
        revision,
        body: decode(body)?,
        recorded_at,
        evidence,
        evidence_current,
        confirmations,
        confirmation_history,
    })
}

fn links(connection: &Connection, id: &str, offset: usize) -> Result<Vec<records::Link>, AppError> {
    let mut statement = connection.prepare("SELECT source_id,target_id,relation FROM record_links WHERE source_id=?1 OR target_id=?1 ORDER BY source_id,target_id,relation LIMIT 129 OFFSET ?2").map_err(AppError::database)?;
    statement
        .query_map(
            params![
                id,
                i64::try_from(offset).map_err(|_| AppError::usage("link offset out of range"))?
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .map_err(AppError::database)?
        .map(|row| {
            let (source_id, target_id, relation) = row.map_err(AppError::database)?;
            Ok(records::Link {
                source_id,
                target_id,
                relation: decode(&encode(&relation)?)?,
            })
        })
        .collect()
}
