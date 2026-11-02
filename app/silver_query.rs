//! Read-only SQL over a silver database, for diagnostics and MCP queries.
//!
//! The connection is opened read-only and pinned with `query_only`, so a
//! statement cannot write even by accident. A progress handler enforces a wall
//! clock budget and the caller bounds the number of returned rows.

use std::path::Path;
use std::time::Duration;

use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::{Connection, ErrorCode, OpenFlags};
use serde_json::{Value, json};

use crate::AppError;

const MAX_ROWS: usize = 10_000;
const MAX_SQL_BYTES: usize = 64 * 1024;
/// Rows are capped by count, but one row can still hold a huge blob or text.
/// Bound the approximate decoded payload as well so a single statement cannot
/// exhaust memory through the row limit alone.
const MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
const ROW_OVERHEAD_BYTES: usize = 8;

/// A typed, declarative query over the stable views. Every value is bound as a
/// parameter and every identifier comes from an enum, so callers cannot name a
/// table, column or SQL fragment. This is the shape a remote shared-silver
/// query can accept without parsing arbitrary SQL.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    pub target: Target,
    #[serde(default)]
    pub filter: Filter,
    #[serde(default)]
    pub group_by: Option<GroupBy>,
    #[serde(default)]
    pub order: Option<Order>,
    #[serde(default)]
    pub descending: bool,
    #[serde(default = "default_request_limit")]
    pub limit: usize,
}

fn default_request_limit() -> usize {
    200
}

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Target {
    Messages,
    Conversations,
    Documents,
}

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GroupBy {
    Source,
    Role,
    Conversation,
}

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Order {
    CreatedAt,
    UpdatedAt,
    Id,
}

#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct Filter {
    pub source: Option<String>,
    pub conversation: Option<String>,
    pub role: Option<String>,
    /// Inclusive lower bound in Unix milliseconds.
    pub since: Option<i64>,
    /// Exclusive upper bound in Unix milliseconds.
    pub until: Option<i64>,
    /// Case-insensitive substring over the target's text column.
    pub text: Option<String>,
}

impl Target {
    fn view(self) -> &'static str {
        match self {
            Self::Messages => "messages",
            Self::Conversations => "conversations",
            Self::Documents => "documents",
        }
    }

    fn columns(self) -> &'static str {
        match self {
            Self::Messages => {
                "id, conversation_id, source, role, ordinal, created_at, updated_at, content"
            }
            Self::Conversations => {
                "id, source, kind, title, project, repository, created_at, updated_at, metadata"
            }
            Self::Documents => {
                "id, source, kind, title, content, created_at, updated_at, parent_id, url"
            }
        }
    }

    fn text_column(self) -> &'static str {
        match self {
            Self::Messages | Self::Documents => "content",
            Self::Conversations => "title",
        }
    }

    fn group_column(self, group: GroupBy) -> Result<&'static str, AppError> {
        match (self, group) {
            (_, GroupBy::Source) => Ok("source"),
            (Self::Messages, GroupBy::Role) => Ok("role"),
            (Self::Messages, GroupBy::Conversation) => Ok("conversation_id"),
            _ => Err(AppError::usage(
                "this target cannot group by role or conversation",
            )),
        }
    }
}

impl Order {
    fn column(self) -> &'static str {
        match self {
            Self::CreatedAt => "created_at",
            Self::UpdatedAt => "updated_at",
            Self::Id => "id",
        }
    }
}

/// Build a single parameterized statement from the request.
fn request_sql(request: &Request) -> Result<(String, Vec<rusqlite::types::Value>), AppError> {
    use rusqlite::types::Value;
    let mut where_clauses: Vec<String> = Vec::new();
    let mut params = Vec::new();
    if let Some(source) = &request.filter.source {
        where_clauses.push("source = ?".to_owned());
        params.push(Value::Text(source.clone()));
    }
    if let Some(conversation) = &request.filter.conversation {
        where_clauses.push("conversation_id = ?".to_owned());
        params.push(Value::Text(conversation.clone()));
    }
    if let Some(role) = &request.filter.role {
        where_clauses.push("role = ?".to_owned());
        params.push(Value::Text(role.clone()));
    }
    if let Some(since) = request.filter.since {
        where_clauses.push("created_at >= ?".to_owned());
        params.push(Value::Integer(since));
    }
    if let Some(until) = request.filter.until {
        where_clauses.push("created_at < ?".to_owned());
        params.push(Value::Integer(until));
    }
    if let Some(text) = &request.filter.text {
        where_clauses.push(format!(
            "instr(lower(coalesce({}, '')), lower(?)) > 0",
            request.target.text_column()
        ));
        params.push(Value::Text(text.clone()));
    }
    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", where_clauses.join(" AND "))
    };
    let (select, group_sql, default_order) = match request.group_by {
        Some(group) => {
            let column = request.target.group_column(group)?;
            (
                format!("{column} AS group_key, COUNT(*) AS count"),
                format!(" GROUP BY {column}"),
                "count",
            )
        }
        None => (request.target.columns().to_owned(), String::new(), "id"),
    };
    let order = request.order.map_or(default_order, |order| order.column());
    let direction = if request.descending || (request.group_by.is_some() && request.order.is_none())
    {
        "DESC"
    } else {
        "ASC"
    };
    let limit = request.limit.clamp(1, MAX_ROWS);
    let sql = format!(
        "SELECT {select} FROM {}{where_sql}{group_sql} ORDER BY {order} {direction} LIMIT {limit}",
        request.target.view()
    );
    Ok((sql, params))
}

/// Run a typed request with the same read-only connection and output bounds as
/// `run`.
pub(crate) fn run_request(
    database: &Path,
    request: &Request,
    timeout: Duration,
) -> Result<Value, AppError> {
    let (sql, params) = request_sql(request)?;
    run_params(
        database,
        &sql,
        &params,
        request.limit,
        timeout,
        MAX_OUTPUT_BYTES,
    )
}

/// Allow only actions a read-only query needs. `query_only` already blocks
/// data changes, but it does not block ATTACH or every pragma: the authorizer
/// closes that gap and makes the documented contract enforceable.
fn authorize(context: AuthContext<'_>) -> Authorization {
    match context.action {
        AuthAction::Select
        | AuthAction::Read { .. }
        | AuthAction::Function { .. }
        | AuthAction::Recursive => Authorization::Allow,
        _ => Authorization::Deny,
    }
}

fn sql_error(error: rusqlite::Error) -> AppError {
    if matches!(
        error,
        rusqlite::Error::SqliteFailure(failure, _)
            if failure.code == ErrorCode::AuthorizationForStatementDenied
    ) {
        AppError::usage("query may only read; ATTACH and pragma are not allowed")
    } else {
        AppError::usage(format!("query is not valid SQL: {error}"))
    }
}

/// Convenience views over the generic `objects` table, installed on every
/// read-only connection. They are the stable names diagnostics and MCP callers
/// can rely on while the underlying columns stay free to change.
const STABLE_VIEWS: &str = "
CREATE TEMP VIEW IF NOT EXISTS conversations AS
SELECT origin, source, id, kind, title, project, repository,
       created_at, updated_at, metadata
FROM objects
WHERE object_role = 'container';

CREATE TEMP VIEW IF NOT EXISTS messages AS
SELECT source, conversation_id, id, kind, object_role, ordinal, role, author,
       content, search_projection, created_at, updated_at, reply_to,
       attachments, metadata
FROM objects
WHERE object_role IN ('entry', 'segment');

CREATE TEMP VIEW IF NOT EXISTS documents AS
SELECT source, id, kind, title, content, created_at, updated_at,
       parent_id, url, metadata
FROM objects
WHERE object_role = 'document';
";

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

fn value_bytes(value: rusqlite::types::ValueRef<'_>) -> usize {
    use rusqlite::types::ValueRef;
    match value {
        ValueRef::Null => 4,
        ValueRef::Integer(_) | ValueRef::Real(_) => 8,
        ValueRef::Text(bytes) => bytes.len(),
        // Blobs are returned as hex, so the decoded payload is twice the size.
        ValueRef::Blob(bytes) => bytes.len() * 2,
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
    run_bounded(database, sql, limit, timeout, MAX_OUTPUT_BYTES)
}

/// `run` with an explicit decoded-payload budget, so the bound is testable.
pub(crate) fn run_bounded(
    database: &Path,
    sql: &str,
    limit: usize,
    timeout: Duration,
    byte_budget: usize,
) -> Result<Value, AppError> {
    run_params(database, sql, &[], limit, timeout, byte_budget)
}

fn run_params(
    database: &Path,
    sql: &str,
    params: &[rusqlite::types::Value],
    limit: usize,
    timeout: Duration,
    byte_budget: usize,
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
        .busy_timeout(Duration::from_secs(5))
        .map_err(AppError::database)?;
    // Views are installed by us before the connection is pinned read-only, so
    // a statement can use them but can neither create nor alter one.
    connection
        .execute_batch(STABLE_VIEWS)
        .map_err(AppError::database)?;
    connection
        .pragma_update(None, "query_only", true)
        .map_err(AppError::database)?;
    // The authorizer runs after our own setup, so the stable views and
    // query_only are installed while it is still absent.
    connection
        .authorizer(Some(authorize))
        .map_err(AppError::database)?;
    // Abort a runaway scan when the budget expires; the handle is inert once
    // the statement has finished.
    let interrupt = connection.get_interrupt_handle();
    std::thread::spawn(move || {
        std::thread::sleep(timeout);
        interrupt.interrupt();
    });

    let mut statement = connection.prepare(sql).map_err(sql_error)?;
    let columns: Vec<String> = statement
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut rows = statement
        .query(rusqlite::params_from_iter(params.iter()))
        .map_err(|error| {
            if matches!(
                error,
                rusqlite::Error::SqliteFailure(failure, _)
                    if failure.code == ErrorCode::AuthorizationForStatementDenied
            ) {
                AppError::usage("query may only read; ATTACH and pragma are not allowed")
            } else {
                AppError::usage(format!("query failed: {error}"))
            }
        })?;
    let mut output = Vec::new();
    let mut truncated = false;
    let mut bytes = 0;
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
        let mut row_bytes = 0;
        for (index, name) in columns.iter().enumerate() {
            let value = row.get_ref(index).map_err(AppError::database)?;
            row_bytes += value_bytes(value) + ROW_OVERHEAD_BYTES + name.len();
            object.insert(name.clone(), json_value(value));
        }
        if bytes + row_bytes > byte_budget {
            truncated = true;
            break;
        }
        bytes += row_bytes;
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
                 INSERT INTO t VALUES (1, 'a'), (2, 'b'), (3, 'c');
                 CREATE TABLE objects (
                   storage_id INTEGER PRIMARY KEY,
                   origin TEXT, source TEXT, id TEXT, object_role TEXT,
                   kind TEXT, parent_id TEXT, parent_storage_id INTEGER,
                   reply_to TEXT, conversation_id TEXT, ordinal INTEGER,
                   context_parent INTEGER, role TEXT, author TEXT, title TEXT,
                   content TEXT, search_projection TEXT, created_at INTEGER,
                   updated_at INTEGER, project TEXT, repository TEXT, url TEXT,
                   metadata TEXT, attachments TEXT DEFAULT '[]', record TEXT);
                 INSERT INTO objects (origin, source, id, object_role, kind,
                   conversation_id, title)
                   VALUES ('provider', 'codex', 'chat-1', 'container',
                           'codex:thread', 'chat-1', 'A chat');
                 INSERT INTO objects (origin, source, id, object_role, kind,
                   conversation_id, ordinal, role, content)
                   VALUES ('provider', 'codex', 'message-1', 'entry',
                           'codex:message', 'chat-1', 0, 'user', 'hello');
                 INSERT INTO objects (origin, source, id, object_role, kind,
                   conversation_id, title, content)
                   VALUES ('connector', 'git:acme/app', 'commit-1', 'document',
                           'git:commit', 'commit-1', 'fix', 'body');",
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
    fn stable_views_expose_containers_messages_and_documents() {
        let (_directory, path) = fixture();
        let chat = run(
            &path,
            "SELECT id, title FROM conversations",
            10,
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(chat["rows"], json!([{"id": "chat-1", "title": "A chat"}]));

        let message = run(
            &path,
            "SELECT id, conversation_id, role, content FROM messages",
            10,
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(
            message["rows"],
            json!([{
                "id": "message-1",
                "conversation_id": "chat-1",
                "role": "user",
                "content": "hello",
            }])
        );

        let document = run(
            &path,
            "SELECT id, kind, content FROM documents",
            10,
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(
            document["rows"],
            json!([{"id": "commit-1", "kind": "git:commit", "content": "body"}])
        );
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
    fn attachments_and_pragmas_are_denied() {
        let (_directory, path) = fixture();
        let attach = run(
            &path,
            "ATTACH DATABASE '/tmp/mash-silver-query-attach.db' AS probe",
            10,
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert!(attach.error.message.contains("may only read"), "{attach:?}");
        let pragma = run(
            &path,
            "PRAGMA writable_schema=ON",
            10,
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert!(pragma.error.message.contains("may only read"), "{pragma:?}");
    }

    #[test]
    fn read_only_diagnostics_and_ctes_are_allowed() {
        let (_directory, path) = fixture();
        for sql in [
            "EXPLAIN QUERY PLAN SELECT * FROM t",
            "WITH x AS (SELECT 1 AS n) SELECT n FROM x",
            "WITH RECURSIVE c(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM c WHERE n < 3) SELECT n FROM c",
            "SELECT lower(name) FROM t",
        ] {
            assert!(
                run(&path, sql, 10, Duration::from_secs(5)).is_ok(),
                "denied: {sql}"
            );
        }
    }

    #[test]
    fn the_decoded_payload_budget_truncates_wide_rows() {
        let (_directory, path) = fixture();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE big (id INTEGER, body TEXT);
                 INSERT INTO big VALUES (1, printf('%.1000c', 'a'));
                 INSERT INTO big VALUES (2, printf('%.1000c', 'a'));",
            )
            .unwrap();
        // One 1000-byte body plus overhead fits; the second row does not.
        let value =
            run_bounded(&path, "SELECT * FROM big", 10, Duration::from_secs(5), 1500).unwrap();
        assert_eq!(value["rows"].as_array().unwrap().len(), 1);
        assert_eq!(value["truncated"], true);
    }

    #[test]
    fn typed_requests_filter_group_and_bound_without_sql() {
        let (_directory, path) = fixture();
        let request: Request = serde_json::from_str(
            r#"{"target":"messages","filter":{"source":"codex","text":"ELL"}}"#,
        )
        .unwrap();
        let value = run_request(&path, &request, Duration::from_secs(5)).unwrap();
        assert_eq!(value["rows"].as_array().unwrap().len(), 1);

        let grouped: Request =
            serde_json::from_str(r#"{"target":"messages","group_by":"source"}"#).unwrap();
        let value = run_request(&path, &grouped, Duration::from_secs(5)).unwrap();
        assert_eq!(value["rows"], json!([{"group_key": "codex", "count": 1}]));

        let documents: Request =
            serde_json::from_str(r#"{"target":"documents","group_by":"source"}"#).unwrap();
        let value = run_request(&path, &documents, Duration::from_secs(5)).unwrap();
        assert_eq!(
            value["rows"],
            json!([{"group_key": "git:acme/app", "count": 1}])
        );

        // Zero is clamped to one row rather than returning nothing.
        let clamped: Request = serde_json::from_str(r#"{"target":"messages","limit":0}"#).unwrap();
        let value = run_request(&path, &clamped, Duration::from_secs(5)).unwrap();
        assert_eq!(value["rows"].as_array().unwrap().len(), 1);

        // Unknown fields, including a raw SQL escape, are rejected by serde.
        assert!(
            serde_json::from_str::<Request>(r#"{"target":"messages","sql":"SELECT 1"}"#).is_err()
        );
        // Role is not a conversation column.
        let invalid: Request =
            serde_json::from_str(r#"{"target":"conversations","group_by":"role"}"#).unwrap();
        assert!(run_request(&path, &invalid, Duration::from_secs(5)).is_err());
    }

    #[test]
    fn empty_and_oversized_statements_are_usage_errors() {
        let (_directory, path) = fixture();
        assert!(run(&path, "  ", 10, Duration::from_secs(5)).is_err());
        let huge = "SELECT 1;".repeat(MAX_SQL_BYTES);
        assert!(run(&path, &huge, 10, Duration::from_secs(5)).is_err());
    }
}
