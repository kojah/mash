//! Export canonical records through one read snapshot.
use super::Storage;
use crate::AppError;
use std::{
    collections::BTreeMap,
    io::{BufRead, Read, Write},
    path::Path,
};
mod records;

pub(crate) struct Snapshot {
    pub(crate) silver: crate::silver::certificate::Certificate,
    pub(crate) counts: crate::storage::Counts,
    pub(crate) files: BTreeMap<String, u64>,
}

pub(crate) fn write(source: &Path, destination: &Path) -> Result<Snapshot, AppError> {
    let history = Storage::open_catalog(source)?;
    let _transaction = history
        .connection
        .unchecked_transaction()
        .map_err(AppError::database)?;
    let snapshot = Snapshot {
        silver: history.silver_certificate()?,
        counts: history.counts()?,
        files: attachments(&history)?,
    };
    let mut temporary = tempfile::NamedTempFile::new_in(
        destination
            .parent()
            .ok_or_else(|| AppError::usage("invalid archive destination"))?,
    )
    .map_err(AppError::io)?;
    let mut output = std::io::BufWriter::with_capacity(64 * 1024, temporary.as_file_mut());
    records::visit(&history, &mut |record| {
        serde_json::to_writer(&mut output, &record)
            .map_err(|_| AppError::internal("serialize canonical archive"))?;
        output.write_all(b"\n").map_err(AppError::io)
    })?;
    output.flush().map_err(AppError::io)?;
    drop(output);
    temporary.as_file().sync_all().map_err(AppError::io)?;
    temporary
        .persist_noclobber(destination)
        .map_err(|error| AppError::io(error.error))?;
    Ok(snapshot)
}

/// Restore a canonical archive. Imports start a new publication identity unless
/// `keep_identity` continues the exporting node, whose database is retired.
pub(crate) fn restore(
    source: &Path,
    destination: &Path,
    keep_identity: bool,
) -> Result<(), AppError> {
    use crate::silver::archive::Record;
    const MAX_RECORD_BYTES: u64 = 64 * 1024 * 1024;
    let target = Storage::open(destination)?;
    let transaction = target
        .connection
        .unchecked_transaction()
        .map_err(AppError::database)?;
    let mut reader = std::io::BufReader::new(std::fs::File::open(source).map_err(AppError::io)?);
    let mut line = Vec::new();
    let mut state_seen = false;
    let mut identity_seen = false;
    loop {
        line.clear();
        let bytes = (&mut reader)
            .take(MAX_RECORD_BYTES + 1)
            .read_until(b'\n', &mut line)
            .map_err(AppError::io)?;
        if bytes == 0 {
            break;
        }
        if bytes as u64 > MAX_RECORD_BYTES || line.last() != Some(&b'\n') || state_seen {
            return Err(AppError::usage(
                "invalid or oversized canonical archive record",
            ));
        }
        let record: Record = serde_json::from_slice(&line)
            .map_err(|_| AppError::usage("invalid canonical archive record"))?;
        state_seen = matches!(record, Record::State { .. });
        identity_seen |= matches!(record, Record::Publication { .. });
        records::insert(&target, record)?;
    }
    if !state_seen {
        return Err(AppError::usage("incomplete canonical archive"));
    }
    if keep_identity && !identity_seen {
        return Err(AppError::usage(
            "export carries no publication identity to keep",
        ));
    }
    // The next publication is a full snapshot: the change journal starts empty.
    transaction
        .execute_batch(if keep_identity {
            "UPDATE shared_publication SET acknowledged=0,target='';
             DELETE FROM shared_changes;
             UPDATE shared_change_state SET floor=(SELECT revision FROM shared_publication);"
        } else {
            "UPDATE shared_publication SET instance=lower(hex(randomblob(16))),revision=1,acknowledged=0,target='';
             DELETE FROM shared_changes;
             UPDATE shared_change_state SET floor=1;"
        })
        .map_err(AppError::database)?;
    transaction.commit().map_err(AppError::database)?;
    target
        .connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;")
        .map_err(AppError::database)?;
    validate(&target)?;
    Ok(())
}

pub(crate) fn validate(storage: &Storage) -> Result<(), AppError> {
    let check: String = storage
        .connection
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(AppError::database)?;
    let foreign_keys = storage
        .connection
        .prepare("PRAGMA foreign_key_check")
        .map_err(AppError::database)?
        .exists([])
        .map_err(AppError::database)?;
    let gold: i64 = storage.connection.query_row(
        "SELECT (SELECT count(*) FROM message_embeddings)+(SELECT count(*) FROM semantic_chunks)
        +(SELECT count(*) FROM message_fts)+(SELECT count(*) FROM pending_embeddings)
        +(SELECT count(*) FROM gold_manifest WHERE manifest IS NOT NULL)
        +(SELECT count(*) FROM lexical_manifest WHERE manifest IS NOT NULL)
        +(SELECT count(*) FROM derived_state WHERE search_dirty<>1 OR semantic_ready_generation IS NOT NULL OR resume_generation IS NOT NULL)
        +(SELECT count(*) FROM shared_publication WHERE acknowledged<>0 OR target<>'')",
        [], |row| row.get(0)).map_err(AppError::database)?;
    if check != "ok" || foreign_keys || gold != 0 {
        return Err(AppError::database_data("invalid silver export database"));
    }
    Ok(())
}

pub(crate) fn attachments(storage: &Storage) -> Result<BTreeMap<String, u64>, AppError> {
    let mut files = BTreeMap::new();
    // Claude's branch evidence can retain files absent from the visible thread.
    for (query, format) in [
        (
            "SELECT attachments FROM messages",
            AttachmentFormat::Canonical,
        ),
        (
            "SELECT attachments FROM claude_source_messages",
            AttachmentFormat::ClaudeSource,
        ),
    ] {
        let mut statement = storage
            .connection
            .prepare(query)
            .map_err(AppError::database)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(AppError::database)?;
        for row in rows {
            let records = format.decode(&row.map_err(AppError::database)?)?;
            crate::attachments::validate_metadata(&records)?;
            for record in records {
                if record.availability == crate::attachments::Availability::Retained {
                    let digest = record
                        .digest
                        .ok_or_else(|| AppError::database_data("missing attachment digest"))?;
                    let bytes = record
                        .bytes
                        .ok_or_else(|| AppError::database_data("missing attachment size"))?;
                    if files.insert(digest, bytes).is_some_and(|old| old != bytes) {
                        return Err(AppError::database_data(
                            "conflicting export attachment sizes",
                        ));
                    }
                }
            }
        }
    }
    Ok(files)
}

enum AttachmentFormat {
    Canonical,
    ClaudeSource,
}

impl AttachmentFormat {
    fn decode(&self, value: &str) -> Result<Vec<crate::attachments::Metadata>, AppError> {
        let invalid = || AppError::database_data("invalid export attachment metadata");
        match self {
            Self::Canonical => serde_json::from_str(value).map_err(|_| invalid()),
            Self::ClaudeSource => {
                let sources: Vec<crate::attachments::Source> =
                    serde_json::from_str(value).map_err(|_| invalid())?;
                sources
                    .into_iter()
                    .map(|source| match source {
                        crate::attachments::Source::Retained { metadata } => Ok(metadata),
                        // Stored contributions contain normalized metadata, never
                        // provider paths or raw bytes to resolve during export.
                        _ => Err(invalid()),
                    })
                    .collect()
            }
        }
    }
}
