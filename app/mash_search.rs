//! Root of the `mash-search` crate: configuration, the embedding and
//! reranking models, local and shared Qdrant views, and ranking. Its files
//! live beside the application library's under `app/`, as `mash-storage`'s do;
//! a module belongs to whichever root declares it.
//!
//! An internal crate: its public items are what the rest of the workspace may
//! use, not a supported API.
// Typed async search futures include transport tracing and concurrent retrieval.
#![recursion_limit = "256"]

use mash_model::{
    Action, AppError, ErrorKind, attachments, message_context, platform, progress, provider,
    redaction, role, scratch,
};
#[cfg(test)]
use mash_model::{test_support, tools};
use mash_storage::{records, search_filter, search_query, search_results, silver, storage};

pub mod config;
pub mod observability;
pub mod search_ranking;
pub mod semantic;
pub mod views;

/// Runs `work` on a blocking thread with the caller's progress sink, model
/// cache, tracing context and writer admission.
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
