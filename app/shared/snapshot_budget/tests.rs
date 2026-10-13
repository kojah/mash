use super::*;

#[test]
fn existing_and_incoming_bytes_share_rounded_capacity_across_clones() {
    let budget = Budget::new(3).unwrap();
    let peer = budget.clone();
    let first = budget.reserve(1, MIB).unwrap();
    assert_eq!(budget.capacity.available_permits(), 1);
    let second = peer.reserve(0, MIB).unwrap();
    let error = budget.reserve(0, 1).unwrap_err();
    assert!(error.error.retryable);
    assert_eq!(error.error.kind, crate::ErrorKind::SharedIndex);
    drop(first);
    let reclaimed = peer.reserve(0, 2 * MIB).unwrap();
    drop((second, reclaimed));
    assert_eq!(budget.capacity.available_permits(), 3);
}

#[test]
fn oversized_work_and_overflow_are_nonretryable_without_consuming_capacity() {
    let budget = Budget::new(1).unwrap();
    for (existing, incoming) in [(0, MIB + 1), (1, MIB), (1, u64::MAX)] {
        let error = budget.reserve(existing, incoming).unwrap_err();
        assert_eq!(error.error.kind, crate::ErrorKind::Usage);
        assert!(!error.error.retryable);
        assert!(error.error.message.contains("max_mutation_snapshot_mib"));
        assert_eq!(budget.capacity.available_permits(), 1);
    }
    for invalid in [0, 65537, usize::MAX] {
        assert!(Budget::new(invalid).is_err());
    }
    assert!(Budget::new(65536).is_ok());
}

#[tokio::test]
async fn cancelled_request_retains_weight_until_blocking_job_finishes() {
    let budget = Budget::new(1).unwrap();
    let reservation = budget.reserve(0, MIB).unwrap();
    let (entered, started) = tokio::sync::oneshot::channel();
    let (release, gate) = std::sync::mpsc::channel();
    let requester = tokio::spawn(async move {
        crate::blocking(move || {
            let _reservation = reservation;
            entered.send(()).unwrap();
            gate.recv().unwrap();
            Ok(())
        })
        .await
    });
    tokio::time::timeout(crate::test_support::HANG_GUARD, started)
        .await
        .unwrap()
        .unwrap();
    requester.abort();
    assert!(requester.await.unwrap_err().is_cancelled());
    assert!(budget.reserve(0, MIB).is_err());
    release.send(()).unwrap();
    let permit = tokio::time::timeout(
        crate::test_support::HANG_GUARD,
        budget.capacity.clone().acquire_owned(),
    )
    .await
    .unwrap()
    .unwrap();
    drop(permit);
    assert!(budget.reserve(0, MIB).is_ok());
}
