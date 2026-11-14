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

/// A typed, declarative query over canonical agent history. Every value is bound as a
/// parameter and every identifier comes from an enum, so callers cannot name a
/// table, column or SQL fragment. This is the shape a remote shared-silver
/// query can accept without parsing arbitrary SQL.
#[derive(Clone, Debug, schemars::JsonSchema, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
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

#[derive(Clone, Copy, Debug, schemars::JsonSchema, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Messages,
    Chats,
    Turns,
}

#[derive(Clone, Copy, Debug, schemars::JsonSchema, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupBy {
    Provider,
    Role,
    Chat,
}

#[derive(Clone, Copy, Debug, schemars::JsonSchema, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    CreatedAt,
    UpdatedAt,
    Id,
}

#[derive(Clone, Debug, Default, schemars::JsonSchema, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Filter {
    pub provider: Option<crate::provider::Provider>,
    pub chat: Option<String>,
    pub role: Option<crate::role::Role>,
    /// Inclusive lower bound in Unix milliseconds.
    pub since: Option<i64>,
    /// Exclusive upper bound in Unix milliseconds.
    pub until: Option<i64>,
    /// Case-insensitive substring over the target's text column.
    pub text: Option<String>,
}

impl Target {
    fn relation(self) -> &'static str {
        match self {
            Self::Messages => {
                "(SELECT m.*,s.provider FROM messages m JOIN chats s ON s.id=m.chat_id)"
            }
            Self::Chats => "chats",
            Self::Turns => "(SELECT t.*,s.provider FROM turns t JOIN chats s ON s.id=t.chat_id)",
        }
    }

    fn columns(self) -> &'static str {
        match self {
            Self::Messages => "id, chat_id, turn_id, provider, role, ordinal, created_at, content",
            Self::Chats => "id, provider, title, project, repository, created_at, updated_at",
            Self::Turns => {
                "id, chat_id, provider, revision, boundary, closed, created_at, updated_at"
            }
        }
    }

    fn text_column(self) -> Result<&'static str, AppError> {
        match self {
            Self::Messages => Ok("content"),
            Self::Chats => Ok("title"),
            Self::Turns => Err(AppError::usage(
                "turns have no text column; query their messages",
            )),
        }
    }

    fn group_column(self, group: GroupBy) -> Result<&'static str, AppError> {
        match (self, group) {
            (_, GroupBy::Provider) => Ok("provider"),
            (Self::Messages, GroupBy::Role) => Ok("role"),
            (Self::Messages | Self::Turns, GroupBy::Chat) => Ok("chat_id"),
            _ => Err(AppError::usage("this target cannot group by role or chat")),
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
    if let Some(provider) = &request.filter.provider {
        where_clauses.push("provider = ?".to_owned());
        params.push(Value::Text(provider.as_str().to_owned()));
    }
    if let Some(chat) = &request.filter.chat {
        let column = if matches!(request.target, Target::Chats) {
            "id"
        } else {
            "chat_id"
        };
        where_clauses.push(format!("{column} = ?"));
        params.push(Value::Text(chat.clone()));
    }
    if let Some(role) = &request.filter.role {
        if !matches!(request.target, Target::Messages) {
            return Err(AppError::usage("role filters apply only to messages"));
        }
        where_clauses.push("role = ?".to_owned());
        params.push(Value::Text(role.as_str().to_owned()));
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
            request.target.text_column()?
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
    if matches!(
        (request.target, request.order),
        (Target::Messages, Some(Order::UpdatedAt))
    ) {
        return Err(AppError::usage("messages have created_at, not updated_at"));
    }
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
        request.target.relation()
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

fn sql_error(error: &rusqlite::Error) -> AppError {
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
    prepare_read_only(&connection, timeout)?;
    execute(&connection, sql, params, limit, byte_budget)
}

/// Pin the connection read-only and arm the time
/// budget. Shared-silver reads call this on a server connection as well, so a
/// typed query gets exactly the same guardrails as a local one.
fn prepare_read_only(connection: &Connection, timeout: Duration) -> Result<(), AppError> {
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(AppError::database)?;
    if version != crate::storage::schema::SCHEMA_VERSION {
        return Err(crate::storage::schema::unsupported(version));
    }
    crate::storage::packed::register(connection).map_err(AppError::database)?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(AppError::database)?;
    connection
        .pragma_update(None, "query_only", true)
        .map_err(AppError::database)?;
    // Install the authorizer after configuring the read-only connection.
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
    Ok(())
}

/// Run a typed request on an already-open connection, applying the same
/// read-only setup as `run_request`.
pub(crate) fn run_request_on(
    connection: &Connection,
    request: &Request,
    timeout: Duration,
) -> Result<Value, AppError> {
    let (sql, params) = request_sql(request)?;
    prepare_read_only(connection, timeout)?;
    execute(
        connection,
        &sql,
        &params,
        request.limit.clamp(1, MAX_ROWS),
        MAX_OUTPUT_BYTES,
    )
}

fn execute(
    connection: &Connection,
    sql: &str,
    params: &[rusqlite::types::Value],
    limit: usize,
    byte_budget: usize,
) -> Result<Value, AppError> {
    let mut statement = connection.prepare(sql).map_err(|error| sql_error(&error))?;
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
        crate::storage::packed::register(&connection).unwrap();
        connection
            .execute_batch(crate::storage::schema::SILVER)
            .unwrap();
        connection
            .execute_batch(
                "CREATE TABLE t (id INTEGER, name TEXT);
             INSERT INTO t VALUES (1, 'a'), (2, 'b'), (3, 'c');
             INSERT INTO chats(id,provider,title) VALUES('chat-1','codex','A chat');
             INSERT INTO turns(id,chat_id,revision,boundary,closed)
                VALUES('turn-1','chat-1','revision','start',0);
             INSERT INTO messages(id,chat_id,turn_id,ordinal,role,content_packed)
                VALUES('message-1','chat-1','turn-1',0,'user',silver_pack('hello'));",
            )
            .unwrap();
        connection
            .pragma_update(None, "user_version", crate::storage::schema::SCHEMA_VERSION)
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
    fn canonical_tables_expose_chats_messages_and_turns() {
        let (_directory, path) = fixture();
        let chat = run(
            &path,
            "SELECT id, title FROM chats",
            10,
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(chat["rows"], json!([{"id": "chat-1", "title": "A chat"}]));

        let message = run(
            &path,
            "SELECT id, chat_id, role, content FROM messages",
            10,
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(
            message["rows"],
            json!([{
                "id": "message-1",
                "chat_id": "chat-1",
                "role": "user",
                "content": "hello",
            }])
        );

        let turn = run(
            &path,
            "SELECT id, boundary FROM turns",
            10,
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(turn["rows"], json!([{"id": "turn-1", "boundary": "start"}]));
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
            r#"{"target":"messages","filter":{"provider":"codex","text":"ELL"}}"#,
        )
        .unwrap();
        let value = run_request(&path, &request, Duration::from_secs(5)).unwrap();
        assert_eq!(value["rows"].as_array().unwrap().len(), 1);

        let grouped: Request =
            serde_json::from_str(r#"{"target":"messages","group_by":"provider"}"#).unwrap();
        let value = run_request(&path, &grouped, Duration::from_secs(5)).unwrap();
        assert_eq!(value["rows"], json!([{"group_key": "codex", "count": 1}]));

        let turns: Request =
            serde_json::from_str(r#"{"target":"turns","group_by":"provider"}"#).unwrap();
        let value = run_request(&path, &turns, Duration::from_secs(5)).unwrap();
        assert_eq!(value["rows"], json!([{"group_key": "codex", "count": 1}]));

        // Zero is clamped to one row rather than returning nothing.
        let clamped: Request = serde_json::from_str(r#"{"target":"messages","limit":0}"#).unwrap();
        let value = run_request(&path, &clamped, Duration::from_secs(5)).unwrap();
        assert_eq!(value["rows"].as_array().unwrap().len(), 1);

        // Unknown fields, including a raw SQL escape, are rejected by serde.
        assert!(
            serde_json::from_str::<Request>(r#"{"target":"messages","sql":"SELECT 1"}"#).is_err()
        );
        // Role is not a chat column.
        let invalid: Request =
            serde_json::from_str(r#"{"target":"chats","group_by":"role"}"#).unwrap();
        assert!(run_request(&path, &invalid, Duration::from_secs(5)).is_err());
    }

    #[test]
    fn empty_and_oversized_statements_are_usage_errors() {
        let (_directory, path) = fixture();
        assert!(run(&path, "  ", 10, Duration::from_secs(5)).is_err());
        let huge = "SELECT 1;".repeat(MAX_SQL_BYTES);
        assert!(run(&path, &huge, 10, Duration::from_secs(5)).is_err());
    }

    #[test]
    fn queries_reject_foreign_schemas_without_modifying_them() {
        let (_directory, path) = fixture();
        for version in [0, crate::storage::schema::SCHEMA_VERSION + 1] {
            let connection = Connection::open(&path).unwrap();
            connection
                .pragma_update(None, "user_version", version)
                .unwrap();
            assert!(run(&path, "SELECT 1", 10, Duration::from_secs(5)).is_err());
            let retained: i64 = connection
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .unwrap();
            assert_eq!(retained, version);
            let count: i64 = connection
                .query_row("SELECT count(*) FROM messages", [], |row| row.get(0))
                .unwrap();
            assert_eq!(count, 1);
        }
    }

    #[test]
    fn agent_query_targets_validate_filters_and_reject_retired_documents() {
        let (_directory, path) = fixture();
        for target in ["chats", "messages", "turns"] {
            let request: Request = serde_json::from_value(json!({
                "target": target, "filter": {"chat": "chat-1", "provider": "codex"}
            }))
            .unwrap();
            let result = run_request(&path, &request, Duration::from_secs(5)).unwrap();
            assert_eq!(result["rows"].as_array().unwrap().len(), 1, "{target}");
        }
        for request in [
            json!({"target":"chats","filter":{"role":"user"}}),
            json!({"target":"turns","filter":{"text":"hello"}}),
            json!({"target":"messages","order":"updated_at"}),
        ] {
            assert!(
                run_request(
                    &path,
                    &serde_json::from_value(request).unwrap(),
                    Duration::from_secs(5)
                )
                .is_err()
            );
        }
        assert!(serde_json::from_value::<Request>(json!({"target":"documents"})).is_err());
        assert!(
            serde_json::from_value::<Request>(
                json!({"target":"messages","filter":{"source":"external"}})
            )
            .is_err()
        );
    }
}
