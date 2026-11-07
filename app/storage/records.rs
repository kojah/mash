//! Transactional record state. Source refresh and privacy forgetting are distinct.
use super::Storage;

mod extraction;
mod matches;
mod reading;
mod recall;
mod relations;
#[cfg(test)]
mod tests;
pub(crate) mod transfer;
use crate::{
    AppError,
    records::{
        self, Body, Change, Citation, Details, Evidence, EvidenceUse, Expected, List, Mutation,
        Page, QuestionState, Receipt, Record, Review, Scope, bounded, conflict, decode, encode,
    },
};
use rusqlite::{Connection, OptionalExtension, params};

fn now() -> Result<i64, AppError> {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| AppError::internal("system clock precedes Unix epoch"))?;
    i64::try_from(elapsed.as_millis()).map_err(|_| AppError::internal("clock out of range"))
}

fn owner(connection: &Connection) -> Result<String, AppError> {
    connection
        .query_row("SELECT id FROM record_owner WHERE singleton=1", [], |row| {
            row.get(0)
        })
        .map_err(AppError::database)
}

fn validate_scope(scope: &Scope) -> Result<(), AppError> {
    match scope {
        Scope::Personal => Ok(()),
        Scope::Repository { identity } => bounded(identity, 2048, "repository identity"),
        Scope::Chat { id } => bounded(id, 512, "chat ID"),
        Scope::LocalProject { path } => {
            bounded(path, 4096, "project path")?;
            if !std::path::Path::new(path).is_absolute() {
                return Err(AppError::usage("local project path must be absolute"));
            }
            Ok(())
        }
    }
}

fn evidence(
    connection: &Connection,
    scope: &Scope,
    citations: &[Citation],
    endorsement: bool,
) -> Result<Vec<Evidence>, AppError> {
    if citations.is_empty() || citations.len() > 16 {
        return Err(AppError::usage("provide 1 to 16 source citations"));
    }
    let mut human = false;
    let mut result = Vec::new();
    for citation in citations {
        bounded(&citation.message_id, 512, "message ID")?;
        bounded(&citation.quote, 8192, "quote")?;
        if crate::redaction::private(&citation.quote) != citation.quote {
            return Err(AppError::usage("private spans cannot support records"));
        }
        let source = connection.query_row(
            "SELECT m.chat_id,m.content,m.role,c.repository,c.project FROM messages m JOIN chats c ON c.id=m.chat_id WHERE m.id=?1",
            [&citation.message_id], |row| Ok((row.get::<_,String>(0)?, row.get::<_,String>(1)?, row.get::<_,String>(2)?, row.get::<_,Option<String>>(3)?, row.get::<_,Option<String>>(4)?))
        ).optional().map_err(AppError::database)?.ok_or_else(|| conflict("cited message is missing"))?;
        let (chat_id, content, role, repository, project) = source;
        let in_scope = match scope {
            Scope::Personal => true,
            Scope::Repository { identity } => repository.as_ref() == Some(identity),
            Scope::LocalProject { path } => project.as_ref() == Some(path),
            Scope::Chat { id } => chat_id == *id,
        };
        if !in_scope {
            return Err(AppError::usage("citation is outside the record scope"));
        }
        let fingerprint = blake3::hash(content.as_bytes()).to_hex().to_string();
        if !content.contains(&citation.quote)
            || citation
                .expected_fingerprint
                .as_ref()
                .is_some_and(|expected| expected != &fingerprint)
        {
            return Err(conflict("citation no longer matches its source message"));
        }
        human |= role == "user" && citation.purpose == EvidenceUse::Endorsement;
        result.push(Evidence {
            chat_id,
            message_id: citation.message_id.clone(),
            fingerprint,
            quote: citation.quote.clone(),
            purpose: Some(citation.purpose),
            source_role: Some(role.into()),
        });
    }
    if endorsement && !human {
        return Err(AppError::usage(
            "established decisions and preferences require a user endorsement citation; uncertain interpretations must remain candidates",
        ));
    }
    Ok(result)
}

fn requires_endorsement(body: &Body) -> bool {
    body.review == Review::Established
        && matches!(
            body.details,
            Details::Decision { .. } | Details::Preference { .. }
        )
}

fn journal(connection: &Connection, id: &str) -> Result<(), AppError> {
    connection
        .execute(
            "UPDATE shared_publication SET revision=revision+1 WHERE singleton=1",
            [],
        )
        .map_err(AppError::database)?;
    connection.execute("INSERT INTO record_changes SELECT ?1,revision FROM shared_publication WHERE singleton=1 ON CONFLICT(record_id) DO UPDATE SET revision=excluded.revision", [id]).map_err(AppError::database)?;
    Ok(())
}

fn insert_revision(
    connection: &Connection,
    id: &str,
    revision: i64,
    body: &Body,
    sources: &[Evidence],
    timestamp: i64,
) -> Result<(), AppError> {
    if revision > 1000 {
        return Err(AppError::usage("record history limit reached"));
    }
    connection
        .execute(
            "INSERT INTO record_revisions(record_id,revision,body,recorded_at) VALUES(?1,?2,?3,?4)",
            params![id, revision, encode(body)?, timestamp],
        )
        .map_err(AppError::database)?;
    for (position, source) in sources.iter().enumerate() {
        let position =
            i64::try_from(position).map_err(|_| AppError::usage("too much record evidence"))?;
        connection.execute("INSERT INTO record_evidence(record_id,revision,position,chat_id,message_id,fingerprint,quote,purpose,source_role) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![id,revision,position,source.chat_id,source.message_id,source.fingerprint,source.quote,source.purpose,source.source_role]).map_err(AppError::database)?;
    }
    connection
        .execute(
            "UPDATE records SET revision=?2 WHERE id=?1",
            params![id, revision],
        )
        .map_err(AppError::database)?;
    journal(connection, id)
}

fn current(
    connection: &Connection,
    expected: &Expected,
) -> Result<(Scope, Body, Vec<Evidence>), AppError> {
    let record = read(connection, &expected.id, false)?;
    if record.owner != owner(connection)? {
        return Err(AppError::usage("only the record owner may edit it"));
    }
    if record.current_revision != expected.revision {
        return Err(conflict("record revision changed; reread before editing"));
    }
    let revision = record
        .revisions
        .into_iter()
        .next()
        .ok_or_else(|| AppError::database_data("missing record revision"))?;
    Ok((record.scope, revision.body, revision.evidence))
}

fn create(
    connection: &Connection,
    id: &str,
    scope: &Scope,
    body: &Body,
    citations: &[Citation],
    timestamp: i64,
) -> Result<(), AppError> {
    validate_scope(scope)?;
    body.validate()?;
    if body.details.superseded() {
        return Err(AppError::usage(
            "create a successor through supersede, not an already superseded record",
        ));
    }
    let sources = evidence(connection, scope, citations, requires_endorsement(body))?;
    connection
        .execute(
            "INSERT INTO records(id,owner,kind,scope,revision) VALUES(?1,?2,?3,?4,1)",
            params![id, owner(connection)?, body.details.kind(), encode(scope)?],
        )
        .map_err(AppError::database)?;
    insert_revision(connection, id, 1, body, &sources, timestamp)
}

fn change(
    connection: &Connection,
    request: &Mutation,
    timestamp: i64,
) -> Result<Receipt, AppError> {
    let id = format!(
        "record/{}/{}",
        owner(connection)?,
        blake3::hash(request.operation_id.as_bytes()).to_hex()
    );
    match &request.change {
        Change::Link {
            expected,
            target,
            relation,
        } => relations::link(connection, expected, target, *relation, timestamp),
        Change::Create {
            scope,
            body,
            evidence,
            on_duplicate,
        } => {
            validate_scope(scope)?;
            body.validate()?;
            self::evidence(connection, scope, evidence, requires_endorsement(body))?;
            if let Some(receipt) = matches::duplicate(connection, scope, body, timestamp)? {
                if matches!(on_duplicate, records::DuplicatePolicy::Report) {
                    return Err(conflict(&format!(
                        "exact live record already exists: {} at revision {}; inspect it or explicitly set on_duplicate=reuse",
                        receipt.record_id, receipt.revision
                    )));
                }
                return Ok(receipt);
            }
            create(connection, &id, scope, body, evidence, timestamp)?;
            Ok(Receipt {
                record_id: id,
                revision: 1,
                related: None,
                reused: false,
            })
        }
        Change::Revise {
            expected,
            body,
            evidence: citations,
        } => revise(connection, expected, body, citations, timestamp),
        Change::Supersede {
            expected,
            body,
            evidence: citations,
        } => supersede(connection, id, expected, body, citations, timestamp),
        Change::Confirm {
            expected,
            evidence: citations,
        } => confirm(connection, expected, citations, timestamp),
        Change::Resolve {
            expected,
            answer,
            resolved_by,
            evidence: citations,
        } => resolve(
            connection,
            expected,
            answer,
            resolved_by.as_ref(),
            citations,
            timestamp,
        ),
        Change::Forget { expected } => {
            current(connection, expected)?;
            purge(connection, &expected.id)?;
            Ok(Receipt {
                record_id: expected.id.clone(),
                revision: expected.revision + 1,
                related: None,
                reused: false,
            })
        }
    }
}

fn revise(
    connection: &Connection,
    expected: &Expected,
    body: &Body,
    citations: &[Citation],
    timestamp: i64,
) -> Result<Receipt, AppError> {
    let (scope, previous, _) = current(connection, expected)?;
    body.validate()?;
    if previous.details.kind() != body.details.kind() {
        return Err(AppError::usage("revision cannot change record kind"));
    }
    if previous.details.superseded() || body.details.superseded() {
        return Err(conflict(
            "superseded records cannot be revived or edited; create a new choice",
        ));
    }
    let sources = evidence(connection, &scope, citations, requires_endorsement(body))?;
    insert_revision(
        connection,
        &expected.id,
        expected.revision + 1,
        body,
        &sources,
        timestamp,
    )?;
    Ok(Receipt {
        record_id: expected.id.clone(),
        revision: expected.revision + 1,
        related: None,
        reused: false,
    })
}

fn supersede(
    connection: &Connection,
    id: String,
    expected: &Expected,
    body: &Body,
    citations: &[Citation],
    timestamp: i64,
) -> Result<Receipt, AppError> {
    let (scope, mut previous, sources) = current(connection, expected)?;
    if body
        .effective_at
        .zip(previous.effective_at)
        .is_some_and(|(new, old)| new < old)
    {
        return Err(AppError::usage(
            "an older choice cannot supersede a later decision",
        ));
    }
    if previous.details.kind() != body.details.kind()
        || !body.details.current()
        || body.review != Review::Established
    {
        return Err(AppError::usage(
            "successor must be an established current record of the same kind",
        ));
    }
    previous.details.supersede()?;
    create(connection, &id, &scope, body, citations, timestamp)?;
    insert_revision(
        connection,
        &expected.id,
        expected.revision + 1,
        &previous,
        &sources,
        timestamp,
    )?;
    connection
        .execute(
            "INSERT INTO record_links(source_id,target_id,relation) VALUES(?1,?2,'supersedes')",
            params![id, expected.id],
        )
        .map_err(AppError::database)?;
    Ok(Receipt {
        record_id: id,
        revision: 1,
        related: Some(Expected {
            id: expected.id.clone(),
            revision: expected.revision + 1,
        }),
        reused: false,
    })
}

fn confirm(
    connection: &Connection,
    expected: &Expected,
    citations: &[Citation],
    timestamp: i64,
) -> Result<Receipt, AppError> {
    let (scope, body, _) = current(connection, expected)?;
    if body.review != Review::Established || !body.details.current() {
        return Err(AppError::usage(
            "only established current records can be confirmed; to establish a reviewed candidate, use revise with body.review=established and verified evidence",
        ));
    }
    let sources = evidence(connection, &scope, citations, requires_endorsement(&body))?;
    // Confirmation citations become retained dependencies for forget.
    let mut position: i64 = connection.query_row("SELECT COALESCE(MAX(position),-1)+1 FROM record_evidence WHERE record_id=?1 AND revision=?2",params![expected.id,expected.revision],|row| row.get(0)).map_err(AppError::database)?;
    if position + i64::try_from(sources.len()).unwrap_or(i64::MAX) > 256 {
        return Err(AppError::usage("confirmation evidence limit reached"));
    }
    for source in sources {
        if requires_endorsement(&body) {
            let endorsed = citations.iter().any(|citation| {
                citation.message_id == source.message_id
                    && citation.quote == source.quote
                    && citation.purpose == EvidenceUse::Endorsement
            });
            let human: bool = connection
                .query_row(
                    "SELECT role='user' FROM messages WHERE id=?1",
                    [&source.message_id],
                    |row| row.get(0),
                )
                .map_err(AppError::database)?;
            if !endorsed || !human {
                continue;
            }
        }
        let seen: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM record_evidence WHERE record_id=?1 AND revision=?2 AND message_id=?3)",params![expected.id,expected.revision,source.message_id],|row|row.get(0)).map_err(AppError::database)?;
        if seen {
            continue;
        }
        let inserted = connection
            .execute(
                "INSERT OR IGNORE INTO record_confirmations VALUES(?1,?2,?3,?4)",
                params![expected.id, expected.revision, source.message_id, timestamp],
            )
            .map_err(AppError::database)?;
        if inserted > 0 {
            connection
                .execute(
                    "INSERT INTO record_evidence VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                    params![
                        expected.id,
                        expected.revision,
                        position,
                        source.chat_id,
                        source.message_id,
                        source.fingerprint,
                        source.quote,
                        source.purpose,
                        source.source_role
                    ],
                )
                .map_err(AppError::database)?;
            position += 1;
        }
    }
    journal(connection, &expected.id)?;
    Ok(Receipt {
        record_id: expected.id.clone(),
        revision: expected.revision,
        related: None,
        reused: false,
    })
}

fn resolve(
    connection: &Connection,
    expected: &Expected,
    answer: &str,
    resolved_by: Option<&Expected>,
    citations: &[Citation],
    timestamp: i64,
) -> Result<Receipt, AppError> {
    let (scope, mut body, _) = current(connection, expected)?;
    if let Some(resolver) = resolved_by {
        let (resolver_scope, resolver_body, _) = current(connection, resolver)?;
        let record = read(connection, &resolver.id, false)?;
        if resolver_scope != scope
            || resolver.id == expected.id
            || resolver_body.review != Review::Established
            || !resolver_body.applies_at(timestamp)
            || matches!(resolver_body.details, Details::OpenQuestion { .. })
            || record
                .revisions
                .first()
                .is_none_or(|revision| !revision.evidence_current)
        {
            return Err(AppError::usage(
                "resolution requires an established current decision, preference or fact with current evidence in the same scope",
            ));
        }
    }
    bounded(answer, 8192, "answer")?;
    let Details::OpenQuestion {
        state,
        answer: stored,
    } = &mut body.details
    else {
        return Err(AppError::usage("only open questions can be resolved"));
    };
    if *state != QuestionState::Open {
        return Err(conflict("question is no longer open"));
    }
    *state = QuestionState::Resolved;
    *stored = Some(answer.to_owned());
    body.validate()?;
    let sources = evidence(connection, &scope, citations, false)?;
    insert_revision(
        connection,
        &expected.id,
        expected.revision + 1,
        &body,
        &sources,
        timestamp,
    )?;
    if let Some(resolver) = resolved_by {
        connection.execute("INSERT OR IGNORE INTO record_links(source_id,target_id,relation) VALUES(?1,?2,'resolves')", params![resolver.id, expected.id]).map_err(AppError::database)?;
        // Links are published with their source record; both ends must travel.
        journal(connection, &resolver.id)?;
    }
    Ok(Receipt {
        record_id: expected.id.clone(),
        revision: expected.revision + 1,
        related: resolved_by.cloned(),
        reused: false,
    })
}

pub(crate) fn purge(connection: &Connection, id: &str) -> Result<(), AppError> {
    connection
        .execute("DELETE FROM record_revisions WHERE record_id=?1", [id])
        .map_err(AppError::database)?;
    connection
        .execute(
            "DELETE FROM record_links WHERE source_id=?1 OR target_id=?1",
            [id],
        )
        .map_err(AppError::database)?;
    connection
        .execute(
            "UPDATE records SET forgotten=1,revision=revision+1,scope='null' WHERE id=?1",
            [id],
        )
        .map_err(AppError::database)?;
    journal(connection, id)
}

/// Privacy deletion follows every historical citation, not only current text.
pub(crate) fn forget_chat(connection: &Connection, chat: &str) -> Result<usize, AppError> {
    connection
        .execute("DELETE FROM record_checkpoints WHERE chat_id=?1", [chat])
        .map_err(AppError::database)?;
    let mut statement = connection
        .prepare("SELECT DISTINCT record_id FROM record_evidence WHERE chat_id=?1")
        .map_err(AppError::database)?;
    let ids = statement
        .query_map([chat], |row| row.get::<_, String>(0))
        .map_err(AppError::database)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::database)?;
    for id in &ids {
        purge(connection, id)?;
    }
    Ok(ids.len())
}

fn source_current(
    source: &Evidence,
    content: Option<&str>,
    role: Option<&crate::role::Role>,
) -> bool {
    content.is_some_and(|content| {
        blake3::hash(content.as_bytes()).to_hex().as_str() == source.fingerprint
            && content.contains(&source.quote)
            && source
                .source_role
                .as_ref()
                .is_some_and(|saved| role == Some(saved))
    })
}

fn confirmation_history(
    connection: &Connection,
    id: &str,
    revision: i64,
) -> Result<Vec<records::Confirmation>, AppError> {
    let mut statement = connection.prepare("SELECT message_id,confirmed_at FROM record_confirmations WHERE record_id=?1 AND revision=?2 ORDER BY confirmed_at,message_id").map_err(AppError::database)?;
    statement
        .query_map(params![id, revision], |row| {
            Ok(records::Confirmation {
                message_id: row.get(0)?,
                confirmed_at: row.get(1)?,
            })
        })
        .map_err(AppError::database)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::database)
}

pub(crate) use reading::{read, read_page};
impl Storage {
    pub(crate) fn mutate_record(&mut self, request: &Mutation) -> Result<Receipt, AppError> {
        bounded(&request.operation_id, 256, "operation ID")?;
        let serialized = encode(request)?;
        if serialized.len() > 160_000 {
            return Err(AppError::usage("record operation exceeds 160000 bytes"));
        }
        let digest = blake3::hash(serialized.as_bytes()).to_hex().to_string();
        let transaction = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(AppError::database)?;
        let previous = transaction
            .query_row(
                "SELECT request_digest,result FROM record_operations WHERE operation_id=?1",
                [&request.operation_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(AppError::database)?;
        if let Some((stored, result)) = previous {
            if stored != digest {
                return Err(conflict(
                    "operation ID was already used for a different request",
                ));
            }
            return decode(&result);
        }
        let result = change(&transaction, request, now()?)?;
        transaction
            .execute(
                "INSERT INTO record_operations VALUES(?1,?2,?3)",
                params![request.operation_id, digest, encode(&result)?],
            )
            .map_err(AppError::database)?;
        transaction.commit().map_err(AppError::database)?;
        Ok(result)
    }

    #[cfg(test)]
    pub(crate) fn record(&self, id: &str, history: bool) -> Result<Record, AppError> {
        bounded(id, 512, "record ID")?;
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(AppError::database)?;
        read(&transaction, id, history)
    }

    pub(crate) fn records(&self, request: &List) -> Result<Page, AppError> {
        list(&self.connection, request)
    }

    pub(crate) fn record_page(&self, request: &records::Get) -> Result<Record, AppError> {
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(AppError::database)?;
        read_page(&transaction, request)
    }
}

pub(crate) fn list(connection: &Connection, request: &List) -> Result<Page, AppError> {
    let limit = request.limit.unwrap_or(20);
    if !(1..=100).contains(&limit) || request.offset > 100_000 {
        return Err(AppError::usage(
            "record limit must be 1..100 and offset at most 100000",
        ));
    }
    if let Some(scope) = &request.scope {
        validate_scope(scope)?;
    }
    if let Some(query) = &request.query {
        bounded(query, 1024, "record query")?;
    }
    let transaction = connection
        .unchecked_transaction()
        .map_err(AppError::database)?;
    let mut statement = transaction.prepare("SELECT r.id FROM records r JOIN record_revisions v ON v.record_id=r.id AND v.revision=r.revision WHERE r.forgotten=0 AND (?1 IS NULL OR r.scope=?1) AND (?2 IS NULL OR instr(lower(json_extract(v.body,'$.statement')),lower(?2))>0) ORDER BY v.recorded_at DESC,r.id LIMIT ?3 OFFSET ?4").map_err(AppError::database)?;
    let mut ids = statement
        .query_map(
            params![
                request.scope.as_ref().map(encode).transpose()?,
                request.query,
                i64::try_from(limit + 1)
                    .map_err(|_| AppError::usage("record limit out of range"))?,
                i64::try_from(request.offset)
                    .map_err(|_| AppError::usage("record offset out of range"))?
            ],
            |row| row.get::<_, String>(0),
        )
        .map_err(AppError::database)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::database)?;
    let mut next_offset = (ids.len() > limit).then_some(request.offset + limit);
    ids.truncate(limit);
    let timestamp = now()?;
    let mut records = Vec::new();
    let mut bytes = 0;
    for (position, id) in ids.into_iter().enumerate() {
        let record = read(&transaction, &id, false)?;
        if !request.include_inactive
            && !record.revisions.first().is_some_and(|revision| {
                revision.body.review == Review::Established
                    && revision.body.applies_at(timestamp)
                    && revision.evidence_current
            })
        {
            continue;
        }
        let size = encode(&record)?.len();
        if bytes + size > reading::LIST_RECORD_BYTES && !records.is_empty() {
            next_offset = Some(request.offset + position);
            break;
        }
        bytes += size;
        records.push(record);
    }
    Ok(Page {
        records,
        next_offset,
        next_cursor: None,
    })
}
