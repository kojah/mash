//! Typed retained history for rebuilding gold without accessing bronze.
use super::{silver_revision, snapshot::layers};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Retained {
    pub(super) build: crate::gold::config::Build,
    pub(super) publication_revision: i64,
    pub(super) silver: silver_revision::State,
    pub(super) generation: String,
    pub(super) dimensions: usize,
    pub(super) history: layers::Silver,
}

#[derive(Serialize)]
pub(super) struct Snapshot<'a> {
    pub(super) build: &'a crate::gold::config::Build,
    pub(super) publication_revision: i64,
    pub(super) silver: &'a silver_revision::State,
    pub(super) generation: &'a str,
    pub(super) dimensions: usize,
    pub(super) history: layers::SilverRef<'a>,
}
