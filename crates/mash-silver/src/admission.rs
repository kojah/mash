//! Reserve estimated parser retention through application, including blocked sends.
use crate::AppError;
use std::sync::{Condvar, Mutex};

const PARSER_MEMORY_BUDGET: u64 = 128 * 1024 * 1024;
const MIN_SOURCE_RESERVATION: u64 = 1024 * 1024;

#[derive(Default)]
struct State {
    used: u64,
    next_ticket: u64,
    serving: u64,
    stats: crate::progress::ParserAdmission,
}

pub struct Admission {
    limit: u64,
    state: Mutex<State>,
    changed: Condvar,
}

pub struct Reservation<'a> {
    admission: &'a Admission,
    bytes: u64,
}

impl Default for Admission {
    fn default() -> Self {
        Self::new()
    }
}

impl Admission {
    pub fn new() -> Self {
        Self::with_limit(PARSER_MEMORY_BUDGET)
    }

    pub fn with_limit(limit: u64) -> Self {
        Self {
            limit,
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
        }
    }

    #[tracing::instrument(name = "parser_admission", skip_all, fields(source_bytes, budget_bytes = self.limit, reservation_bytes = tracing::field::Empty, waited = tracing::field::Empty, wait_ms = tracing::field::Empty))]
    pub fn reserve(&self, source_bytes: u64) -> Result<Reservation<'_>, AppError> {
        // Normalized text, projections, owned IDs and vector capacity can exceed
        // source bytes. This is conservative admission accounting, not an RSS cap.
        let bytes = source_bytes
            .saturating_mul(4)
            .saturating_add(MIN_SOURCE_RESERVATION)
            .min(self.limit);
        let started = std::time::Instant::now();
        let mut waited = false;
        let mut state = self.state.lock().map_err(|_| poisoned())?;
        let ticket = state.next_ticket;
        state.next_ticket = state
            .next_ticket
            .checked_add(1)
            .ok_or_else(|| AppError::internal("too many parser admission requests"))?;
        while ticket != state.serving || bytes > self.limit - state.used {
            waited = true;
            state = self.changed.wait(state).map_err(|_| poisoned())?;
        }
        state.used += bytes;
        state.serving += 1;
        state.stats.active_sources += 1;
        state.stats.peak_active_sources = state
            .stats
            .peak_active_sources
            .max(state.stats.active_sources);
        state.stats.peak_reserved_bytes = state.stats.peak_reserved_bytes.max(state.used);
        if waited {
            state.stats.waited_sources = state.stats.waited_sources.saturating_add(1);
            state.stats.wait_milliseconds = state
                .stats
                .wait_milliseconds
                .saturating_add(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX));
        }
        self.changed.notify_all();
        drop(state);
        let trace = tracing::Span::current();
        trace.record("reservation_bytes", bytes);
        trace.record("waited", waited);
        trace.record("wait_ms", crate::progress::milliseconds(started));
        Ok(Reservation {
            admission: self,
            bytes,
        })
    }

    pub fn snapshot(&self) -> Option<crate::progress::ParserAdmission> {
        let state = self.state.lock().ok()?;
        let mut snapshot = state.stats.clone();
        snapshot.budget_bytes = self.limit;
        snapshot.estimated_reserved_bytes = state.used;
        snapshot.waiting_sources = state.next_ticket - state.serving;
        Some(snapshot)
    }
}

fn poisoned() -> AppError {
    AppError::internal("parser admission lock is poisoned")
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        let mut state = self
            .admission
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.used -= self.bytes;
        state.stats.active_sources -= 1;
        self.admission.changed.notify_all();
    }
}

#[cfg(test)]
mod tests;
