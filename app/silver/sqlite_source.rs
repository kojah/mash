//! Read-only snapshots of upstream application databases.
use crate::AppError;
use rusqlite::{Connection, OpenFlags};
use std::{path::Path, time::Duration};

pub(super) const MAX_RECORD_BYTES: i64 = 8 * 1024 * 1024;

pub(super) fn open(path: &Path) -> Result<Connection, AppError> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(AppError::database)?;
    snapshot(connection)
}

/// Opens a database without locks or WAL lookup. Only for a file whose
/// `-wal` and `-journal` are absent, whose committed pages are all in the main
/// file; the caller must verify the file's stamp after reading.
pub(super) fn open_immutable(path: &Path) -> Result<Connection, AppError> {
    let text = path
        .to_str()
        .ok_or_else(|| AppError::configuration("database path is not UTF-8"))?;
    let mut uri = String::from("file:");
    if cfg!(windows) && !text.starts_with(['/', '\\']) && path.is_absolute() {
        uri.push('/');
    }
    for character in text.chars() {
        match character {
            '%' => uri.push_str("%25"),
            '?' => uri.push_str("%3f"),
            '#' => uri.push_str("%23"),
            '\\' if cfg!(windows) => uri.push('/'),
            other => uri.push(other),
        }
    }
    uri.push_str("?immutable=1");
    let connection = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(AppError::database)?;
    snapshot(connection)
}

fn snapshot(connection: Connection) -> Result<Connection, AppError> {
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
    bounded_json_with_limit(row, super::MAX_RECORD_BYTES)
}

pub(super) fn bounded_json_with_limit<T: serde::de::DeserializeOwned>(
    row: &rusqlite::Row<'_>,
    limit: usize,
) -> Result<T, AppError> {
    let bytes: i64 = row.get("bytes").map_err(AppError::database)?;
    if !usize::try_from(bytes).is_ok_and(|bytes| bytes <= limit) {
        return Err(AppError::configuration(format!(
            "provider record exceeds the {} MiB ingestion limit",
            limit / (1024 * 1024)
        )));
    }
    let json: String = row.get("data").map_err(AppError::database)?;
    super::records::decode_with_limit(json.as_bytes(), limit)
}
