use super::*;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn oversized_source_runs_alone_and_queued_result_keeps_its_reservation() {
    let admission = Admission::with_limit(16);
    let oversized = admission.reserve(u64::MAX).unwrap();
    let (sender, receiver) = mpsc::channel();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let reservation = admission.reserve(1).unwrap();
            sender.send(()).unwrap();
            drop(reservation);
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while admission.state.lock().unwrap().next_ticket != 2 {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(receiver.recv_timeout(Duration::from_millis(30)).is_err());
        // Model the transfer to a queued parsed result: moving the reservation
        // must not allow a second parser to allocate until the consumer drops it.
        let queued = oversized;
        assert_eq!(admission.state.lock().unwrap().used, 16);
        assert!(receiver.recv_timeout(Duration::from_millis(30)).is_err());
        drop(queued);
        receiver.recv_timeout(Duration::from_secs(2)).unwrap();
    });
    assert_eq!(admission.state.lock().unwrap().used, 0);
    let stats = admission.snapshot().unwrap();
    assert_eq!(stats.peak_reserved_bytes, 16);
    assert_eq!(stats.peak_active_sources, 1);
    assert_eq!(stats.active_sources, 0);
    assert_eq!(stats.waiting_sources, 0);
    assert_eq!(stats.waited_sources, 1);
}

#[test]
fn reservations_cover_parallel_workers_and_error_or_disconnect_drops() {
    let admission = Admission::new();
    let first = admission.reserve(1).unwrap();
    let second = admission.reserve(1).unwrap();
    assert_eq!(
        admission.state.lock().unwrap().used,
        first.bytes + second.bytes
    );
    let (sender, receiver) = mpsc::sync_channel(1);
    sender.send(first).unwrap();
    drop(receiver);
    assert_eq!(admission.state.lock().unwrap().used, second.bytes);
    assert!(sender.send(second).is_err());
    assert_eq!(admission.state.lock().unwrap().used, 0);
}

#[test]
fn waiting_large_source_precedes_new_small_sources() {
    let admission = Admission::with_limit(8 * MIN_SOURCE_RESERVATION);
    let held = admission.reserve(1).unwrap();
    let (sender, receiver) = mpsc::channel();
    std::thread::scope(|scope| {
        let sender = &sender;
        let admission = &admission;
        scope.spawn(move || {
            let _large = admission.reserve(u64::MAX).unwrap();
            sender.send("large").unwrap();
        });
        // Wait for the first waiter to take its ticket; bounded by an explicit
        // deadline rather than relying on a scheduling sleep.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while admission.state.lock().unwrap().next_ticket != 2 {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        scope.spawn(move || {
            let _small = admission.reserve(1).unwrap();
            sender.send("small").unwrap();
        });
        drop(held);
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(2)).unwrap(),
            "large"
        );
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(2)).unwrap(),
            "small"
        );
    });
}
