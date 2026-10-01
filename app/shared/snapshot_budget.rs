//! Admission by serialized snapshot work; this is not a process RSS limit.
use crate::AppError;
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub(super) const DEFAULT_MIB: usize = 1024;
const MIB: u64 = 1024 * 1024;

#[derive(Clone)]
pub(super) struct Budget {
    limit_mib: usize,
    capacity: Arc<Semaphore>,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            limit_mib: DEFAULT_MIB,
            capacity: Arc::new(Semaphore::new(DEFAULT_MIB)),
        }
    }
}
impl Budget {
    pub(super) fn validate(limit_mib: usize) -> Result<(), AppError> {
        if !(1..=65536).contains(&limit_mib) {
            return Err(AppError::configuration(
                "max_mutation_snapshot_mib must be between 1 and 65536",
            ));
        }
        Ok(())
    }
    pub(super) fn new(limit_mib: usize) -> Result<Self, AppError> {
        Self::validate(limit_mib)?;
        Ok(Self {
            limit_mib,
            capacity: Arc::new(Semaphore::new(limit_mib)),
        })
    }
    #[tracing::instrument(name = "reserve_snapshot_work", skip_all, fields(work_mib = tracing::field::Empty, limit_mib = self.limit_mib, outcome = tracing::field::Empty))]
    pub(super) fn reserve(
        &self,
        existing: usize,
        incoming: u64,
    ) -> Result<OwnedSemaphorePermit, AppError> {
        let oversized = || {
            tracing::Span::current().record("outcome", "oversized");
            AppError::usage(
                "mutation snapshot work exceeds max_mutation_snapshot_mib; increase the server budget",
            )
        };
        let bytes = u64::try_from(existing)
            .ok()
            .and_then(|n| n.checked_add(incoming))
            .ok_or_else(oversized)?;
        let units = bytes.div_ceil(MIB).max(1);
        tracing::Span::current().record("work_mib", units);
        if units > u64::try_from(self.limit_mib).unwrap_or(0) {
            return Err(oversized());
        }
        let units = u32::try_from(units).map_err(|_| oversized())?;
        let permit = self
            .capacity
            .clone()
            .try_acquire_many_owned(units)
            .map_err(|_| {
                tracing::Span::current().record("outcome", "busy");
                AppError::shared("mutation snapshot capacity is busy; retry later")
            })?;
        tracing::Span::current().record("outcome", "accepted");
        Ok(permit)
    }
}

#[cfg(test)]
mod tests;
