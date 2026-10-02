use super::*;
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn fixed_stage_capture_propagates_to_blocking_work_without_payloads() {
    let capture = Capture::default();
    async {
        {
            let _ignored =
                tracing::info_span!("private-name-canary", query = "private-query-canary")
                    .entered();
            tracing::info!(
                credential = "private-credential-canary",
                "private-event-canary"
            );
        }
        for _ in 0..2 {
            crate::blocking(|| {
                let _span =
                    tracing::info_span!("embed", history = "private-history-canary").entered();
                tracing::info!(path = "private-path-canary", "not collected");
                Ok(())
            })
            .await
            .unwrap();
        }
    }
    .with_subscriber(capture.dispatch())
    .await;
    let timings = capture.finish();
    assert_eq!(timings.len(), 1);
    assert_eq!(timings[0].stage, Stage::Embed);
    assert_eq!(timings[0].calls, 2);
    assert!(valid(&timings));
    let wire = serde_json::to_string(&timings).unwrap();
    assert!(!wire.contains("canary"));
    assert!(!wire.contains("not collected"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn overlapping_requests_keep_stage_counts_isolated() {
    async fn run(capture: Capture, count: usize) {
        async {
            for _ in 0..count {
                crate::blocking(|| {
                    let _span = tracing::info_span!("score_vectors").entered();
                    Ok(())
                })
                .await
                .unwrap();
                tokio::task::yield_now().await;
            }
        }
        .with_subscriber(capture.dispatch())
        .await;
    }
    let first = Capture::default();
    let second = Capture::default();
    tokio::join!(run(first.clone(), 3), run(second.clone(), 7));
    for (capture, count) in [(first, 3), (second, 7)] {
        let timings = capture.finish();
        assert_eq!(timings.len(), 1);
        assert_eq!(timings[0].stage, Stage::ScoreVectors);
        assert_eq!(timings[0].calls, count);
        assert!(valid(&timings));
    }
}
