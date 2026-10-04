//! Durable interpretation and completeness contract for rebuilding derived views.
use serde::{Deserialize, Serialize};

/// Increment when native interpretation or canonical enrichment rules change.
/// A checkpoint produced by another version cannot skip provider replay.
pub(crate) const INTERPRETATION_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Completeness {
    Unverified,
    Pending,
    Incomplete,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Certificate {
    pub(crate) revision: u64,
    pub(crate) interpretation_version: u32,
    pub(crate) completeness: Completeness,
    pub(crate) rejected_sources: u64,
}

impl Certificate {
    pub(crate) fn is_complete(&self) -> bool {
        self.completeness == Completeness::Complete
    }
}
