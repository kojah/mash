//! Vocabulary shared by every Mash crate: roles, providers, tool activity,
//! attachments, credential masking, platform helpers and the typed error.
//!
//! An internal crate: its public items are what the rest of the workspace may
//! use, not a supported API.

pub mod attachments;
pub mod database_error;
pub mod diagnostics;
mod error;
pub mod message_context;
pub mod platform;
pub mod progress;
pub mod provider;
pub mod redaction;
pub mod role;
pub mod scratch;
pub mod test_support;
pub mod tools;

pub use error::{Action, AppError, ErrorBody, ErrorKind};
