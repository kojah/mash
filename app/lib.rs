// Typed async search futures include transport tracing and concurrent retrieval.
#![recursion_limit = "256"]

// Silver canonical history and the vocabulary used by native adapters.
mod message_context;
mod provider;
mod role;
mod silver;

// Gold materialization, retrieval models, policies and query-oriented views.
mod gold;
mod search_filter;
mod search_query;
mod search_ranking;
mod search_results;
mod semantic;
mod thread_catalog;

// Application composition and infrastructure supporting both layers.
mod attachments;
mod cli;
mod config;
mod core;
mod mcp;
mod observability;
mod progress;
mod redaction;
mod scratch;
mod shared;
mod storage;
#[cfg(test)]
mod test_support;
mod watch;
#[cfg(unix)]
mod worker;

use std::ffi::OsString;
use std::io::{self, Write};

use serde::{Deserialize, Serialize};

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    let sink = progress::current_sink();
    let indexer_cache = semantic::indexer::current();
    let context = observability::ThreadContext::capture();
    tokio::task::spawn_blocking(move || {
        context.run(|| {
            semantic::indexer::on_blocking_thread(indexer_cache, || {
                progress::on_blocking_thread(sink, work)
            })
        })
    })
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
    observability::initialize();
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
            |()| match response {
                cli::Response::Operation(value) if value.needs_retry() => 9,
                _ => 0,
            },
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
    kind: ErrorKind,
    message: String,
    retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    recommended_action: Option<Action>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ErrorKind {
    SharedIndex,
    Configuration,
    Usage,
    DatabaseMissing,
    IndexBusy,
    ModelBusy,
    Database,
    Io,
    Internal,
    Model,
    SearchNotReady,
    SchemaIncompatible,
    RemoteInference,
    Conflict,
    SourceChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
enum Action {
    #[serde(rename = "index")]
    Index,
    #[serde(rename = "models install")]
    ModelsInstall,
}

impl AppError {
    fn source_changed() -> Self {
        Self::with_action(
            9,
            ErrorKind::SourceChanged,
            "history changed while indexing; retry index after the source settles",
            true,
            Action::Index,
        )
    }

    fn shared(message: impl Into<String>) -> Self {
        Self::new(11, ErrorKind::SharedIndex, message, true)
    }

    fn configuration(message: impl Into<String>) -> Self {
        Self::new(9, ErrorKind::Configuration, message, false)
    }

    fn usage(message: impl Into<String>) -> Self {
        Self::new(2, ErrorKind::Usage, message, false)
    }

    fn missing_database(path: &std::path::Path) -> Self {
        Self::with_action(
            3,
            ErrorKind::DatabaseMissing,
            format!("database does not exist: {}", path.display()),
            true,
            Action::Index,
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
            return Self::new(
                6,
                ErrorKind::IndexBusy,
                "another index writer is active",
                true,
            );
        }
        let message = error.to_string();
        drop(error);
        Self::new(5, ErrorKind::Database, message, true)
    }

    fn database_data(message: impl Into<String>) -> Self {
        Self::new(5, ErrorKind::Database, message, false)
    }

    fn io(error: io::Error) -> Self {
        let message = error.to_string();
        drop(error);
        Self::new(5, ErrorKind::Io, message, true)
    }

    fn internal(message: impl Into<String>) -> Self {
        Self::new(10, ErrorKind::Internal, message, false)
    }

    fn model(message: impl Into<String>) -> Self {
        Self::with_action(4, ErrorKind::Model, message, true, Action::ModelsInstall)
    }

    fn model_busy() -> Self {
        Self::with_action(
            6,
            ErrorKind::ModelBusy,
            "another model installer is active; retry after it finishes",
            true,
            Action::ModelsInstall,
        )
    }

    fn search_not_ready(message: impl Into<String>) -> Self {
        Self::with_action(8, ErrorKind::SearchNotReady, message, true, Action::Index)
    }

    fn schema(message: impl Into<String>) -> Self {
        Self::new(7, ErrorKind::SchemaIncompatible, message, false)
    }

    fn new(code: i32, kind: ErrorKind, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            error: ErrorBody {
                kind,
                message: message.into(),
                retryable,
                recommended_action: None,
            },
            code,
        }
    }

    fn with_action(
        code: i32,
        kind: ErrorKind,
        message: impl Into<String>,
        retryable: bool,
        recommended_action: Action,
    ) -> Self {
        let mut error = Self::new(code, kind, message, retryable);
        error.error.recommended_action = Some(recommended_action);
        error
    }
}
