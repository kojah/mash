//! Mutation capacity belongs to the work, including workers outliving their requester.
use crate::AppError;
use std::{future::Future, sync::Arc};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::{Instrument, instrument::WithSubscriber};

pub(super) fn reserve(capacity: &Arc<Semaphore>) -> Result<OwnedSemaphorePermit, AppError> {
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
    async fn cancellation_retains_capacity_through_blocking_work_and_publication() {
        let capacity = Arc::new(Semaphore::new(1));
        let (entered, started) = tokio::sync::oneshot::channel();
        let (release_cpu, cpu_gate) = std::sync::mpsc::channel();
        let (done_cpu, cpu_finished) = tokio::sync::oneshot::channel();
        let (release_publish, publish_gate) = tokio::sync::oneshot::channel();
        let permit = reserve(&capacity).unwrap();
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
        let error = reserve(&capacity).unwrap_err();
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
            reserve(&capacity).is_err(),
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
        let error = run(reserve(&capacity).unwrap(), async {
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
        run(reserve(&capacity).unwrap(), async {
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
