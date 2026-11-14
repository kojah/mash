//! Transactional record state. Source refresh and privacy forgetting are distinct.
use super::Storage;

mod capture;
mod extraction;
mod matches;
mod reading;
mod recall;
mod relations;
#[cfg(test)]
mod tests;
pub mod transfer;
use crate::{
    AppError,
    records::{
        self, Body, Change, Citation, Details, Evidence, EvidenceUse, Expected, List, Mutation,
        Page, Receipt, Record, Review, Scope, bounded, conflict, decode, encode,
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

/// Transcript messages cited before indexing has retained them, by message ID.
type Live<'a> = std::collections::BTreeMap<&'a str, &'a records::live::Message>;

/// Replace session citations with the message that contains each quote:
/// the latest retained one, or else the one read from the live transcript.
fn resolve_sessions<'a>(
    connection: &Connection,
    request: &Mutation,
    transcript: &'a [records::live::Message],
) -> Result<(Mutation, Live<'a>), AppError> {
    let mut request = request.clone();
    let mut live = Live::new();
    for citation in request.change.citations_mut().into_iter().flatten() {
        let Some(session) = citation.session_id.take() else {
            if citation.message_id.is_empty() {
                return Err(AppError::usage(
                    "each citation needs a message_id or a session_id",
                ));
            }
            continue;
        };
        if !citation.message_id.is_empty() {
            return Err(AppError::usage(
                "cite a message_id or a session_id, not both",
            ));
        }
        bounded(&session, 512, "citation session ID")?;
        bounded(&citation.quote, 8192, "quote")?;
        let mut chats = connection
            .prepare("SELECT id FROM chats WHERE id=?1 OR source_session=?1 ORDER BY id LIMIT 2")
            .map_err(AppError::database)?;
        let chats = chats
            .query_map([&session], |row| row.get::<_, String>(0))
            .map_err(AppError::database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::database)?;
        if chats.len() > 1 {
            return Err(AppError::usage(
                "cited session matches multiple chats; use its chat ID",
            ));
        }
        // An endorsement must be the person's own words, so only their messages match.
        // It is also what they visibly said, not text a tool result repeats.
        let role = (citation.purpose == EvidenceUse::Endorsement).then_some("user");
        let retained: Option<String> = match chats.first() {
            Some(chat) => connection
                .query_row(
                    "SELECT id FROM messages WHERE chat_id=?1 AND instr(CASE WHEN ?3 IS NULL THEN content ELSE COALESCE(search_projection,content) END,?2)>0 AND (?3 IS NULL OR role=?3) ORDER BY ordinal DESC LIMIT 1",
                    params![chat, citation.quote, role],
                    |row| row.get(0),
                )
                .optional()
                .map_err(AppError::database)?,
            None => None,
        };
        citation.message_id = if let Some(id) = retained {
            id
        } else if let Some(message) = transcript
            .iter()
            .find(|message| message.session_id == session && message.quote == citation.quote)
        {
            live.insert(&message.id, message);
            message.id.clone()
        } else {
            return Err(conflict(if role.is_some() {
                "endorsement quote was not found in the person's messages in that session; copy their exact words"
            } else {
                "quote was not found in that session; copy the exact words from a message"
            }));
        };
    }
    Ok((request, live))
}

fn evidence(
    connection: &Connection,
    scope: &Scope,
    citations: &[Citation],
    endorsement: bool,
    live: &Live<'_>,
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
        ).optional().map_err(AppError::database)?
        // A transcript message stands in until indexing retains it.
        .or_else(|| live.get(citation.message_id.as_str()).map(|message| (
            message.chat_id.clone(),
            message.content.clone(),
            message.role.as_str().to_owned(),
            message.repository.clone(),
            message.project.clone(),
        )))
        .ok_or_else(|| conflict("cited message is missing"))?;
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
        && !body.details.agent_decided()
        && matches!(
            body.details,
            Details::Decision { .. } | Details::Preference { .. }
        )
}

/// A settled agent decision rests on the agent's own message making it.
fn agent_authored(body: &Body, sources: &[Evidence]) -> Result<(), AppError> {
    let cited = sources
        .iter()
        .any(|source| source.source_role == Some(crate::role::Role::Assistant));
    if body.review == Review::Established && body.details.agent_decided() && !cited {
        return Err(AppError::usage(
            "an established agent decision must cite the agent's own message where it made the choice",
        ));
    }
    Ok(())
}

/// The person's decision stays theirs: an agent's choice cannot replace it.
fn keeps_author(previous: &Body, body: &Body) -> Result<(), AppError> {
    if matches!(previous.details, Details::Decision { .. })
        && !previous.details.agent_decided()
        && body.details.agent_decided()
    {
        return Err(AppError::usage(
            "an agent decision cannot replace the person's decision; record the person's new choice with their endorsement",
        ));
    }
    Ok(())
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
    live: &Live<'_>,
) -> Result<(), AppError> {
    validate_scope(scope)?;
    body.validate()?;
    if body.details.superseded() {
        return Err(AppError::usage(
            "create a successor through supersede, not an already superseded record",
        ));
    }
    let sources = evidence(
        connection,
        scope,
        citations,
        requires_endorsement(body),
        live,
    )?;
    agent_authored(body, &sources)?;
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
    live: &Live<'_>,
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
            let sources = self::evidence(
                connection,
                scope,
                evidence,
                requires_endorsement(body),
                live,
            )?;
            agent_authored(body, &sources)?;
            if let Some(receipt) = matches::duplicate(connection, scope, body, timestamp)? {
                if matches!(on_duplicate, records::DuplicatePolicy::Report) {
                    return Err(conflict(&format!(
                        "exact live record already exists: {} at revision {}; inspect it or explicitly set on_duplicate=reuse",
                        receipt.record_id, receipt.revision
                    )));
                }
                return Ok(receipt);
            }
            create(connection, &id, scope, body, evidence, timestamp, live)?;
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
        } => revise(connection, expected, body, citations, timestamp, live),
        Change::Supersede {
            expected,
            body,
            evidence: citations,
        } => supersede(connection, id, expected, body, citations, timestamp, live),
        Change::Confirm {
            expected,
            evidence: citations,
        } => confirm(connection, expected, citations, timestamp, live),
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
    live: &Live<'_>,
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
    keeps_author(&previous, body)?;
    let sources = evidence(
        connection,
        &scope,
        citations,
        requires_endorsement(body),
        live,
    )?;
    agent_authored(body, &sources)?;
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
    live: &Live<'_>,
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
    keeps_author(&previous, body)?;
    previous.details.supersede()?;
    create(connection, &id, &scope, body, citations, timestamp, live)?;
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
    live: &Live<'_>,
) -> Result<Receipt, AppError> {
    let (scope, body, _) = current(connection, expected)?;
    if body.review != Review::Established || !body.details.current() {
        return Err(AppError::usage(
            "only established current records can be confirmed; to establish a reviewed candidate, use revise with body.review=established and verified evidence",
        ));
    }
    let sources = evidence(
        connection,
        &scope,
        citations,
        requires_endorsement(&body),
        live,
    )?;
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
            let human = source.source_role == Some(crate::role::Role::User);
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

pub fn purge(connection: &Connection, id: &str) -> Result<(), AppError> {
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
pub fn forget_chat(connection: &Connection, chat: &str) -> Result<usize, AppError> {
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

pub use reading::{read, read_page};
impl Storage {
    #[cfg(any(test, feature = "test-support"))]
    pub fn mutate_record(&mut self, request: &Mutation) -> Result<Receipt, AppError> {
        self.mutate_record_citing(request, &[])
    }

    /// Apply a record change. `transcript` holds quotes read from cited live
    /// sessions, used only where retained history lacks the quoted message.
    pub fn mutate_record_citing(
        &mut self,
        request: &Mutation,
        transcript: &[records::live::Message],
    ) -> Result<Receipt, AppError> {
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
        let (resolved, live) = resolve_sessions(&transaction, request, transcript)?;
        let result = change(&transaction, &resolved, now()?, &live)?;
        transaction
            .execute(
                "INSERT INTO record_operations VALUES(?1,?2,?3)",
                params![request.operation_id, digest, encode(&result)?],
            )
            .map_err(AppError::database)?;
        transaction.commit().map_err(AppError::database)?;
        Ok(result)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn record(&self, id: &str, history: bool) -> Result<Record, AppError> {
        bounded(id, 512, "record ID")?;
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(AppError::database)?;
        read(&transaction, id, history)
    }

    pub fn records(&self, request: &List) -> Result<Page, AppError> {
        list(&self.connection, request)
    }

    pub fn record_page(&self, request: &records::Get) -> Result<Record, AppError> {
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(AppError::database)?;
        read_page(&transaction, request)
    }
}

pub fn list(connection: &Connection, request: &List) -> Result<Page, AppError> {
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
    if let Some(chat) = &request.chat {
        bounded(chat, 512, "record chat ID")?;
    }
    let transaction = connection
        .unchecked_transaction()
        .map_err(AppError::database)?;
    let mut statement = transaction.prepare("SELECT r.id FROM records r JOIN record_revisions v ON v.record_id=r.id AND v.revision=r.revision WHERE r.forgotten=0 AND (?1 IS NULL OR r.scope=?1) AND (?2 IS NULL OR instr(lower(json_extract(v.body,'$.statement')),lower(?2))>0) AND (?5 IS NULL OR r.kind=json_extract(?5,'$')) AND (?6 IS NULL OR json_extract(v.body,'$.details.state')=json_extract(?6,'$')) AND (?7 IS NULL OR json_extract(v.body,'$.review')=json_extract(?7,'$')) AND (?8 IS NULL OR EXISTS(SELECT 1 FROM record_evidence e WHERE e.record_id=r.id AND e.chat_id=?8)) ORDER BY v.recorded_at DESC,r.id LIMIT ?3 OFFSET ?4").map_err(AppError::database)?;
    let mut ids = statement
        .query_map(
            params![
                request.scope.as_ref().map(encode).transpose()?,
                request.query,
                i64::try_from(limit + 1)
                    .map_err(|_| AppError::usage("record limit out of range"))?,
                i64::try_from(request.offset)
                    .map_err(|_| AppError::usage("record offset out of range"))?,
                request.kind.as_ref().map(encode).transpose()?,
                request.state.as_ref().map(encode).transpose()?,
                request.review.as_ref().map(encode).transpose()?,
                request.chat,
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
