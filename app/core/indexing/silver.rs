//! Provider discovery and canonical reconciliation. No model execution.
use crate::{AppError, ingestion, storage::Storage};
use std::time::Instant;

pub(super) struct Ingestion {
    pub(super) summary: ingestion::IndexSummary,
    pub(super) milliseconds: u64,
}

impl Ingestion {
    pub(super) fn is_complete(&self) -> bool {
        self.summary.malformed_records == 0
    }
}

#[tracing::instrument(name = "silver_ingest", skip_all)]
pub(super) fn ingest(
    storage: &mut Storage,
    options: &ingestion::IndexOptions,
) -> Result<Ingestion, AppError> {
    let started = Instant::now();
    storage.defer_derived_updates()?;
    let summary = ingestion::index(storage, options)?;
    // Publish canonical changes and invalidation together, before building views.
    storage.commit_and_continue_writer()?;
    Ok(Ingestion {
        summary,
        milliseconds: super::elapsed_milliseconds(started),
    })
}
