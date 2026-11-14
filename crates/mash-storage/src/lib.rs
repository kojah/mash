//! Root of the `mash-storage` crate: the silver SQLite index, memory records,
//! and the search filter, result and catalogue types every layer above shares.
//!
//! An internal crate: its public items are what the rest of the workspace may
//! use, not a supported API.
use mash_model::{
    AppError, ErrorKind, attachments, database_error, message_context, provider, redaction, role,
    tools,
};
use mash_model::{platform, scratch};
#[cfg(test)]
use mash_model::{progress, test_support};

/// The chat model and adapters, with the archive format storage reads and writes.
pub mod silver {
    pub use mash_silver::*;
    pub mod archive;
}

pub mod chat_catalog;
pub mod export;
pub mod records;
pub mod search_filter;
pub mod search_query;
pub mod search_results;
pub mod silver_query;
pub mod storage;

/// Wire types of a publication that storage produces and applies.
pub mod shared {
    pub mod batches;
    pub mod validation;
}

/// View-side types that search hits and published documents are made of.
pub mod views {
    pub mod candidates;
    pub mod documents;
    pub mod exact;
    pub mod matching;
}
