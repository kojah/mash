use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{IsTerminal, Write};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use indicatif::{ProgressBar, ProgressStyle};

mod event;
pub(crate) use event::{EtaScope, Event, ParserAdmission, Phase};

type Sink = tokio::sync::mpsc::Sender<Event>;

tokio::task_local! { static ASYNC_SINK: Sink; }
thread_local! { static BLOCKING_SINK: RefCell<Option<Sink>> = const { RefCell::new(None) }; }

pub(crate) fn current_sink() -> Option<Sink> {
    ASYNC_SINK
        .try_with(Clone::clone)
        .ok()
        .or_else(|| BLOCKING_SINK.with(|sink| sink.borrow().clone()))
}

pub(crate) async fn scope<T>(sink: Sink, work: impl std::future::Future<Output = T>) -> T {
    ASYNC_SINK.scope(sink, work).await
}

pub(crate) fn on_blocking_thread<T>(sink: Option<Sink>, work: impl FnOnce() -> T) -> T {
    struct Restore(Option<Sink>);
    impl Drop for Restore {
        fn drop(&mut self) {
            BLOCKING_SINK.with(|sink| *sink.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(BLOCKING_SINK.with(|current| current.replace(sink)));
    work()
}

// Estimates cover the current phase only: later phases may have unknown work.
pub(crate) struct Estimate {
    samples: VecDeque<(u64, u64)>,
}

impl Estimate {
    pub(crate) fn new() -> Self {
        Self::starting_at(0)
    }

    pub(crate) fn starting_at(completed: u64) -> Self {
        Self {
            samples: VecDeque::from([(0, completed)]),
        }
    }

    pub(crate) fn observe(&mut self, completed: u64, total: u64, elapsed_ms: u64) -> Option<u64> {
        if completed >= total {
            return Some(0);
        }
        if self
            .samples
            .back()
            .is_some_and(|&(time, count)| time > elapsed_ms || count > completed)
        {
            self.samples.clear();
        }
        self.samples.push_back((elapsed_ms, completed));
        while self.samples.len() > 2 && self.samples[1].0 <= elapsed_ms.saturating_sub(30_000) {
            self.samples.pop_front();
        }
        let &(time, count) = self.samples.front()?;
        let rate_count = completed.saturating_sub(count);
        let duration = elapsed_ms.saturating_sub(time);
        if duration == 0 || rate_count == 0 {
            return None;
        }
        let remaining_ms =
            u128::from(total - completed) * u128::from(duration) / u128::from(rate_count);
        u64::try_from(remaining_ms.div_ceil(1_000)).ok()
    }
}

pub(crate) fn emit(event: &Event) {
    if let Some(sink) = current_sink() {
        // Observability must not block ingestion or grow an unbounded queue
        // when a client is slow or disconnects. Intermediate updates may drop.
        let _ = sink.try_send(event.clone());
    }
    let stderr = std::io::stderr();
    if stderr.is_terminal() {
        render(event);
    } else {
        let mut writer = stderr.lock();
        if serde_json::to_writer(&mut writer, event).is_ok() {
            let _ = writeln!(writer);
        }
    }
}

static TERMINAL: Mutex<Option<ProgressBar>> = Mutex::new(None);

pub(crate) struct Session;

impl Drop for Session {
    fn drop(&mut self) {
        if let Ok(mut terminal) = TERMINAL.lock()
            && let Some(bar) = terminal.take()
        {
            bar.finish_and_clear();
        }
    }
}

fn render(event: &Event) {
    let Ok(mut terminal) = TERMINAL.lock() else {
        return;
    };
    let bar = terminal.get_or_insert_with(|| {
        let bar = ProgressBar::new_spinner();
        bar.enable_steady_tick(Duration::from_millis(100));
        bar
    });
    let phase = event.phase;
    if matches!(phase, Phase::Complete | Phase::SharedPending) {
        bar.finish_with_message(display(event));
        terminal.take();
        return;
    }
    let counter = if phase == Phase::SharedCommit {
        None
    } else {
        counter(event)
    };
    let template = if let Some((done, total, _)) = counter {
        bar.set_length(total);
        bar.set_position(done);
        "{spinner} [{bar:24.cyan/blue}] {msg}"
    } else {
        "{spinner} {msg}"
    };
    if let Ok(style) = ProgressStyle::with_template(template) {
        bar.set_style(style);
    }
    bar.set_message(display(event));
}

fn counter(event: &Event) -> Option<(u64, u64, &'static str)> {
    event
        .stored_vectors
        .zip(event.total_vectors)
        .map(|(done, total)| (done, total, "vectors"))
        .or_else(|| {
            event
                .uploaded_bytes
                .zip(event.total_bytes)
                .map(|(done, total)| (done, total, "bytes"))
        })
        .or_else(|| {
            event
                .processed_files
                .zip(event.scanned_files)
                .map(|(done, total)| (done, total, "files"))
        })
}

pub(crate) fn display(event: &Event) -> String {
    let phase = &event.phase;
    let counter = counter(event)
        .map(|(done, total, unit)| format!(" · {done}/{total} {unit}"))
        .unwrap_or_default();
    let eta = event.eta_seconds.map_or_else(
        || "ETA estimating".to_owned(),
        |seconds| format!("ETA {}m {:02}s", seconds / 60, seconds % 60),
    );
    let elapsed = event.elapsed_milliseconds / 1_000;
    let scope = &event.eta_scope;
    format!("mash index · {phase}{counter} · {elapsed}s elapsed · {eta} ({scope})")
}

pub(crate) fn phase(name: Phase, elapsed_ms: u64) {
    emit(&Event::new(name, elapsed_ms, None));
}

// Keep expensive model loading / SQLite rebuilds visible even before there is
// measurable work. The scoped monitor stops on errors as well as success.
pub(crate) fn during<T>(name: Phase, work: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    phase(name, 0);
    std::thread::scope(|scope| {
        let (stop, receiver) = std::sync::mpsc::channel::<()>();
        let sink = current_sink();
        scope.spawn(move || {
            on_blocking_thread(sink, || {
                while matches!(
                    receiver.recv_timeout(Duration::from_secs(1)),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                ) {
                    phase(name, milliseconds(started));
                }
            });
        });
        let result = work();
        drop(stop);
        result
    })
}

pub(crate) fn milliseconds(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn eta_handles_unknown_complete_and_changing_throughput() {
        let mut estimate = Estimate::new();
        assert_eq!(estimate.observe(0, 100, 1_000), None);
        assert_eq!(estimate.observe(10, 100, 2_000), Some(18));
        assert_eq!(estimate.observe(20, 100, 32_000), Some(240));
        assert_eq!(estimate.observe(30, 100, 62_000), Some(210));
        assert_eq!(estimate.observe(100, 100, 63_000), Some(0));
    }

    #[test]
    fn terminal_progress_shows_counts_and_phase_eta() {
        assert_eq!(
            display(&Event {
                stored_vectors: Some(32),
                total_vectors: Some(128),
                ..Event::new(Phase::SemanticEmbeddings, 1_000, Some(3))
            }),
            "mash index · semantic-embeddings · 32/128 vectors · 1s elapsed · ETA 0m 03s (phase)"
        );
        assert!(display(&Event::new(Phase::SharedCommit, 0, None)).contains("ETA estimating"));
    }
}
