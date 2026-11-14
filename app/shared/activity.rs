//! Ephemeral progress evidence, independent of admission and durable state.
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, Weak};
use std::time::Instant;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "ActivityKind")]
pub enum Kind {
    Publication,
    Attachment,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    WaitingForCapacity,
    Receiving,
    WaitingForNode,
    CheckingViews,
    ApplyingSilver,
    FinalizingSilver,
    CommittingSilver,
    WritingViews,
    CertifyingViews,
    OptimizingViews,
    CheckingSearch,
    Attachment,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    Bytes,
    Records,
    Chats,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub(crate) struct Active {
    operation_id: u64,
    node_id: String,
    kind: Kind,
    phase: Phase,
    elapsed_ms: u64,
    phase_elapsed_ms: u64,
    since_progress_ms: u64,
    completed: u64,
    unit: Unit,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[schemars(rename = "ActivityReport")]
pub struct Report {
    capacity: usize,
    occupied: usize,
    available: usize,
    active: Vec<Active>,
    scope: String,
}

struct Step {
    phase: Phase,
    started: Instant,
    updated: Instant,
    completed: u64,
    unit: Unit,
}
struct Record {
    id: u64,
    group: String,
    node: String,
    kind: Kind,
    started: Instant,
    step: Mutex<Step>,
}

#[derive(Clone, Default)]
pub struct Registry(Arc<Mutex<Vec<Weak<Record>>>>);

#[derive(Clone, Default)]
pub struct Progress(Option<Arc<Record>>);

impl Registry {
    pub fn start(&self, group: &str, node: &str, kind: Kind) -> Progress {
        let now = Instant::now();
        let record = Arc::new(Record {
            id: crate::observability::request_id(),
            group: group.into(),
            node: node.into(),
            kind,
            started: now,
            step: Mutex::new(Step {
                phase: Phase::Receiving,
                started: now,
                updated: now,
                completed: 0,
                unit: Unit::Bytes,
            }),
        });
        let mut entries = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        entries.retain(|entry| entry.strong_count() > 0);
        entries.push(Arc::downgrade(&record));
        tracing::info!(
            operation_id = record.id,
            node_id = node,
            ?kind,
            "shared operation started"
        );
        Progress(Some(record))
    }

    pub fn snapshot(&self, group: &str, capacity: usize, available: usize) -> Report {
        let entries = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let active = entries
            .iter()
            .filter_map(Weak::upgrade)
            .filter(|record| record.group == group)
            .map(|record| {
                let step = record
                    .step
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                Active {
                    operation_id: record.id,
                    node_id: record.node.clone(),
                    kind: record.kind,
                    phase: step.phase,
                    elapsed_ms: crate::progress::milliseconds(record.started),
                    phase_elapsed_ms: crate::progress::milliseconds(step.started),
                    since_progress_ms: crate::progress::milliseconds(step.updated),
                    completed: step.completed,
                    unit: step.unit,
                }
            })
            .collect();
        Report { capacity, occupied: capacity.saturating_sub(available), available, active,
            scope: "Live mutations for this authenticated group; capacity is service-wide. Counters describe the current phase, not committed records. No ETA or proof of a stall; a long SQLite statement may not report intermediate progress. Records disappear when work finishes and reset on server restart.".into() }
    }
}

impl Progress {
    pub fn phase(&self, phase: Phase, unit: Unit) {
        if let Some(record) = &self.0 {
            let now = Instant::now();
            let mut step = record
                .step
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            tracing::info!(operation_id = record.id, node_id = %record.node, ?phase, elapsed_ms = crate::progress::milliseconds(record.started), "shared operation phase");
            *step = Step {
                phase,
                unit,
                started: now,
                updated: now,
                completed: 0,
            };
        }
    }

    pub(crate) fn advance(&self, amount: u64) {
        if let Some(record) = &self.0 {
            let mut step = record
                .step
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            step.completed = step.completed.saturating_add(amount);
            step.updated = Instant::now();
        }
    }
}

impl Drop for Record {
    fn drop(&mut self) {
        tracing::info!(operation_id = self.id, node_id = %self.node, elapsed_ms = crate::progress::milliseconds(self.started), "shared operation ended; inspect mutation outcome for success");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_activity_tracks_worker_lifetime_and_never_exposes_other_groups() {
        let registry = Registry::default();
        let requester = registry.start("one", "node-a", Kind::Publication);
        let worker = requester.clone();
        let other = registry.start("two", "private-node", Kind::Publication);
        requester.phase(Phase::ApplyingSilver, Unit::Records);
        worker.advance(128);
        drop(requester);
        let report = registry.snapshot("one", 2, 0);
        assert_eq!(report.occupied, 2);
        assert_eq!(report.active.len(), 1);
        assert_eq!(report.active[0].completed, 128);
        assert!(
            !serde_json::to_string(&report)
                .unwrap()
                .contains("private-node")
        );
        drop(worker);
        assert!(registry.snapshot("one", 2, 1).active.is_empty());
        drop(other);
        assert!(registry.snapshot("two", 2, 2).active.is_empty());
    }
}
