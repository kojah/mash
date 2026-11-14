// Typed async search futures include transport tracing and concurrent retrieval.
#![recursion_limit = "256"]

// Silver canonical history and the vocabulary used by native adapters.
pub mod openapi;
pub use mash_model::{message_context, provider, role, tools};
pub mod silver;
pub mod silver_query;

// View materialization, retrieval models, policies and query-oriented views.
pub mod chat_catalog;
pub mod search_filter;
mod search_query;
mod search_ranking;
pub mod search_results;
pub mod semantic;
pub mod views;

// Application composition and infrastructure supporting both layers.
use mash_model::{Action, AppError, ErrorKind};
pub use mash_model::{attachments, database_error, platform, progress, redaction, scratch};
pub mod config;
pub mod core;
mod export;
/// Registers the silver text codec on a raw connection, as tests need to read silver directly.
#[doc(hidden)]
pub use storage::packed::register as register_silver_codec;
pub mod observability;
#[cfg(test)]
mod progress_tests;
pub mod records;
pub mod remote;
pub mod runtime;
pub mod shared;
pub mod storage;
#[cfg(test)]
use mash_model::test_support;
pub mod watch;

use std::io::{self, Write};

use serde::Serialize;

/// The commit this build was made from.
pub const BUILD_REVISION: &str = env!("MASH_BUILD_REVISION");

pub async fn blocking<T: Send + 'static>(
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

/// Internal code for a closed output stream; never reported as an exit status.
pub const BROKEN_PIPE: i32 = -1;

pub fn write_json<T: Serialize>(writer: &mut dyn Write, value: &T) -> Result<(), AppError> {
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
