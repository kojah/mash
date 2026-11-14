//! Root of the `mash-sync` crate: the shared index. It holds the node-side
//! client, sync and publication, shared retrieval, and the server's silver
//! store and view synchronization. Its files live under `app/shared/`.
//!
//! An internal crate: its public items are what the rest of the workspace may
//! use, not a supported API.
// Typed async search futures include transport tracing and concurrent retrieval.
#![recursion_limit = "256"]

#[cfg(test)]
use mash_model::test_support;
use mash_model::{
    AppError, ErrorKind, attachments, message_context, platform, progress, provider, redaction,
    role, tools,
};
use mash_search::{blocking, config, observability, search_ranking, semantic, views};
use mash_storage::{
    chat_catalog, records, search_filter, search_query, search_results, silver, silver_query,
    storage,
};

pub mod shared;
