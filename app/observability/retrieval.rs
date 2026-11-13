//! Bounded request-local measurements, independent of tracing subscribers.
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    future::Future,
    sync::{Arc, Mutex},
    time::Instant,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[repr(usize)]
pub(crate) enum Stage {
    LocalExact,
    SharedCandidates,
    Context,
    Embed,
    Rerank,
    Fusion,
    LocalMetadata,
    SharedMetadata,
    MetadataMerge,
    LocalCandidates,
}
const COUNT: usize = 10;

/// Where contextual reranking ran. One search pass uses one engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum RerankerEngine {
    Cpu,
    /// The configured remote reranking worker.
    Remote,
    /// The local CPU, because the configured remote worker did not answer.
    CpuFallback,
}

/// Why a search configured for remote reranking used the local CPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum RerankerFallback {
    /// No reply within the configured bound, typically a cold worker.
    Timeout,
    /// The connection failed or the worker returned an HTTP error.
    Unavailable,
    /// The reply did not match the request, recipe or score bounds.
    InvalidResponse,
    /// The configured token environment variable is unset in this process.
    CredentialMissing,
    /// The installed reranker differs from the worker's pinned model.
    RecipeMismatch,
    /// The query or candidates exceed the worker's request limits.
    InputLimit,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub(crate) struct Measurement {
    pub(crate) stage: Stage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) reranker_engine: Option<RerankerEngine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) reranker_fallback: Option<RerankerFallback>,
    /// Placement of metadata/scope predicates, independent of call completion.
    #[serde(default)]
    pub(crate) filters_before_limit: bool,
    pub(crate) calls: u64,
    pub(crate) completed_calls: u64,
    pub(crate) total_us: u64,
    pub(crate) max_us: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) items_in: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) items_out: Option<u64>,
}

#[derive(Clone)]
pub(crate) struct Capture(Arc<Mutex<[Option<Measurement>; COUNT]>>);

impl Default for Capture {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(std::array::from_fn(|_| None))))
    }
}

tokio::task_local! { static ACTIVE: Capture; }
thread_local! { static BLOCKING: RefCell<Option<Capture>> = const { RefCell::new(None) }; }

pub(crate) fn current() -> Option<Capture> {
    ACTIVE
        .try_with(Clone::clone)
        .ok()
        .or_else(|| BLOCKING.with(|c| c.borrow().clone()))
}

pub(crate) fn on_blocking_thread<T>(capture: Option<Capture>, work: impl FnOnce() -> T) -> T {
    struct Restore(Option<Capture>);
    impl Drop for Restore {
        fn drop(&mut self) {
            BLOCKING.with(|c| c.replace(self.0.take()));
        }
    }
    let _restore = Restore(BLOCKING.with(|c| c.replace(capture)));
    work()
}

impl Capture {
    pub(crate) async fn scope<F: Future>(&self, work: F) -> F::Output {
        ACTIVE.scope(self.clone(), work).await
    }

    pub(crate) fn finish(&self) -> Vec<Measurement> {
        self.0.lock().map_or_else(
            |_| Vec::new(),
            |items| items.iter().flatten().cloned().collect(),
        )
    }
}

struct Active {
    capture: Capture,
    stage: Stage,
    started: Instant,
    complete: bool,
    items_in: Option<u64>,
    items_out: Option<u64>,
}

pub(crate) struct Guard(Option<Active>);

impl Guard {
    pub(crate) fn begin(stage: Stage) -> Self {
        Self(current().map(|capture| Active {
            capture,
            stage,
            started: Instant::now(),
            complete: false,
            items_in: None,
            items_out: None,
        }))
    }

    pub(crate) fn complete(&mut self, inputs: Option<usize>, outputs: Option<usize>) {
        if let Some(active) = &mut self.0 {
            active.complete = true;
            active.items_in = inputs.map(|n| u64::try_from(n).unwrap_or(u64::MAX));
            active.items_out = outputs.map(|n| u64::try_from(n).unwrap_or(u64::MAX));
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let Some(active) = &self.0 else { return };
        let elapsed = u64::try_from(active.started.elapsed().as_micros()).unwrap_or(u64::MAX);
        if let Ok(mut items) = active.capture.0.lock() {
            let item = items[active.stage as usize].get_or_insert(Measurement {
                stage: active.stage,
                reranker_engine: None,
                reranker_fallback: None,
                filters_before_limit: matches!(
                    active.stage,
                    Stage::LocalExact
                        | Stage::LocalCandidates
                        | Stage::SharedCandidates
                        | Stage::LocalMetadata
                        | Stage::SharedMetadata
                ),
                calls: 0,
                completed_calls: 0,
                total_us: 0,
                max_us: 0,
                items_in: None,
                items_out: None,
            });
            item.calls = item.calls.saturating_add(1);
            item.completed_calls = item
                .completed_calls
                .saturating_add(u64::from(active.complete));
            item.total_us = item.total_us.saturating_add(elapsed);
            item.max_us = item.max_us.max(elapsed);
            for (total, count) in [
                (&mut item.items_in, active.items_in),
                (&mut item.items_out, active.items_out),
            ] {
                if let Some(count) = count {
                    *total = Some(total.unwrap_or(0).saturating_add(count));
                }
            }
        }
    }
}

pub(crate) fn record_reranker(engine: RerankerEngine, fallback: Option<RerankerFallback>) {
    if let Some(capture) = current()
        && let Ok(mut items) = capture.0.lock()
        && let Some(item) = &mut items[Stage::Rerank as usize]
    {
        item.reranker_engine = Some(engine);
        item.reranker_fallback = fallback;
    }
}

pub(crate) fn measure_vec<T>(
    stage: Stage,
    inputs: Option<usize>,
    work: impl FnOnce() -> Result<Vec<T>, crate::AppError>,
) -> Result<Vec<T>, crate::AppError> {
    let mut guard = Guard::begin(stage);
    let result = work()?;
    guard.complete(inputs, Some(result.len()));
    Ok(result)
}

#[cfg(test)]
mod tests;
