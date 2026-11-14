//! The `mash` and `mash-server` executables: argument parsing and output over
//! the typed application library, `mash-app`.
// The search futures this crate drives nest deeper than the default limit
// when the compiler proves them `Send`, as they do in `mash-app`.
#![recursion_limit = "256"]
mod cli;
mod server_cli;

use std::ffi::OsString;
use std::io::{self, Write};

use mash_app::{
    BROKEN_PIPE, blocking, chat_catalog, config, core, observability, remote, runtime,
    search_filter, search_results, semantic, shared, storage, views, watch, write_json,
};
use mash_dash as dash;
use mash_mcp as mcp;
use mash_model::{AppError, platform, provider, redaction, role};
use serde::Serialize;

/// Registers the silver text codec on a raw connection, as tests need to read silver directly.
#[doc(hidden)]
pub use mash_app::register_silver_codec;
/// Build-time generation of the bundled history skill from the live MCP contract.
pub use mash_mcp::guidance::{generate_history_skill, validate_history_release};

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
