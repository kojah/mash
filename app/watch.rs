use std::collections::BTreeMap;
use std::ffi::OsString;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;

use crate::AppError;

const RETRY: Duration = Duration::from_secs(30);
const RECONCILE: Duration = Duration::from_secs(300);

pub(crate) struct Targets {
    pub(crate) roots: Vec<PathBuf>,
    pub(crate) files: Vec<PathBuf>,
}

impl Targets {
    fn relevant(&self, event: &Event) -> bool {
        if matches!(event.kind, EventKind::Access(_)) {
            return false;
        }
        event.need_rescan()
            || event.paths.iter().any(|path| {
                self.files
                    .iter()
                    .any(|file| file == path || file.starts_with(path))
                    || self.roots.iter().any(|root| {
                        root.starts_with(path)
                            || (path.starts_with(root)
                                && (path.extension().is_some_and(|ext| ext == "jsonl")
                                    || matches!(
                                        event.kind,
                                        EventKind::Create(notify::event::CreateKind::Folder)
                                            | EventKind::Remove(notify::event::RemoveKind::Folder)
                                            | EventKind::Modify(notify::event::ModifyKind::Name(_))
                                    )))
                    })
            })
    }
}

fn nearest_directory(path: &Path) -> Result<PathBuf, AppError> {
    path.ancestors()
        .find(|p| p.is_dir())
        .map(Path::to_path_buf)
        .ok_or_else(|| AppError::configuration("watch path has no existing parent directory"))
}

fn attach(mut targets: Targets, sender: mpsc::Sender<()>) -> Result<RecommendedWatcher, AppError> {
    let cwd = std::env::current_dir().map_err(|_| AppError::internal("resolve watch paths"))?;
    for path in targets.roots.iter_mut().chain(&mut targets.files) {
        if path.is_relative() {
            *path = cwd.join(&*path);
        }
    }
    let mut registrations = BTreeMap::<PathBuf, bool>::new();
    for root in &targets.roots {
        if root.is_dir() {
            registrations.insert(root.clone(), true);
        }
        let parent = root.parent().unwrap_or(root);
        registrations
            .entry(nearest_directory(parent)?)
            .or_insert(false);
    }
    for file in &targets.files {
        let parent = file.parent().unwrap_or(file);
        registrations
            .entry(nearest_directory(parent)?)
            .or_insert(false);
    }
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
        let relevant = if let Ok(event) = result {
            targets.relevant(&event)
        } else {
            eprintln!(
                "{{\"event\":\"watch-rescan\",\"reason\":\"filesystem notification failed\"}}"
            );
            true
        };
        if relevant {
            // One pending pass is sufficient, including changes during indexing.
            let _ = sender.try_send(());
        }
    })
    .map_err(|_| AppError::internal("initialize filesystem watcher"))?;
    for (path, recursive) in registrations {
        watcher
            .watch(
                &path,
                if recursive {
                    RecursiveMode::Recursive
                } else {
                    RecursiveMode::NonRecursive
                },
            )
            .map_err(|_| AppError::configuration(format!("cannot watch {}", path.display())))?;
    }
    Ok(watcher)
}

#[derive(Default)]
struct Dirty {
    first: Option<Instant>,
    last: Option<Instant>,
}

impl Dirty {
    fn mark(&mut self, now: Instant) {
        self.first.get_or_insert(now);
        self.last = Some(now);
    }
    fn deadline(&self, debounce: Duration) -> Option<Instant> {
        Some((self.last? + debounce).min(self.first? + debounce.max(Duration::from_secs(10))))
    }
}

// The native watcher only signals work. All indexing stays in the existing async
// pipeline, with at most one pass active and a bounded notification queue.
pub(crate) async fn run<F, Fut>(
    arguments: Vec<OsString>,
    debounce_ms: u64,
    shutdown: watch::Receiver<bool>,
    refresh: F,
) -> Result<(), AppError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<bool, AppError>>,
{
    run_with_intervals(arguments, debounce_ms, shutdown, refresh, RETRY, RECONCILE).await
}

async fn run_with_intervals<F, Fut>(
    arguments: Vec<OsString>,
    debounce_ms: u64,
    mut shutdown: watch::Receiver<bool>,
    mut refresh: F,
    retry_interval: Duration,
    reconcile_interval: Duration,
) -> Result<(), AppError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<bool, AppError>>,
{
    let debounce = Duration::from_millis(debounce_ms);
    let (sender, mut receiver) = mpsc::channel(1);
    let args = arguments.clone();
    let tx = sender.clone();
    let mut watcher =
        crate::blocking(move || attach(crate::cli::watch_targets(&args)?, tx)).await?;
    let mut dirty = Dirty::default();
    let mut due = Instant::now(); // Attach before the initial scan to avoid a startup gap.
    loop {
        if *shutdown.borrow() {
            break;
        }
        let deadline = dirty.deadline(debounce).map_or(due, |event| event.min(due));
        tokio::select! {
            biased;
            result = shutdown.changed() => {
                if result.is_err() || *shutdown.borrow() { break; }
            }
            () = tokio::time::sleep_until(deadline) => {
                dirty = Dirty::default();
                let args = arguments.clone();
                let tx = sender.clone();
                let replacement = crate::blocking(move || attach(crate::cli::watch_targets(&args)?, tx)).await;
                let retry = match replacement {
                    Ok(replacement) => {
                        let previous = std::mem::replace(&mut watcher, replacement);
                        crate::blocking(move || { drop(previous); Ok(()) }).await?;
                        match refresh().await {
                            Ok(retry) => retry,
                            Err(error) => { let _ = crate::write_json(&mut std::io::stderr(), &error); true }
                        }
                    }
                    Err(error) => {
                        // Retain the old watches so correcting invalid configuration wakes us.
                        let _ = crate::write_json(&mut std::io::stderr(), &error);
                        true
                    }
                };
                due = Instant::now() + if retry { retry_interval } else { reconcile_interval };
            }
            event = receiver.recv() => {
                if event.is_none() { break; }
                dirty.mark(Instant::now());
            }
        }
    }
    // Native watcher teardown may join its thread; keep it off the async executor.
    crate::blocking(move || {
        drop(watcher);
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pending_publication_retries_without_another_filesystem_event() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        std::fs::write(&config, "version = 1\n[providers]\n").unwrap();
        let arguments = vec![
            "mash".into(),
            "--config".into(),
            config.into_os_string(),
            "--models-dir".into(),
            dir.path().join("models").into_os_string(),
            "index".into(),
        ];
        let (stop, shutdown) = watch::channel(false);
        let mut passes = 0;
        let task = run_with_intervals(
            arguments,
            50,
            shutdown,
            || {
                passes += 1;
                let stop = stop.clone();
                let current = passes;
                async move {
                    if current == 2 {
                        stop.send(true).unwrap();
                    }
                    Ok(current == 1)
                }
            },
            Duration::from_millis(50),
            Duration::from_secs(60),
        );
        tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(passes, 2);
    }

    #[tokio::test]
    async fn changes_during_indexing_queue_a_sequential_followup() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        let root = dir.path().join("histories");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(
            &config,
            toml::to_string(&toml::toml! {
                version = 1
                [providers.codex]
                roots = [(root.to_str().unwrap())]
            })
            .unwrap(),
        )
        .unwrap();
        let arguments = vec![
            "mash".into(),
            "--config".into(),
            config.into_os_string(),
            "--models-dir".into(),
            dir.path().join("models").into_os_string(),
            "index".into(),
        ];
        let (stop, shutdown) = watch::channel(false);
        let mut passes = 0;
        let task = run_with_intervals(
            arguments,
            50,
            shutdown,
            || {
                passes += 1;
                let stop = stop.clone();
                let source = root.join("new.jsonl");
                let current = passes;
                async move {
                    if current == 1 {
                        std::fs::write(source, "changed while indexing").unwrap();
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    } else {
                        stop.send(true).unwrap();
                    }
                    Ok(false)
                }
            },
            Duration::from_secs(60),
            Duration::from_secs(60),
        );
        tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(passes, 2);
    }

    #[test]
    fn debounce_resets_until_quiet_but_continuous_writes_cannot_starve() {
        let now = Instant::now();
        let mut dirty = Dirty::default();
        dirty.mark(now);
        dirty.mark(now + Duration::from_millis(400));
        assert_eq!(
            dirty.deadline(Duration::from_secs(1)),
            Some(now + Duration::from_millis(1400))
        );
        dirty.mark(now + Duration::from_secs(12));
        assert_eq!(
            dirty.deadline(Duration::from_secs(1)),
            Some(now + Duration::from_secs(10))
        );
    }

    #[test]
    fn filter_ignores_reads_and_unrelated_files_but_tracks_removals_and_config() {
        let targets = Targets {
            roots: vec!["/history".into()],
            files: vec!["/settings/config.toml".into()],
        };
        let event = |kind, path: &str| Event::new(kind).add_path(path.into());
        assert!(!targets.relevant(&event(
            EventKind::Access(notify::event::AccessKind::Any),
            "/history/a.jsonl"
        )));
        assert!(!targets.relevant(&event(
            EventKind::Modify(notify::event::ModifyKind::Any),
            "/history/index.sqlite3"
        )));
        assert!(targets.relevant(&event(
            EventKind::Remove(notify::event::RemoveKind::File),
            "/history/a.jsonl"
        )));
        assert!(targets.relevant(&event(
            EventKind::Remove(notify::event::RemoveKind::Folder),
            "/history/nested"
        )));
        assert!(targets.relevant(&event(
            EventKind::Modify(notify::event::ModifyKind::Any),
            "/settings/config.toml"
        )));
    }
}
