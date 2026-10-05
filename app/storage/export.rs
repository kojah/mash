//! Copy silver through a single SQLite read snapshot, without copying gold pages.
use super::{Storage, schema::SCHEMA_VERSION};
use crate::AppError;
use std::{collections::BTreeMap, path::Path};

pub(crate) const fn schema_version() -> i64 {
    SCHEMA_VERSION
}

pub(crate) fn copy(source: &Path, destination: &Path) -> Result<(), AppError> {
    // Preflight is read-only; export never initializes/migrates the source.
    let _source = Storage::open_catalog(source)?;
    let source = std::fs::canonicalize(source).map_err(AppError::io)?;
    let mut uri = reqwest::Url::from_file_path(source)
        .map_err(|()| AppError::usage("invalid export source path"))?;
    uri.query_pairs_mut().append_pair("mode", "ro");
    let target = Storage::open(destination)?;
    target
        .connection
        .execute("ATTACH DATABASE ?1 AS history", [uri.as_str()])
        .map_err(AppError::database)?;
    let transaction = target
        .connection
        .unchecked_transaction()
        .map_err(AppError::database)?;
    let version: i64 = transaction
        .query_row("PRAGMA history.user_version", [], |row| row.get(0))
        .map_err(AppError::database)?;
    if version != SCHEMA_VERSION {
        return Err(AppError::schema(
            "export source schema changed; retry with a compatible Mash build",
        ));
    }
    transaction
        .execute_batch(include_str!("export.sql"))
        .map_err(AppError::database)?;
    transaction.commit().map_err(AppError::database)?;
    target
        .connection
        .execute_batch(
            "DETACH DATABASE history; PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;",
        )
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
    let mut statement = storage.connection.prepare(
        "SELECT attachments FROM messages UNION ALL SELECT attachments FROM claude_source_messages"
    ).map_err(AppError::database)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(AppError::database)?;
    for row in rows {
        let records: Vec<crate::attachments::Metadata> =
            serde_json::from_str(&row.map_err(AppError::database)?)
                .map_err(|_| AppError::database_data("invalid export attachment metadata"))?;
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
    Ok(files)
}
