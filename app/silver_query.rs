//! Read-only SQL over a silver database, for diagnostics and MCP queries.
//!
//! The connection is opened read-only and pinned with `query_only`, so a
//! statement cannot write even by accident. A progress handler enforces a wall
//! clock budget and the caller bounds the number of returned rows.

use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};

use crate::AppError;

const MAX_ROWS: usize = 10_000;
const MAX_SQL_BYTES: usize = 64 * 1024;

fn json_value(value: rusqlite::types::ValueRef<'_>) -> Value {
    use rusqlite::types::ValueRef;
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(number) => json!(number),
        ValueRef::Real(number) => json!(number),
        ValueRef::Text(bytes) => json!(String::from_utf8_lossy(bytes)),
        // Bytes have no JSON type; hex keeps them comparable and reversible.
        ValueRef::Blob(bytes) => json!(format!("x'{}'", hex(bytes))),
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Run one read-only statement. Returns its columns, up to `limit` rows, and
/// whether more rows were available.
pub(crate) fn run(
    database: &Path,
    sql: &str,
    limit: usize,
    timeout: Duration,
) -> Result<Value, AppError> {
    let sql = sql.trim();
    if sql.is_empty() {
        return Err(AppError::usage("query is empty"));
    }
    if sql.len() > MAX_SQL_BYTES {
        return Err(AppError::usage("query exceeds the statement limit"));
    }
    let limit = limit.clamp(1, MAX_ROWS);
    let connection = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(AppError::database)?;
    connection
        .pragma_update(None, "query_only", true)
        .map_err(AppError::database)?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(AppError::database)?;
    // Abort a runaway scan when the budget expires; the handle is inert once
    // the statement has finished.
    let interrupt = connection.get_interrupt_handle();
    std::thread::spawn(move || {
        std::thread::sleep(timeout);
        interrupt.interrupt();
    });

    let mut statement = connection
        .prepare(sql)
        .map_err(|error| AppError::usage(format!("query is not valid SQL: {error}")))?;
    let columns: Vec<String> = statement
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut rows = statement
        .query([])
        .map_err(|error| AppError::usage(format!("query failed: {error}")))?;
    let mut output = Vec::new();
    let mut truncated = false;
    loop {
        let row = rows.next().map_err(|error| {
            if matches!(error, rusqlite::Error::SqliteFailure(failure, _) if failure.code == rusqlite::ErrorCode::OperationInterrupted)
            {
                AppError::usage("query exceeded its time budget")
            } else {
                AppError::database(error)
            }
        })?;
        let Some(row) = row else { break };
        if output.len() >= limit {
            truncated = true;
            break;
        }
        let mut object = serde_json::Map::with_capacity(columns.len());
        for (index, name) in columns.iter().enumerate() {
            let value = row.get_ref(index).map_err(AppError::database)?;
            object.insert(name.clone(), json_value(value));
        }
        output.push(Value::Object(object));
    }
    Ok(json!({
        "columns": columns,
        "rows": output,
        "truncated": truncated,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("silver.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE t (id INTEGER, name TEXT);
                 INSERT INTO t VALUES (1, 'a'), (2, 'b'), (3, 'c');",
            )
            .unwrap();
        (directory, path)
    }

    #[test]
    fn returns_columns_rows_and_truncation() {
        let (_directory, path) = fixture();
        let value = run(
            &path,
            "SELECT * FROM t ORDER BY id",
            2,
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(value["columns"], json!(["id", "name"]));
        assert_eq!(value["rows"].as_array().unwrap().len(), 2);
        assert_eq!(value["truncated"], true);
    }

    #[test]
    fn writes_are_rejected_by_the_read_only_connection() {
        let (_directory, path) = fixture();
        assert!(run(&path, "DELETE FROM t", 10, Duration::from_secs(5)).is_err());
        let connection = Connection::open(&path).unwrap();
        let remaining: i64 = connection
            .query_row("SELECT COUNT(*) FROM t", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, 3, "the rejected write changed nothing");
    }

    #[test]
    fn empty_and_oversized_statements_are_usage_errors() {
        let (_directory, path) = fixture();
        assert!(run(&path, "  ", 10, Duration::from_secs(5)).is_err());
        let huge = "SELECT 1;".repeat(MAX_SQL_BYTES);
        assert!(run(&path, &huge, 10, Duration::from_secs(5)).is_err());
    }
}
