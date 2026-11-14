// Typed async search futures include transport tracing and concurrent retrieval.
#![recursion_limit = "256"]

// Silver canonical history and the vocabulary used by native adapters.
mod openapi;
use mash_model::{message_context, provider, role, tools};
mod silver;
mod silver_query;

// View materialization, retrieval models, policies and query-oriented views.
mod chat_catalog;
mod search_filter;
mod search_query;
mod search_ranking;
mod search_results;
mod semantic;
mod views;

// Application composition and infrastructure supporting both layers.
use mash_model::{Action, AppError, ErrorBody, ErrorKind};
use mash_model::{attachments, database_error, platform, redaction, scratch};
mod cli;
mod config;
mod core;
mod dash;
mod export;
mod mcp;
/// Build-time generation of the bundled history skill from the live MCP contract.
pub use mcp::guidance::{generate_history_skill, validate_history_release};
/// Registers the silver text codec on a raw connection, as tests need to read silver directly.
#[doc(hidden)]
pub use storage::packed::register as register_silver_codec;
mod observability;
mod progress;
mod records;
mod runtime;
mod server_cli;
mod shared;
mod storage;
#[cfg(test)]
mod test_support;
mod watch;

use std::ffi::OsString;
use std::io::{self, Write};

use serde::Serialize;

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    let sink = progress::current_sink();
    let indexer_cache = semantic::indexer::current();
    let context = observability::ThreadContext::capture();
    let admission = storage::admission::current();
    tokio::task::spawn_blocking(move || {
        storage::admission::on_blocking_thread(admission, || {
            context.run(|| {
                semantic::indexer::on_blocking_thread(indexer_cache, || {
                    progress::on_blocking_thread(sink, work)
                })
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
    execute(std::env::args_os(), &mut io::stdout(), &mut io::stderr())
}

/// Runs the standalone shared-index HTTP server with process arguments.
#[must_use]
pub fn server_main_entry() -> i32 {
    observability::initialize();
    match server_cli::run(std::env::args_os()) {
        Ok(Some(server_cli::Information::Help { help })) => {
            write_help(&mut io::stdout(), &mut io::stderr(), &help)
        }
        Ok(Some(response)) => write_json(&mut io::stdout(), &response).map_or_else(
            |error| {
                if error.code == BROKEN_PIPE {
                    0
                } else {
                    error.code
                }
            },
            |()| 0,
        ),
        Ok(None) => 0,
        Err(error) => {
            let _ = write_json(&mut io::stderr(), &error);
            error.code
        }
    }
}

fn execute<I, T>(args: I, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let arguments: Vec<OsString> = args.into_iter().map(Into::into).collect();
    match cli::run(arguments.clone()) {
        // MCP owns stdout for its entire session, including shutdown.
        Ok(cli::Response::McpStopped) => 0,
        // The async runtime and its lease are gone; start the replacement.
        Ok(cli::Response::WatchReplaced { reason, executable }) => {
            let _ = write_json(stderr, &reason);
            if cfg!(unix) {
                let _ = write_json(stderr, &WatchRestart::event());
                let error = runtime::restart(&executable, &arguments);
                let _ = write_json(stderr, &error);
                error.code
            } else {
                reason.code
            }
        }
        // Help is for a person to read: plain text, as clap renders it.
        Ok(cli::Response::Help { help }) => write_help(stdout, stderr, &help),
        Ok(response) => match write_json(stdout, &response) {
            // A reader that stopped early (`mash ... | head`) is not a failure.
            Err(error) if error.code == BROKEN_PIPE => 0,
            Err(error) => {
                let _ = write_json(stderr, &error);
                error.code
            }
            Ok(()) => match response {
                // A partial success keeps its report on stdout and its typed
                // error on stderr, as a failure would.
                cli::Response::Operation(value) => {
                    if let Some(error) = value.failure() {
                        let _ = write_json(stderr, error);
                        error.code
                    } else if value.needs_retry() {
                        9
                    } else {
                        i32::from(value.agents_need_attention())
                    }
                }
                _ => 0,
            },
        },
        Err(error) if error.code == BROKEN_PIPE => 0,
        Err(error) => {
            let _ = write_json(stderr, &error);
            error.code
        }
    }
}

/// Logged before a replaced watcher starts its replacement in place.
#[derive(Serialize)]
struct WatchRestart {
    event: &'static str,
    reason: &'static str,
}

impl WatchRestart {
    const fn event() -> Self {
        Self {
            event: "watch-restart",
            reason: "executable-replaced",
        }
    }
}

/// Internal code for a closed output stream; never reported as an exit status.
const BROKEN_PIPE: i32 = -1;

/// Prints help text and returns the exit code. A reader that stopped early
/// (`mash --help | head`) is not a failure.
fn write_help(stdout: &mut dyn Write, stderr: &mut dyn Write, help: &str) -> i32 {
    let text = help.trim_end();
    match writeln!(stdout, "{text}").and_then(|()| stdout.flush()) {
        Ok(()) => 0,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(error) => {
            let error = AppError::io(error);
            let _ = write_json(stderr, &error);
            error.code
        }
    }
}

fn write_json<T: Serialize>(writer: &mut dyn Write, value: &T) -> Result<(), AppError> {
    let closed = |mut error: AppError, kind: Option<io::ErrorKind>| {
        if kind == Some(io::ErrorKind::BrokenPipe) {
            error.code = BROKEN_PIPE;
        }
        error
    };
    serde_json::to_writer(&mut *writer, value).map_err(|error| {
        closed(
            AppError::internal(format!("failed to serialize response: {error}")),
            error.io_error_kind(),
        )
    })?;
    writeln!(writer).map_err(|error| {
        closed(
            AppError::internal(format!("failed to write response: {error}")),
            Some(error.kind()),
        )
    })
}

#[cfg(test)]
mod output_tests {
    use std::io::{self, Write};

    /// A reader that has gone away, as with `mash ... | head`.
    struct Closed;

    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_closed_output_pipe_exits_quietly() {
        let mut stderr = Vec::new();
        let code = super::execute(["mash", "--version"], &mut Closed, &mut stderr);
        assert_eq!(code, 0);
        assert!(stderr.is_empty(), "{}", String::from_utf8_lossy(&stderr));
    }
}
