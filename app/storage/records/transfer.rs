//! Stream native record history without lossy prompt projections.
use crate::{
    AppError,
    records::{bounded, decode, encode, transfer::Row},
};
use rusqlite::{Connection, params};

pub(crate) fn visit(
    connection: &Connection,
    mut emit: impl FnMut(Row) -> Result<(), AppError>,
) -> Result<(), AppError> {
    macro_rules! table {
        ($sql:expr,$convert:expr) => {{
            let mut statement = connection.prepare($sql).map_err(AppError::database)?;
            let mut rows = statement.query([]).map_err(AppError::database)?;
            while let Some(row) = rows.next().map_err(AppError::database)? {
                let item: Row = ($convert)(row)?;
                emit(item)?;
            }
        }};
    }
    fn value<T: rusqlite::types::FromSql>(
        row: &rusqlite::Row<'_>,
        index: usize,
    ) -> Result<T, AppError> {
        row.get(index).map_err(AppError::database)
    }
    emit(Row::Owner {
        id: super::owner(connection)?,
    })?;
    table!(
        "SELECT id,owner,kind,scope,revision,forgotten FROM records ORDER BY id",
        |row| Ok::<_, AppError>(Row::Header {
            id: value(row, 0)?,
            owner: value(row, 1)?,
            kind: decode(&encode(&value::<String>(row, 2)?)?)?,
            scope: decode(&value::<String>(row, 3)?)?,
            revision: value(row, 4)?,
            forgotten: value(row, 5)?
        })
    );
    table!(
        "SELECT record_id,revision,body,recorded_at FROM record_revisions ORDER BY record_id,revision",
        |row| Ok::<_, AppError>(Row::Revision {
            id: value(row, 0)?,
            revision: value(row, 1)?,
            body: decode(&value::<String>(row, 2)?)?,
            recorded_at: value(row, 3)?
        })
    );
    table!(
        "SELECT record_id,revision,position,chat_id,message_id,fingerprint,quote,purpose,source_role FROM record_evidence ORDER BY record_id,revision,position",
        |row| Ok::<_, AppError>(Row::Evidence {
            id: value(row, 0)?,
            revision: value(row, 1)?,
            position: value(row, 2)?,
            source: crate::records::Evidence {
                chat_id: value(row, 3)?,
                message_id: value(row, 4)?,
                fingerprint: value(row, 5)?,
                quote: value(row, 6)?,
                purpose: value(row, 7)?,
                source_role: value(row, 8)?
            }
        })
    );
    table!(
        "SELECT source_id,target_id,relation FROM record_links ORDER BY source_id,target_id,relation",
        |row| Ok::<_, AppError>(Row::Link {
            link: crate::records::Link {
                source_id: value(row, 0)?,
                target_id: value(row, 1)?,
                relation: decode(&encode(&value::<String>(row, 2)?)?)?
            }
        })
    );
    table!(
        "SELECT record_id,revision,message_id,confirmed_at FROM record_confirmations ORDER BY record_id,revision,message_id",
        |row| Ok::<_, AppError>(Row::Confirmation {
            id: value(row, 0)?,
            revision: value(row, 1)?,
            message_id: value(row, 2)?,
            confirmed_at: value(row, 3)?
        })
    );
    table!(
        "SELECT operation_id,request_digest,result FROM record_operations ORDER BY operation_id",
        |row| Ok::<_, AppError>(Row::Operation {
            id: value(row, 0)?,
            digest: value(row, 1)?,
            result: decode(&value::<String>(row, 2)?)?
        })
    );
    table!(
        "SELECT scope,chat_id,source_fingerprint FROM record_checkpoints ORDER BY scope,chat_id",
        |row| Ok::<_, AppError>(Row::Checkpoint {
            scope: decode(&value::<String>(row, 0)?)?,
            chat_id: value(row, 1)?,
            source_fingerprint: value(row, 2)?
        })
    );
    Ok(())
}

pub(crate) struct Import {
    source_owner: Option<String>,
    target_owner: String,
    keep_identity: bool,
}

impl Import {
    pub(crate) fn new(connection: &Connection, keep_identity: bool) -> Result<Self, AppError> {
        Ok(Self {
            source_owner: None,
            target_owner: super::owner(connection)?,
            keep_identity,
        })
    }

    fn id(&self, id: &str) -> Result<String, AppError> {
        bounded(id, 512, "record ID")?;
        Ok(if self.keep_identity {
            id.into()
        } else {
            format!(
                "record/{}/{}",
                self.target_owner,
                blake3::hash(id.as_bytes()).to_hex()
            )
        })
    }

    #[allow(
        clippy::too_many_lines,
        reason = "exhaustive typed canonical import mapping"
    )]
    pub(crate) fn insert(&mut self, connection: &Connection, row: Row) -> Result<(), AppError> {
        if let Row::Owner { id } = row {
            bounded(&id, 256, "record owner")?;
            if self.source_owner.is_some() {
                return Err(AppError::usage("duplicate record owner in archive"));
            }
            if self.keep_identity {
                connection
                    .execute("UPDATE record_owner SET id=?1 WHERE singleton=1", [&id])
                    .map_err(AppError::database)?;
                self.target_owner.clone_from(&id);
            }
            self.source_owner = Some(id);
            return Ok(());
        }
        let Some(source_owner) = &self.source_owner else {
            return Err(AppError::usage("record archive lacks owner identity"));
        };
        match row {
            Row::Owner { .. } => return Err(AppError::usage("duplicate record owner")),
            Row::Header {
                id,
                owner,
                kind,
                scope,
                revision,
                forgotten,
            } => {
                if &owner != source_owner
                    || !(1..=1001).contains(&revision)
                    || forgotten != scope.is_none()
                {
                    return Err(AppError::usage("invalid archived record identity"));
                }
                if let Some(scope) = &scope {
                    super::validate_scope(scope)?;
                }
                connection
                    .execute(
                        "INSERT INTO records VALUES(?1,?2,?3,?4,?5,?6)",
                        params![
                            self.id(&id)?,
                            self.target_owner,
                            kind.as_str(),
                            encode(&scope)?,
                            revision,
                            forgotten
                        ],
                    )
                    .map_err(AppError::database)?;
            }
            Row::Revision {
                id,
                revision,
                body,
                recorded_at,
            } => {
                body.validate()?;
                let id = self.id(&id)?;
                let valid: bool = connection
                    .query_row(
                        "SELECT forgotten=0 AND kind=?2 AND revision>=?3 FROM records WHERE id=?1",
                        params![id, body.details.kind(), revision],
                        |row| row.get(0),
                    )
                    .map_err(AppError::database)?;
                if !valid || !(1..=1000).contains(&revision) {
                    return Err(AppError::usage("invalid archived record revision"));
                }
                connection
                    .execute(
                        "INSERT INTO record_revisions VALUES(?1,?2,?3,?4)",
                        params![id, revision, encode(&body)?, recorded_at],
                    )
                    .map_err(AppError::database)?;
            }
            Row::Evidence {
                id,
                revision,
                position,
                source,
            } => {
                bounded(&source.chat_id, 512, "evidence chat ID")?;
                bounded(&source.message_id, 512, "evidence message ID")?;
                bounded(&source.quote, 8192, "evidence quote")?;
                if let Some(role) = &source.source_role {
                    bounded(role.as_str(), 256, "evidence source role")?;
                }
                digest(&source.fingerprint)?;
                if !(0..256).contains(&position)
                    || crate::redaction::private(&source.quote) != source.quote
                {
                    return Err(AppError::usage("invalid archived evidence"));
                }
                connection
                    .execute(
                        "INSERT INTO record_evidence VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                        params![
                            self.id(&id)?,
                            revision,
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
            }
            Row::Link { link } => {
                let source = self.id(&link.source_id)?;
                let target = self.id(&link.target_id)?;
                let same_scope:bool=connection.query_row("SELECT a.owner=b.owner AND a.scope=b.scope AND a.forgotten=0 AND b.forgotten=0 FROM records a,records b WHERE a.id=?1 AND b.id=?2",params![source,target],|row|row.get(0)).map_err(AppError::database)?;
                if !same_scope {
                    return Err(AppError::usage("record link crosses owner or scope"));
                }
                let relation: String = decode(&encode(&link.relation)?)?;
                connection
                    .execute(
                        "INSERT INTO record_links VALUES(?1,?2,?3)",
                        params![source, target, relation],
                    )
                    .map_err(AppError::database)?;
            }
            Row::Confirmation {
                id,
                revision,
                message_id,
                confirmed_at,
            } => {
                bounded(&message_id, 512, "confirmation message ID")?;
                let id = self.id(&id)?;
                let cited:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM record_evidence WHERE record_id=?1 AND revision=?2 AND message_id=?3)",params![id,revision,message_id],|row|row.get(0)).map_err(AppError::database)?;
                if !cited {
                    return Err(AppError::usage("confirmation has no retained evidence"));
                }
                connection
                    .execute(
                        "INSERT INTO record_confirmations VALUES(?1,?2,?3,?4)",
                        params![id, revision, message_id, confirmed_at],
                    )
                    .map_err(AppError::database)?;
            }
            Row::Operation {
                id,
                digest: hash,
                result,
            } => {
                bounded(&id, 256, "record operation ID")?;
                digest(&hash)?;
                self.validate_receipt(connection, &result.record_id, result.revision)?;
                if let Some(related) = &result.related {
                    self.validate_receipt(connection, &related.id, related.revision)?;
                }
                // A copy is a new writer. Old operation digests refer to the old
                // IDs, so only identity-preserving recovery restores receipts.
                if self.keep_identity {
                    connection
                        .execute(
                            "INSERT INTO record_operations VALUES(?1,?2,?3)",
                            params![id, hash, encode(&result)?],
                        )
                        .map_err(AppError::database)?;
                }
            }
            Row::Checkpoint {
                scope,
                chat_id,
                source_fingerprint,
            } => {
                super::validate_scope(&scope)?;
                bounded(&chat_id, 512, "checkpoint chat ID")?;
                bounded(&source_fingerprint, 256, "checkpoint fingerprint")?;
                connection
                    .execute(
                        "INSERT INTO record_checkpoints VALUES(?1,?2,?3)",
                        params![encode(&scope)?, chat_id, source_fingerprint],
                    )
                    .map_err(AppError::database)?;
            }
        }
        Ok(())
    }

    fn validate_receipt(
        &self,
        connection: &Connection,
        id: &str,
        revision: i64,
    ) -> Result<(), AppError> {
        let valid: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM records WHERE id=?1 AND revision>=?2)",
                params![self.id(id)?, revision],
                |row| row.get(0),
            )
            .map_err(AppError::database)?;
        if revision < 1 || !valid {
            return Err(AppError::usage("record receipt references missing history"));
        }
        Ok(())
    }

    pub(crate) fn finish(connection: &Connection) -> Result<(), AppError> {
        let invalid:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM records r WHERE r.forgotten=0 AND NOT EXISTS(SELECT 1 FROM record_revisions v WHERE v.record_id=r.id AND v.revision=r.revision))",[],|row|row.get(0)).map_err(AppError::database)?;
        if invalid {
            return Err(AppError::usage("record archive lacks its current revision"));
        }
        let incomplete:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM records r WHERE r.forgotten=0 AND r.revision<>(SELECT count(*) FROM record_revisions v WHERE v.record_id=r.id)) OR EXISTS(SELECT 1 FROM record_revisions v WHERE NOT EXISTS(SELECT 1 FROM record_evidence e WHERE e.record_id=v.record_id AND e.revision=v.revision))",[],|row|row.get(0)).map_err(AppError::database)?;
        if incomplete {
            return Err(AppError::usage(
                "record archive has incomplete revision history or evidence",
            ));
        }
        let cycle:bool=connection.query_row("WITH RECURSIVE paths(source,target) AS (SELECT source_id,target_id FROM record_links WHERE relation='supersedes' UNION SELECT p.source,l.target_id FROM paths p JOIN record_links l ON l.source_id=p.target WHERE l.relation='supersedes') SELECT EXISTS(SELECT 1 FROM paths WHERE source=target)",[],|row|row.get(0)).map_err(AppError::database)?;
        if cycle {
            return Err(AppError::usage(
                "record archive contains a supersession cycle",
            ));
        }
        let invalid_link:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM record_links l JOIN records a ON a.id=l.source_id JOIN records b ON b.id=l.target_id JOIN record_revisions v ON v.record_id=b.id AND v.revision=b.revision WHERE l.relation='supersedes' AND (a.kind<>b.kind OR json_extract(v.body,'$.details.state')<>'superseded'))",[],|row|row.get(0)).map_err(AppError::database)?;
        if invalid_link {
            return Err(AppError::usage(
                "invalid supersession state in record archive",
            ));
        }
        // A forgotten source may not return indirectly through retained summaries.
        let mut statement=connection.prepare("SELECT DISTINCT chat_id FROM tombstones WHERE chat_id IN (SELECT chat_id FROM record_evidence)").map_err(AppError::database)?;
        let chats = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(AppError::database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::database)?;
        if !chats.is_empty() {
            return Err(AppError::usage(
                "record archive retains evidence from forgotten chats",
            ));
        }
        Ok(())
    }
}

fn digest(value: &str) -> Result<(), AppError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(AppError::usage("invalid record fingerprint"));
    }
    Ok(())
}
