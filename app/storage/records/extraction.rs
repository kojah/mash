use super::{Storage, bounded, change, conflict, decode, encode, now, validate_scope};
use crate::{
    AppError,
    records::{
        Change, Mutation, Review, Scope,
        extraction::{Inspect, Request, Result as Receipt, State},
    },
};
use rusqlite::{Connection, OptionalExtension, params};

fn source(connection: &Connection, scope: &Scope, chat: &str) -> Result<String, AppError> {
    validate_scope(scope)?;
    bounded(chat, 512, "extraction chat ID")?;
    let row = connection
        .query_row(
            "SELECT source_fingerprint,repository,project FROM chats WHERE id=?1",
            [chat],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()
        .map_err(AppError::database)?
        .ok_or_else(|| conflict("extraction chat is missing or forgotten"))?;
    let (fingerprint, repository, project) = row;
    let in_scope = match scope {
        Scope::Personal => true,
        Scope::Chat { id } => id == chat,
        Scope::Repository { identity } => repository.as_ref() == Some(identity),
        Scope::LocalProject { path } => project.as_ref() == Some(path),
    };
    if !in_scope {
        return Err(AppError::usage(
            "extraction chat is outside the requested scope",
        ));
    }
    if fingerprint.len() != 64
        || !fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(AppError::usage(
            "chat has no canonical source fingerprint; reindex before extracting records",
        ));
    }
    Ok(fingerprint)
}

impl Storage {
    pub(crate) fn extraction_checkpoint(&self, request: &Inspect) -> Result<State, AppError> {
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(AppError::database)?;
        let source_fingerprint = source(&transaction, &request.scope, &request.chat_id)?;
        let processed_fingerprint: Option<String> = transaction
            .query_row(
                "SELECT source_fingerprint FROM record_checkpoints WHERE scope=?1 AND chat_id=?2",
                params![encode(&request.scope)?, request.chat_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(AppError::database)?;
        let up_to_date = processed_fingerprint.as_ref() == Some(&source_fingerprint);
        Ok(State {
            source_fingerprint,
            processed_fingerprint,
            up_to_date,
        })
    }

    pub(crate) fn extract_records(&mut self, request: &Request) -> Result<Receipt, AppError> {
        bounded(&request.operation_id, 256, "extraction operation ID")?;
        let serialized = encode(request)?;
        if request.records.len() > 64 || serialized.len() > 2_000_000 {
            return Err(AppError::usage(
                "extraction accepts at most 64 records and 2000000 request bytes",
            ));
        }
        let digest = blake3::hash(serialized.as_bytes()).to_hex().to_string();
        let transaction = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(AppError::database)?;
        let previous: Option<(String, String)> = transaction
            .query_row(
                "SELECT request_digest,result FROM record_extractions WHERE operation_id=?1",
                [&request.operation_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(AppError::database)?;
        if let Some((saved, result)) = previous {
            if saved != digest {
                return Err(conflict(
                    "extraction operation ID was already used for a different request",
                ));
            }
            return decode(&result);
        }
        let fingerprint = source(&transaction, &request.scope, &request.chat_id)?;
        if fingerprint != request.source_fingerprint {
            return Err(conflict(
                "chat changed since extraction began; reread before committing",
            ));
        }
        let timestamp = now()?;
        let operation_hash = blake3::hash(request.operation_id.as_bytes()).to_hex();
        let mut records = Vec::new();
        for (index, draft) in request.records.iter().enumerate() {
            if draft.body.review != Review::Candidate {
                return Err(AppError::usage(
                    "extraction creates candidates only; review them explicitly before establishing conclusions",
                ));
            }
            for citation in &draft.evidence {
                let in_chat: bool = transaction
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM messages WHERE id=?1 AND chat_id=?2)",
                        params![citation.message_id, request.chat_id],
                        |row| row.get(0),
                    )
                    .map_err(AppError::database)?;
                if !in_chat {
                    return Err(AppError::usage(
                        "extraction evidence must belong to the reviewed chat",
                    ));
                }
            }
            let mutation = Mutation {
                operation_id: format!("extraction/{operation_hash}/{index}"),
                change: Change::Create {
                    scope: request.scope.clone(),
                    body: draft.body.clone(),
                    evidence: draft.evidence.clone(),
                    on_duplicate: draft.on_duplicate.clone(),
                },
            };
            records.push(change(&transaction, &mutation, timestamp)?);
        }
        transaction.execute("INSERT INTO record_checkpoints(scope,chat_id,source_fingerprint) VALUES(?1,?2,?3) ON CONFLICT(scope,chat_id) DO UPDATE SET source_fingerprint=excluded.source_fingerprint", params![encode(&request.scope)?, request.chat_id, fingerprint]).map_err(AppError::database)?;
        let result = Receipt {
            source_fingerprint: fingerprint,
            records,
        };
        transaction
            .execute(
                "INSERT INTO record_extractions VALUES(?1,?2,?3)",
                params![request.operation_id, digest, encode(&result)?],
            )
            .map_err(AppError::database)?;
        transaction.commit().map_err(AppError::database)?;
        Ok(result)
    }
}
