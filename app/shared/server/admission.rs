//! Mutation capacity belongs to the work, including workers outliving their requester.
use crate::AppError;
use std::{future::Future, sync::Arc};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::{Instrument, instrument::WithSubscriber};

/// Bound queued HTTP handlers independently of the disk-heavy worker limit.
pub(super) const MAX_WAITERS: usize = 32;
// A full publication can spend several minutes settling Qdrant. Keep queued
// requests in place through that work rather than forcing repeated submissions.
// The waiter count bounds retained handlers; this deadline also remains below
// the outer 30-minute bulk request budget.
const WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

pub(super) async fn reserve(
    capacity: &Arc<Semaphore>,
    waiters: &Arc<Semaphore>,
    progress: &crate::shared::activity::Progress,
) -> Result<OwnedSemaphorePermit, AppError> {
    reserve_with_timeout(capacity, waiters, progress, WAIT_TIMEOUT).await
}

async fn reserve_with_timeout(
    capacity: &Arc<Semaphore>,
    waiters: &Arc<Semaphore>,
    progress: &crate::shared::activity::Progress,
    timeout: std::time::Duration,
) -> Result<OwnedSemaphorePermit, AppError> {
    use crate::shared::activity::{Phase, Unit};
    let permit = if let Ok(permit) = capacity.clone().try_acquire_owned() {
        permit
    } else {
        // Tokio's semaphore respects already queued acquisitions, including
        // try_acquire. The waiter permit bounds handlers, not active workers.
        let _waiting = waiters.clone().try_acquire_owned().map_err(|_| {
            AppError::shared("shared index mutation capacity wait queue is full; retry later")
        })?;
        progress.phase(Phase::WaitingForCapacity, Unit::Records);
        tokio::time::timeout(timeout, capacity.clone().acquire_owned())
            .await
            .map_err(|_| AppError::shared("shared index mutation capacity wait timed out; inspect shared activity before retrying"))?
            .map_err(|_| AppError::shared("shared index mutation admission is closed"))?
    };
    progress.phase(Phase::Receiving, Unit::Bytes);
    Ok(permit)
}

#[cfg(test)]
fn try_reserve(capacity: &Arc<Semaphore>) -> Result<OwnedSemaphorePermit, AppError> {
    capacity
        .clone()
        .try_acquire_owned()
        .map_err(|_| AppError::shared("shared index mutation capacity is busy; retry later"))
}

#[tracing::instrument(name = "shared_mutation", skip_all, fields(outcome = tracing::field::Empty, error_kind = tracing::field::Empty))]
pub(super) async fn run<T: Send + 'static>(
    permit: OwnedSemaphorePermit,
    work: impl Future<Output = Result<T, AppError>> + Send + 'static,
) -> Result<T, AppError> {
    // Dropping the awaiting HTTP handler detaches this task; it must keep the
    // permit until blocking work and atomic publication have actually finished.
    tokio::spawn(
        async move {
            let _permit = permit;
            let result = work.await;
            tracing::Span::current().record("outcome", if result.is_ok() { "ok" } else { "error" });
            if let Err(error) = &result {
                tracing::Span::current()
                    .record("error_kind", tracing::field::debug(error.error.kind));
            }
            result
        }
        .in_current_span()
        .with_current_subscriber(),
    )
    .await
    .map_err(|_| AppError::internal("shared index mutation worker failed"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bounded_waiters_are_fifo_and_visible_before_receiving() {
        use crate::shared::activity::{Kind, Registry};
        let capacity = Arc::new(Semaphore::new(1));
        let waiters = Arc::new(Semaphore::new(2));
        let busy = try_reserve(&capacity).unwrap();
        let registry = Registry::default();
        let one = registry.start("group", "one", Kind::Publication);
        let two = registry.start("group", "two", Kind::Publication);
        let three = registry.start("group", "three", Kind::Publication);
        let mut first = Box::pin(reserve(&capacity, &waiters, &one));
        let mut second = Box::pin(reserve(&capacity, &waiters, &two));
        assert!(futures_util::poll!(&mut first).is_pending());
        assert!(futures_util::poll!(&mut second).is_pending());
        assert_eq!(waiters.available_permits(), 0);
        let report = serde_json::to_value(registry.snapshot("group", 1, 0)).unwrap();
        assert_eq!(report["active"][0]["phase"], "waiting_for_capacity");
        assert_eq!(report["active"][1]["phase"], "waiting_for_capacity");
        let error = reserve(&capacity, &waiters, &three).await.unwrap_err();
        assert!(error.error.message.contains("queue is full"));
        assert!(error.error.retryable);
        drop(busy);
        // A fresh arrival cannot steal a permit reserved for the FIFO head.
        assert!(try_reserve(&capacity).is_err());
        assert!(futures_util::poll!(&mut second).is_pending());
        let first = first.await.unwrap();
        assert_eq!(waiters.available_permits(), 1);
        let report = serde_json::to_value(registry.snapshot("group", 1, 0)).unwrap();
        assert_eq!(report["active"][0]["phase"], "receiving");
        assert!(futures_util::poll!(&mut second).is_pending());
        drop(first);
        drop(second.await.unwrap());
        assert_eq!(capacity.available_permits(), 1);
        assert_eq!(waiters.available_permits(), 2);
    }

    #[tokio::test]
    async fn cancelled_and_expired_waiters_release_queue_positions() {
        let capacity = Arc::new(Semaphore::new(1));
        let waiters = Arc::new(Semaphore::new(1));
        let progress = crate::shared::activity::Progress::default();
        let busy = try_reserve(&capacity).unwrap();
        let mut cancelled = Box::pin(reserve(&capacity, &waiters, &progress));
        assert!(futures_util::poll!(&mut cancelled).is_pending());
        assert_eq!(waiters.available_permits(), 0);
        drop(cancelled);
        assert_eq!(waiters.available_permits(), 1);
        let error = reserve_with_timeout(&capacity, &waiters, &progress, std::time::Duration::ZERO)
            .await
            .unwrap_err();
        assert!(error.error.message.contains("wait timed out"));
        assert_eq!(waiters.available_permits(), 1);
        assert_eq!(capacity.available_permits(), 0);
        drop(busy);
        drop(reserve(&capacity, &waiters, &progress).await.unwrap());
        assert_eq!(capacity.available_permits(), 1);
    }

    #[tokio::test]
    async fn cancellation_retains_capacity_through_blocking_work_and_publication() {
        let capacity = Arc::new(Semaphore::new(1));
        let (entered, started) = tokio::sync::oneshot::channel();
        let (release_cpu, cpu_gate) = std::sync::mpsc::channel();
        let (done_cpu, cpu_finished) = tokio::sync::oneshot::channel();
        let (release_publish, publish_gate) = tokio::sync::oneshot::channel();
        let permit = try_reserve(&capacity).unwrap();
        let activity = crate::shared::activity::Registry::default();
        let progress = activity.start("group", "node", crate::shared::activity::Kind::Publication);
        let requester = tokio::spawn(run(permit, async move {
            let _progress = progress;
            crate::blocking(move || {
                entered.send(()).unwrap();
                cpu_gate.recv().unwrap();
                Ok(())
            })
            .await?;
            done_cpu.send(()).unwrap();
            publish_gate.await.unwrap();
            Ok(())
        }));
        tokio::time::timeout(crate::test_support::HANG_GUARD, started)
            .await
            .unwrap()
            .unwrap();
        requester.abort();
        assert!(requester.await.unwrap_err().is_cancelled());
        let error = try_reserve(&capacity).unwrap_err();
        assert_eq!(error.error.kind, crate::ErrorKind::SharedIndex);
        assert!(error.error.retryable);
        let report = serde_json::to_value(activity.snapshot("group", 1, 0)).unwrap();
        assert_eq!(report["active"].as_array().unwrap().len(), 1);
        release_cpu.send(()).unwrap();
        tokio::time::timeout(crate::test_support::HANG_GUARD, cpu_finished)
            .await
            .unwrap()
            .unwrap();
        assert!(
            try_reserve(&capacity).is_err(),
            "publication still owns the reservation"
        );
        release_publish.send(()).unwrap();
        let permit = tokio::time::timeout(
            crate::test_support::HANG_GUARD,
            capacity.clone().acquire_owned(),
        )
        .await
        .unwrap()
        .unwrap();
        drop(permit);
        assert_eq!(capacity.available_permits(), 1);
        let report = serde_json::to_value(activity.snapshot("group", 1, 1)).unwrap();
        assert_eq!(report["active"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn mutation_errors_release_capacity_and_return_the_original_error() {
        let capacity = Arc::new(Semaphore::new(1));
        let error = run(try_reserve(&capacity).unwrap(), async {
            Err::<(), _>(AppError::usage("invalid mutation"))
        })
        .await
        .unwrap_err();
        assert_eq!(error.error.kind, crate::ErrorKind::Usage);
        assert_eq!(error.error.message, "invalid mutation");
        assert_eq!(capacity.available_permits(), 1);
    }

    #[tokio::test]
    async fn mutation_tasks_and_blocking_workers_preserve_trace_context_and_outcome() {
        #[derive(serde::Deserialize)]
        struct Event {
            fields: Fields,
            span: Option<Span>,
            #[serde(default)]
            spans: Vec<Span>,
        }
        #[derive(serde::Deserialize)]
        struct Fields {
            message: String,
        }
        #[derive(serde::Deserialize)]
        struct Span {
            name: String,
            marker: Option<u64>,
            outcome: Option<String>,
        }
        let (bytes, dispatcher) = crate::observability::test_support::capture();
        let parent = tracing::dispatcher::with_default(&dispatcher, || {
            tracing::info_span!("mutation_parent", marker = 42)
        });
        let capacity = Arc::new(Semaphore::new(1));
        run(try_reserve(&capacity).unwrap(), async {
            crate::blocking(|| {
                tracing::info!("mutation-worker-canary");
                Ok(())
            })
            .await
        })
        .instrument(parent)
        .with_subscriber(dispatcher)
        .await
        .unwrap();
        let output = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
        let events: Vec<Event> = output
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let worker = events
            .iter()
            .find(|e| e.fields.message == "mutation-worker-canary")
            .unwrap();
        assert!(
            worker
                .spans
                .iter()
                .any(|s| s.name == "mutation_parent" && s.marker == Some(42))
        );
        assert!(events.iter().any(|e| e.fields.message == "close"
            && e.span.as_ref().is_some_and(
                |s| s.name == "shared_mutation" && s.outcome.as_deref() == Some("ok")
            )));
    }
}
