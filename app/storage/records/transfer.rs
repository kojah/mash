//! Stream native record history without lossy prompt projections.
use crate::{
    AppError,
    records::{bounded, decode, encode, transfer::Row},
};
use rusqlite::{Connection, OptionalExtension, params};

/// Select owner-authored changes, excluding machine-local project scopes.
pub(crate) fn visit_shared(
    connection: &Connection,
    after: Option<i64>,
    policy: crate::redaction::Policy,
    mut emit: impl FnMut(Row) -> Result<(), AppError>,
) -> Result<(), AppError> {
    visit_selected(connection, Selection::Shared { after }, |mut row| {
        if let Row::Evidence { source, .. } = &mut row {
            let content: Option<String> = connection
                .query_row(
                    "SELECT content FROM messages WHERE id=?1 AND chat_id=?2",
                    params![source.message_id, source.chat_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(AppError::database)?;
            source.fingerprint = match content {
                Some(content)
                    if blake3::hash(content.as_bytes()).to_hex().as_str() == source.fingerprint =>
                {
                    blake3::hash(policy.text(&content).as_bytes())
                        .to_hex()
                        .to_string()
                }
                _ => "0".repeat(64),
            };
        }
        row.redact(policy);
        emit(row)
    })
}

pub(crate) fn fingerprint(connection: &Connection) -> Result<String, AppError> {
    let mut hash = blake3::Hasher::new();
    visit_selected(connection, Selection::Canonical, |row| {
        let bytes = serde_json::to_vec(&row)
            .map_err(|_| AppError::internal("serialize record fingerprint"))?;
        hash.update(&(bytes.len() as u64).to_le_bytes());
        hash.update(&bytes);
        Ok(())
    })?;
    Ok(hash.finalize().to_hex().to_string())
}

pub(crate) fn visit(
    connection: &Connection,
    emit: impl FnMut(Row) -> Result<(), AppError>,
) -> Result<(), AppError> {
    visit_selected(connection, Selection::Archive, emit)
}

#[derive(Clone, Copy)]
enum Selection {
    Archive,
    Canonical,
    Shared { after: Option<i64> },
}

#[expect(
    clippy::too_many_lines,
    reason = "Keep the fixed archive table order and typed row mappings together."
)]
fn visit_selected(
    connection: &Connection,
    selection: Selection,
    mut emit: impl FnMut(Row) -> Result<(), AppError>,
) -> Result<(), AppError> {
    macro_rules! table {
        ($table:literal, $columns:literal, $key:literal, $order:literal, $convert:expr) => {{
            // Every identifier comes from the fixed table declarations below;
            // the only input value (revision) is bound as a SQL parameter.
            let predicate = match selection {
                Selection::Shared { .. } => concat!(" WHERE ", $key, " IN (SELECT r.id FROM records r WHERE (r.forgotten=1 OR json_extract(r.scope,'$.kind')<>'local_project') AND (?1 IS NULL OR r.id IN (SELECT record_id FROM record_changes WHERE revision>?1)))"),
                _ => "",
            };
            let sql = format!("SELECT {} FROM {}{} ORDER BY {}", $columns, $table, predicate, $order);
            let mut statement = connection.prepare(&sql).map_err(AppError::database)?;
            let mut rows = match selection {
                Selection::Shared { after } => statement.query([after]),
                _ => statement.query([]),
            }.map_err(AppError::database)?;
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
        "records",
        "id,owner,kind,scope,revision,forgotten",
        "id",
        "id",
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
        "record_revisions",
        "record_id,revision,body,recorded_at",
        "record_id",
        "record_id,revision",
        |row| Ok::<_, AppError>(Row::Revision {
            id: value(row, 0)?,
            revision: value(row, 1)?,
            body: decode(&value::<String>(row, 2)?)?,
            recorded_at: value(row, 3)?
        })
    );
    table!(
        "record_evidence",
        "record_id,revision,position,chat_id,message_id,fingerprint,quote,purpose,source_role",
        "record_id",
        "record_id,revision,position",
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
        "record_links",
        "source_id,target_id,relation",
        "source_id",
        "source_id,target_id,relation",
        |row| Ok::<_, AppError>(Row::Link {
            link: crate::records::Link {
                source_id: value(row, 0)?,
                target_id: value(row, 1)?,
                relation: decode(&encode(&value::<String>(row, 2)?)?)?
            }
        })
    );
    table!(
        "record_confirmations",
        "record_id,revision,message_id,confirmed_at",
        "record_id",
        "record_id,revision,message_id",
        |row| Ok::<_, AppError>(Row::Confirmation {
            id: value(row, 0)?,
            revision: value(row, 1)?,
            message_id: value(row, 2)?,
            confirmed_at: value(row, 3)?
        })
    );
    if !matches!(selection, Selection::Archive) {
        return Ok(());
    }
    table!(
        "record_operations",
        "operation_id,request_digest,result",
        "operation_id",
        "operation_id",
        |row| Ok::<_, AppError>(Row::Operation {
            id: value(row, 0)?,
            digest: value(row, 1)?,
            result: decode(&value::<String>(row, 2)?)?
        })
    );
    table!(
        "record_extractions",
        "operation_id,request_digest,result",
        "operation_id",
        "operation_id",
        |row| Ok::<_, AppError>(Row::ExtractionOperation {
            id: value(row, 0)?,
            digest: value(row, 1)?,
            result: decode(&value::<String>(row, 2)?)?
        })
    );
    table!(
        "record_checkpoints",
        "scope,chat_id,source_fingerprint",
        "chat_id",
        "scope,chat_id",
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
    shared_snapshot: Option<bool>,
    seen_headers: std::collections::BTreeSet<String>,
}

impl Import {
    pub(crate) fn new(connection: &Connection, keep_identity: bool) -> Result<Self, AppError> {
        Ok(Self {
            source_owner: None,
            target_owner: super::owner(connection)?,
            keep_identity,
            shared_snapshot: None,
            seen_headers: std::collections::BTreeSet::new(),
        })
    }

    pub(crate) fn shared(connection: &Connection, snapshot: bool) -> Result<Self, AppError> {
        let mut importer = Self::new(connection, true)?;
        connection.execute_batch("DROP TABLE IF EXISTS temp.recall_prior_headers;
            CREATE TEMP TABLE recall_prior_headers AS SELECT id,owner,kind,scope,revision,forgotten FROM records;").map_err(AppError::database)?;
        importer.shared_snapshot = Some(snapshot);
        Ok(importer)
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
            if let Some(snapshot) = self.shared_snapshot {
                let populated: bool = connection
                    .query_row("SELECT EXISTS(SELECT 1 FROM records)", [], |row| row.get(0))
                    .map_err(AppError::database)?;
                if populated && self.target_owner != id {
                    return Err(AppError::usage("shared record owner changed"));
                }
                if snapshot {
                    connection.execute_batch("DELETE FROM records WHERE forgotten=0; DELETE FROM record_operations; DELETE FROM record_extractions; DELETE FROM record_checkpoints; DELETE FROM record_changes;").map_err(AppError::database)?;
                }
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
                if !self.seen_headers.insert(id.clone()) {
                    return Err(AppError::usage("duplicate record header"));
                }
                if self.shared_snapshot.is_none() {
                    let exists: bool = connection
                        .query_row(
                            "SELECT EXISTS(SELECT 1 FROM records WHERE id=?1)",
                            [self.id(&id)?],
                            |row| row.get(0),
                        )
                        .map_err(AppError::database)?;
                    if exists {
                        return Err(AppError::usage("record already exists in import target"));
                    }
                }
                if &owner != source_owner
                    || !(1..=1001).contains(&revision)
                    || forgotten != scope.is_none()
                {
                    return Err(AppError::usage("invalid archived record identity"));
                }
                if let Some(scope) = &scope {
                    super::validate_scope(scope)?;
                    if self.shared_snapshot.is_some()
                        && matches!(scope, crate::records::Scope::LocalProject { .. })
                    {
                        return Err(AppError::usage("local project records cannot be shared"));
                    }
                }
                if self.shared_snapshot.is_some() {
                    let id = self.id(&id)?;
                    let invalid: bool = connection.query_row(
                        "SELECT EXISTS(SELECT 1 FROM recall_prior_headers WHERE id=?1 AND
                            (owner<>?2 OR kind<>?3 OR revision>?4 OR (forgotten=1 AND ?5=0) OR (?5=0 AND scope<>?6)))",
                        params![id,owner,kind.as_str(),revision,forgotten,encode(&scope)?], |row| row.get(0),
                    ).map_err(AppError::database)?;
                    if invalid {
                        return Err(AppError::usage(
                            "shared record identity changed or revision went backwards",
                        ));
                    }
                    connection
                        .execute("DELETE FROM record_revisions WHERE record_id=?1", [&id])
                        .map_err(AppError::database)?;
                    connection
                        .execute(
                            "DELETE FROM record_links WHERE source_id=?1 OR (?2 AND target_id=?1)",
                            params![id, forgotten],
                        )
                        .map_err(AppError::database)?;
                }
                connection
                    .execute(
                        "INSERT INTO records VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET owner=excluded.owner,kind=excluded.kind,scope=excluded.scope,revision=excluded.revision,forgotten=excluded.forgotten",
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
                self.require_header(&id)?;
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
                self.require_header(&id)?;
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
                self.require_header(&link.source_id)?;
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
                self.require_header(&id)?;
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
                if self.shared_snapshot.is_some() {
                    return Err(AppError::usage("operation receipts are owner-local"));
                }
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
            Row::ExtractionOperation {
                id,
                digest: hash,
                result,
            } => {
                if self.shared_snapshot.is_some() {
                    return Err(AppError::usage("extraction receipts are owner-local"));
                }
                bounded(&id, 256, "extraction operation ID")?;
                digest(&hash)?;
                digest(&result.source_fingerprint)?;
                if result.records.len() > 64 {
                    return Err(AppError::usage("too many extracted record receipts"));
                }
                for receipt in &result.records {
                    self.validate_receipt(connection, &receipt.record_id, receipt.revision)?;
                    if let Some(related) = &receipt.related {
                        self.validate_receipt(connection, &related.id, related.revision)?;
                    }
                }
                if self.keep_identity {
                    connection
                        .execute(
                            "INSERT INTO record_extractions VALUES(?1,?2,?3)",
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
                if self.shared_snapshot.is_some() {
                    return Err(AppError::usage("extraction checkpoints are owner-local"));
                }
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

    fn require_header(&self, id: &str) -> Result<(), AppError> {
        if self.shared_snapshot.is_some() && !self.seen_headers.contains(id) {
            return Err(AppError::usage(
                "shared record row has no replacement header",
            ));
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
        let opposite: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM record_links a JOIN record_links b
             ON ((a.source_id=b.source_id AND a.target_id=b.target_id) OR
                 (a.source_id=b.target_id AND a.target_id=b.source_id))
             WHERE a.relation='supports' AND b.relation='contradicts')",
                [],
                |row| row.get(0),
            )
            .map_err(AppError::database)?;
        if opposite {
            return Err(AppError::usage(
                "record archive contains opposite relationships for the same pair",
            ));
        }
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
        // Open questions and the links that resolved them are no longer kept.
        let retired: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM records WHERE kind='open_question') OR EXISTS(SELECT 1 FROM record_links WHERE relation='resolves')", [], |row| row.get(0)).map_err(AppError::database)?;
        if retired {
            return Err(AppError::usage(
                "this archive holds open questions, which Mash no longer keeps; forget them with the release that wrote the archive, then export again",
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
