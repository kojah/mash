use super::*;

#[tokio::test]
async fn request_measurements_propagate_to_blocking_work_and_remain_isolated() {
    let first = Capture::default();
    let second = Capture::default();
    let work = |capture: Capture, count| async move {
        capture
            .scope(async move {
                let mut guard = Guard::begin(Stage::SharedCandidates);
                crate::blocking(move || {
                    measure_vec(Stage::LocalMetadata, None, || Ok(vec![0; count]))
                })
                .await
                .unwrap();
                tokio::task::yield_now().await;
                guard.complete(None, Some(count));
            })
            .await;
    };
    tokio::join!(work(first.clone(), 2), work(second.clone(), 5));
    for (capture, expected) in [(first, 2), (second, 5)] {
        let measurements = capture.finish();
        assert_eq!(measurements.len(), 2);
        for measured in measurements {
            assert_eq!(measured.calls, 1);
            assert_eq!(measured.completed_calls, 1);
            assert_eq!(measured.items_out, Some(expected));
            assert!(measured.total_us >= measured.max_us);
            assert!(measured.filters_before_limit);
        }
    }
    assert!(current().is_none());
    crate::blocking(|| {
        assert!(current().is_none());
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn unknown_failed_and_zero_counts_are_distinct_and_bounded() {
    let capture = Capture::default();
    capture
        .scope(async {
            let _failed = Guard::begin(Stage::SharedCandidates);
            for _ in 0..1000 {
                measure_vec::<u8>(Stage::LocalExact, None, || Ok(vec![])).unwrap();
            }
        })
        .await;
    let measurements = capture.finish();
    assert_eq!(measurements.len(), 2);
    let exact = &measurements[0];
    assert_eq!(exact.completed_calls, 1000);
    assert_eq!(exact.items_out, Some(0));
    let failed = &measurements[1];
    assert_eq!(failed.calls, 1);
    assert_eq!(failed.completed_calls, 0);
    assert!(failed.filters_before_limit);
    assert!(failed.items_out.is_none());
    let unmeasured = Guard::begin(Stage::Embed);
    assert!(unmeasured.0.is_none());
}

#[tokio::test]
async fn non_retrieval_stages_do_not_claim_filter_placement() {
    let capture = Capture::default();
    capture
        .scope(async {
            measure_vec(Stage::Rerank, Some(1), || Ok(vec![1])).unwrap();
        })
        .await;
    assert!(!capture.finish()[0].filters_before_limit);
}
#[test]
fn reranker_diagnostics_report_cpu() {
    let capture = super::Capture::default();
    super::on_blocking_thread(Some(capture.clone()), || {
        let _: Result<Vec<()>, crate::AppError> =
            super::measure_vec(super::Stage::Rerank, Some(0), || Ok(vec![]));
        super::record_reranker(super::RerankerEngine::Cpu);
    });
    assert_eq!(
        capture.finish()[0].reranker_engine,
        Some(super::RerankerEngine::Cpu)
    );
}
