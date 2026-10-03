//! Provider discovery and canonical reconciliation. No model execution.
use crate::{AppError, silver, storage::Storage};
use std::time::Instant;

pub(crate) struct Ingestion {
    pub(crate) summary: silver::IndexSummary,
    pub(crate) milliseconds: u64,
}

impl Ingestion {
    pub(crate) fn is_complete(&self) -> bool {
        self.summary.malformed_records == 0
    }
}

#[tracing::instrument(name = "silver_ingest", skip_all)]
pub(crate) fn ingest(
    storage: &mut Storage,
    options: &silver::IndexOptions,
) -> Result<Ingestion, AppError> {
    let started = Instant::now();
    storage.defer_derived_updates()?;
    let summary = silver::index(storage, options)?;
    // Publish canonical changes and invalidation together, before building views.
    storage.commit_and_continue_writer()?;
    Ok(Ingestion {
        summary,
        milliseconds: crate::progress::milliseconds(started),
    })
}
