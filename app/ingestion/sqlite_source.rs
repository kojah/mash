//! Read-only snapshots of upstream application databases.
use crate::AppError;
use rusqlite::{Connection, OpenFlags};
use std::{path::Path, time::Duration};

pub(super) const MAX_RECORD_BYTES: i64 = 8 * 1024 * 1024;

pub(super) fn open(path: &Path) -> Result<Connection, AppError> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(AppError::database)?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(AppError::database)?;
    connection
        .execute_batch("PRAGMA query_only=ON; BEGIN;")
        .map_err(AppError::database)?;
    Ok(connection)
}

pub(super) fn bounded_json<T: serde::de::DeserializeOwned>(
    row: &rusqlite::Row<'_>,
) -> Result<T, AppError> {
    let bytes: i64 = row.get("bytes").map_err(AppError::database)?;
    if bytes > MAX_RECORD_BYTES {
        return Err(AppError::configuration(
            "provider record exceeds the 8 MiB ingestion limit",
        ));
    }
    let json: String = row.get("data").map_err(AppError::database)?;
    serde_json::from_str(&json)
        .map_err(|_| AppError::configuration("invalid structured provider history"))
}
