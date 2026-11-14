//! Progress scoping through `crate::blocking`, which lives in this crate.
use std::time::Duration;

use crate::progress::{Phase, current_sink, during, phase, scope};

#[tokio::test]
async fn progress_is_scoped_across_async_tasks_blocking_workers_and_heartbeat_threads() {
    let (left, mut left_events) = tokio::sync::mpsc::channel(16);
    let (right, mut right_events) = tokio::sync::mpsc::channel(16);
    let left_work = scope(left, async {
        phase(Phase::WaitingForIndex, 0);
        crate::blocking(|| {
            during(Phase::ModelLoading, || {
                std::thread::sleep(Duration::from_millis(1_100));
            });
            Ok(())
        })
        .await
        .unwrap();
    });
    let right_work = scope(right, async {
        crate::blocking(|| {
            phase(Phase::Checkpoint, 0);
            Ok(())
        })
        .await
        .unwrap();
    });
    tokio::join!(left_work, right_work);
    let mut phases = Vec::new();
    while let Ok(event) = left_events.try_recv() {
        let phase = event.phase;
        assert!(matches!(
            phase,
            Phase::WaitingForIndex | Phase::ModelLoading
        ));
        phases.push((phase, event.elapsed_milliseconds));
    }
    assert!(
        phases
            .iter()
            .any(|(phase, elapsed)| *phase == Phase::ModelLoading && *elapsed >= 1_000)
    );
    assert_eq!(right_events.recv().await.unwrap().phase, Phase::Checkpoint);
    assert!(current_sink().is_none());
    crate::blocking(|| {
        assert!(current_sink().is_none());
        Ok(())
    })
    .await
    .unwrap();
}
