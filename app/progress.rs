use std::collections::VecDeque;
use std::io::{IsTerminal, Write};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use indicatif::{ProgressBar, ProgressStyle};

use serde_json::{Value, json};

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

pub(crate) fn emit(event: &Value) {
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

fn render(event: &Value) {
    let Ok(mut terminal) = TERMINAL.lock() else {
        return;
    };
    let bar = terminal.get_or_insert_with(|| {
        let bar = ProgressBar::new_spinner();
        bar.enable_steady_tick(Duration::from_millis(100));
        bar
    });
    let phase = event["phase"].as_str().unwrap_or("index");
    if matches!(phase, "complete" | "shared-pending") {
        bar.finish_with_message(display(event));
        terminal.take();
        return;
    }
    let counter = if phase == "shared-commit" {
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

fn counter(event: &Value) -> Option<(u64, u64, &'static str)> {
    [
        ("stored_vectors", "total_vectors", "vectors"),
        ("uploaded_bytes", "total_bytes", "bytes"),
        ("processed_files", "scanned_files", "files"),
    ]
    .into_iter()
    .find_map(|(done, total, unit)| Some((event[done].as_u64()?, event[total].as_u64()?, unit)))
}

fn display(event: &Value) -> String {
    let phase = event["phase"].as_str().unwrap_or("index");
    let counter = counter(event)
        .map(|(done, total, unit)| format!(" · {done}/{total} {unit}"))
        .unwrap_or_default();
    let eta = event["eta_seconds"].as_u64().map_or_else(
        || "ETA estimating".to_owned(),
        |seconds| format!("ETA {}m {:02}s", seconds / 60, seconds % 60),
    );
    let elapsed = event["elapsed_milliseconds"].as_u64().unwrap_or(0) / 1_000;
    let scope = event["eta_scope"].as_str().unwrap_or("phase");
    format!("mash index · {phase}{counter} · {elapsed}s elapsed · {eta} ({scope})")
}

pub(crate) fn phase(name: &str, elapsed_ms: u64) {
    emit(&json!({"event":"index-progress", "phase":name,
        "elapsed_milliseconds":elapsed_ms, "eta_seconds":null, "eta_scope":"phase"}));
}

// Keep expensive model loading / SQLite rebuilds visible even before there is
// measurable work. The scoped monitor stops on errors as well as success.
pub(crate) fn during<T>(name: &str, work: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    phase(name, 0);
    std::thread::scope(|scope| {
        let (stop, receiver) = std::sync::mpsc::channel::<()>();
        scope.spawn(move || {
            while matches!(
                receiver.recv_timeout(Duration::from_secs(1)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ) {
                phase(name, milliseconds(started));
            }
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
            display(&json!({"phase":"semantic-embeddings", "stored_vectors":32,
            "total_vectors":128,"elapsed_milliseconds":1_000,"eta_seconds":3})),
            "mash index · semantic-embeddings · 32/128 vectors · 1s elapsed · ETA 0m 03s (phase)"
        );
        assert!(display(&json!({"phase":"shared-commit"})).contains("ETA estimating"));
    }
}
