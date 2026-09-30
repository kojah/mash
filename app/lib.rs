mod cli;
mod config;
mod ingestion;
mod mcp;
mod search_filter;
mod semantic;
mod shared;
mod storage;
mod watch;
#[cfg(unix)]
mod worker;

use std::ffi::OsString;
use std::io::{self, Write};

use serde::{Deserialize, Serialize};

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| AppError::internal("blocking worker failed"))?
}

/// Runs mash with the process arguments, writing JSON or serving stdio MCP.
///
/// # Examples
///
/// ```no_run
/// let exit_code = mash::main_entry();
/// std::process::exit(exit_code);
/// ```
#[must_use]
pub fn main_entry() -> i32 {
    #[cfg(unix)]
    {
        let arguments = std::env::args_os().collect::<Vec<_>>();
        if arguments.len() == 3 && arguments[1] == "--internal-search-worker" {
            return crate::worker::run(std::path::Path::new(&arguments[2]))
                .map_or_else(|error| error.code, |()| 0);
        }
    }
    execute(std::env::args_os(), &mut io::stdout(), &mut io::stderr())
}

fn execute<I, T>(args: I, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    match cli::run(args) {
        // MCP owns stdout for its entire session, including shutdown.
        Ok(cli::Response::McpStopped) => 0,
        Ok(response) => write_json(stdout, &response).map_or_else(
            |error| {
                let _ = write_json(stderr, &error);
                error.code
            },
            |()| 0,
        ),
        Err(error) => {
            let _ = write_json(stderr, &error);
            error.code
        }
    }
}

fn write_json<T: Serialize>(writer: &mut dyn Write, value: &T) -> Result<(), AppError> {
    serde_json::to_writer(&mut *writer, value)
        .map_err(|error| AppError::internal(format!("failed to serialize response: {error}")))?;
    writeln!(writer)
        .map_err(|error| AppError::internal(format!("failed to write response: {error}")))
}

#[derive(Debug, Deserialize, Serialize)]
struct AppError {
    error: ErrorBody,
    #[serde(skip)]
    code: i32,
}

#[derive(Debug, Deserialize, Serialize)]
struct ErrorBody {
    kind: String,
    message: String,
    retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    recommended_action: Option<String>,
}

impl AppError {
    fn shared(message: impl Into<String>) -> Self {
        Self::new(11, "shared-index", message, true)
    }

    fn configuration(message: impl Into<String>) -> Self {
        Self::new(9, "configuration", message, false)
    }

    fn usage(message: impl Into<String>) -> Self {
        Self::new(2, "usage", message, false)
    }

    fn missing_database(path: &std::path::Path) -> Self {
        Self::with_action(
            3,
            "database-missing",
            format!("database does not exist: {}", path.display()),
            true,
            "index",
        )
    }

    fn database(error: rusqlite::Error) -> Self {
        if matches!(
            &error,
            rusqlite::Error::SqliteFailure(details, _)
                if matches!(
                    details.code,
                    rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                )
        ) {
            return Self::new(6, "index-busy", "another index writer is active", true);
        }
        let message = error.to_string();
        drop(error);
        Self::new(5, "database", message, true)
    }

    fn database_data(message: impl Into<String>) -> Self {
        Self::new(5, "database", message, false)
    }

    fn io(error: io::Error) -> Self {
        let message = error.to_string();
        drop(error);
        Self::new(5, "io", message, true)
    }

    fn internal(message: impl Into<String>) -> Self {
        Self::new(10, "internal", message, false)
    }

    fn model(message: impl Into<String>) -> Self {
        Self::with_action(4, "model", message, true, "models install")
    }

    fn search_not_ready(message: impl Into<String>) -> Self {
        Self::with_action(8, "search-not-ready", message, true, "index")
    }

    fn schema(message: impl Into<String>) -> Self {
        Self::new(7, "schema-incompatible", message, false)
    }

    fn new(code: i32, kind: &'static str, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            error: ErrorBody {
                kind: kind.to_owned(),
                message: message.into(),
                retryable,
                recommended_action: None,
            },
            code,
        }
    }

    fn with_action(
        code: i32,
        kind: &'static str,
        message: impl Into<String>,
        retryable: bool,
        recommended_action: &'static str,
    ) -> Self {
        let mut error = Self::new(code, kind, message, retryable);
        error.error.recommended_action = Some(recommended_action.to_owned());
        error
    }
}
