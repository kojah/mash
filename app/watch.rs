use crate::core::Request;
use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;

use crate::AppError;

const RETRY: Duration = Duration::from_secs(30);
const RECONCILE: Duration = Duration::from_secs(300);
const STANDBY_POLL: Duration = Duration::from_secs(1);

/// Observe the leader lock without creating files or registering a watcher.
/// A held lock is evidence of leadership, not successful indexing or freshness.
pub(crate) fn active(database: &Path) -> std::io::Result<bool> {
    let mut path = std::fs::canonicalize(database)?.into_os_string();
    path.push(".watch-lock");
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    match file.try_lock_shared() {
        Ok(()) => Ok(false), // Dropping the file releases our temporary probe.
        Err(std::fs::TryLockError::WouldBlock) => Ok(true),
        Err(std::fs::TryLockError::Error(error)) => Err(error),
    }
}

#[test]
fn freshness_probe_observes_lock_ownership_not_file_existence() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("silver.sqlite3");
    std::fs::write(&database, []).unwrap();
    assert!(!active(&database).unwrap());
    let path = directory.path().join("silver.sqlite3.watch-lock");
    assert!(!path.exists());
    let leader = try_lead(&path).unwrap().unwrap();
    assert!(active(&database).unwrap());
    drop(leader);
    assert!(!active(&database).unwrap());
    assert!(try_lead(&path).unwrap().is_some());
    let other_probe = std::fs::File::open(&path).unwrap();
    other_probe.lock_shared().unwrap();
    assert!(!active(&database).unwrap());
    assert!(active(&directory.path().join("missing")).is_err());
}

/// Whether this process indexes for its database or waits for the leader to exit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Role {
    Leader,
    Standby,
}

/// One watcher per database across processes: the leader holds an exclusive
/// advisory lock beside the database for as long as it watches. The operating
/// system releases it when the holder exits or crashes.
fn lock_path(request: &Request) -> Result<PathBuf, AppError> {
    let database = request
        .paths
        .db
        .clone()
        .unwrap_or_else(crate::core::default_database_path);
    let database = if database.is_relative() {
        std::env::current_dir()
            .map_err(|_| AppError::internal("resolve watch database path"))?
            .join(database)
    } else {
        database
    };
    let name = database
        .file_name()
        .ok_or_else(|| AppError::configuration("database path has no file name"))?
        .to_os_string();
    let canonical = std::fs::canonicalize(&database).or_else(|_| {
        let parent = database.parent().unwrap_or(Path::new("/"));
        std::fs::create_dir_all(parent)
            .and_then(|()| std::fs::canonicalize(parent))
            .map(|parent| parent.join(&name))
    });
    let mut lock = canonical
        .map_err(|_| AppError::configuration("cannot resolve the watched database directory"))?
        .into_os_string();
    lock.push(".watch-lock");
    Ok(lock.into())
}

fn try_lead(path: &Path) -> Result<Option<std::fs::File>, AppError> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .map_err(|_| AppError::configuration(format!("cannot open {}", path.display())))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(_)) => Err(AppError::internal(format!(
            "cannot lock {}",
            path.display()
        ))),
    }
}

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
                            || (root.extension().is_some_and(|extension| {
                                matches!(extension.to_str(), Some("db" | "vscdb" | "sqlite"))
                            }) && root.parent() == path.parent()
                                && path.file_name().is_some_and(|name| {
                                    let mut wal_name =
                                        root.file_name().unwrap_or_default().to_os_string();
                                    wal_name.push("-wal");
                                    name == wal_name
                                }))
                            || (path.starts_with(root)
                                && (path.extension().is_some_and(|ext| {
                                    ext == "jsonl" || ext == "db" || ext == "sqlite"
                                }) || path.file_name().is_some_and(|name| {
                                    name.to_str().is_some_and(|n| {
                                        n.ends_with(".db-wal") || n.ends_with(".sqlite-wal")
                                    }) || name == "state.vscdb"
                                        || name == "state.vscdb-wal"
                                }) || matches!(
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

#[tracing::instrument(name = "watch_attach", skip_all, fields(roots = targets.roots.len()))]
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
    request: Request,
    debounce_ms: u64,
    shutdown: watch::Receiver<bool>,
    role: impl FnMut(Role),
    refresh: F,
) -> Result<(), AppError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<bool, AppError>>,
{
    crate::semantic::indexer::scope(elect(
        request,
        debounce_ms,
        shutdown,
        role,
        refresh,
        Intervals {
            retry: RETRY,
            reconcile: RECONCILE,
            standby: STANDBY_POLL,
        },
    ))
    .await
}

#[derive(Clone, Copy)]
struct Intervals {
    retry: Duration,
    reconcile: Duration,
    standby: Duration,
}

// A standby neither attaches filesystem watches nor indexes. Once it acquires
// the lock it watches with its own settings, starting with a full pass, so
// changes made while no process led are indexed. Stopping finishes the active
// pass before the lock (and leadership) is released.
async fn elect<F, Fut>(
    request: Request,
    debounce_ms: u64,
    mut shutdown: watch::Receiver<bool>,
    mut role: impl FnMut(Role),
    refresh: F,
    intervals: Intervals,
) -> Result<(), AppError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<bool, AppError>>,
{
    let path = {
        let request = request.clone();
        crate::blocking(move || lock_path(&request)).await?
    };
    let mut standby = false;
    let lease = loop {
        if *shutdown.borrow() {
            return Ok(());
        }
        let attempt = path.clone();
        if let Some(lease) = crate::blocking(move || try_lead(&attempt)).await? {
            break lease;
        }
        if !standby {
            standby = true;
            role(Role::Standby);
        }
        tokio::select! {
            biased;
            result = shutdown.changed() => {
                if result.is_err() { return Ok(()); }
            }
            () = tokio::time::sleep(intervals.standby) => {}
        }
    };
    role(Role::Leader);
    let result = run_with_intervals(
        request,
        debounce_ms,
        shutdown,
        refresh,
        intervals.retry,
        intervals.reconcile,
    )
    .await;
    drop(lease);
    result
}

async fn run_with_intervals<F, Fut>(
    request: Request,
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
    let initial_request = request.clone();
    let tx = sender.clone();
    let mut watcher =
        crate::blocking(move || attach(crate::core::watch_targets(&initial_request)?, tx)).await?;
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
                let request = request.clone();
                let tx = sender.clone();
                let replacement = crate::blocking(move || attach(crate::core::watch_targets(&request)?, tx)).await;
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

    #[test]
    fn native_history_and_database_wal_events_trigger_refresh_for_file_or_directory_roots() {
        use notify::event::{AccessKind, DataChange, ModifyKind};
        let root = PathBuf::from("/histories");
        let directory = Targets {
            roots: vec![root.clone()],
            files: Vec::new(),
        };
        let database = Targets {
            roots: vec![root.join("state.db")],
            files: Vec::new(),
        };
        let custom = Targets {
            roots: vec![root.join("custom.db")],
            files: Vec::new(),
        };
        let ide = Targets {
            roots: vec![root.join("state.vscdb")],
            files: Vec::new(),
        };
        let change = |path| {
            Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content))).add_path(path)
        };
        assert!(directory.relevant(&change(root.join("state.db-wal"))));
        assert!(directory.relevant(&change(root.join("store.db"))));
        assert!(directory.relevant(&change(root.join("store.db-wal"))));
        assert!(directory.relevant(&change(root.join("state.vscdb"))));
        assert!(directory.relevant(&change(
            root.join("conversations/50000000-0000-4000-8000-000000000001.db")
        )));
        assert!(directory.relevant(&change(
            root.join("conversations/50000000-0000-4000-8000-000000000001.db-wal")
        )));
        assert!(directory.relevant(&change(root.join("state.vscdb-wal"))));
        assert!(ide.relevant(&change(root.join("state.vscdb-wal"))));
        assert!(directory.relevant(&change(root.join("opencode.db-wal"))));
        assert!(database.relevant(&change(root.join("state.db-wal"))));
        assert!(custom.relevant(&change(root.join("custom.db-wal"))));
        let codex = Targets {
            roots: vec![
                root.join("thread_history_1.sqlite"),
                root.join("state_5.sqlite"),
            ],
            files: vec![],
        };
        for name in [
            "thread_history_1.sqlite",
            "thread_history_1.sqlite-wal",
            "state_5.sqlite",
            "state_5.sqlite-wal",
        ] {
            assert!(codex.relevant(&change(root.join(name))));
            assert!(directory.relevant(&change(root.join(name))));
        }
        assert!(!custom.relevant(&change(root.join("other.db-wal"))));
        assert!(!database.relevant(
            &Event::new(EventKind::Access(AccessKind::Read)).add_path(root.join("state.db-wal"))
        ));
        assert!(!directory.relevant(&change(root.join("README.md"))));
    }

    #[tokio::test]
    async fn runtime_shutdown_finishes_the_admitted_pass_before_releasing_leadership() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config.toml");
        std::fs::write(&config, "version=1\nproviders={}\n").unwrap();
        let database = root.path().join("index.sqlite3");
        let executable = root.path().join("mash");
        std::fs::write(&executable, "running").unwrap();
        let request = Request {
            paths: crate::core::Paths {
                config: Some(config),
                db: Some(database.clone()),
                models_dir: None,
            },
            operation: crate::core::Operation::Index(crate::core::IndexRequest::default()),
        };
        let lease = crate::runtime::Lease::fixture(&request.paths, &executable);
        let lock = lock_path(&request).unwrap();
        let (stop, shutdown) = watch::channel(false);
        let entered = std::sync::Arc::new(tokio::sync::Notify::new());
        let release = std::sync::Arc::new(tokio::sync::Notify::new());
        let entered_pass = entered.clone();
        let release_pass = release.clone();
        let output = database.clone();
        let task = tokio::spawn(async move {
            run(request, 50, shutdown, |_| {}, || {
                lease.ensure_current().unwrap();
                let entered = entered_pass.clone();
                let release = release_pass.clone();
                let output = output.clone();
                async move {
                    entered.notify_one();
                    release.notified().await;
                    crate::blocking(move || {
                        let connection = rusqlite::Connection::open(output).unwrap();
                        connection.execute_batch("CREATE TABLE completed(value INTEGER); INSERT INTO completed VALUES(1);").unwrap();
                        Ok(())
                    }).await?;
                    Ok(false)
                }
            }).await
        });
        tokio::time::timeout(crate::test_support::HANG_GUARD, entered.notified())
            .await
            .unwrap();
        std::fs::write(executable, "replacement").unwrap();
        stop.send(true).unwrap();
        assert!(!task.is_finished());
        assert!(try_lead(&lock).unwrap().is_none());
        release.notify_one();
        tokio::time::timeout(crate::test_support::HANG_GUARD, task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(try_lead(&lock).unwrap().is_some());
        let connection = rusqlite::Connection::open(database).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT value FROM completed", [], |row| row
                    .get::<_, i32>(0))
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn pending_publication_retries_without_another_filesystem_event() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        std::fs::write(&config, "version = 1\n[providers]\n").unwrap();
        let request = Request {
            paths: crate::core::Paths {
                config: Some(config),
                db: None,
                models_dir: Some(dir.path().join("models")),
            },
            operation: crate::core::Operation::Index(crate::core::IndexRequest::default()),
        };
        let (stop, shutdown) = watch::channel(false);
        let mut passes = 0;
        let task = run_with_intervals(
            request,
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
        let request = Request {
            paths: crate::core::Paths {
                config: Some(config),
                db: None,
                models_dir: Some(dir.path().join("models")),
            },
            operation: crate::core::Operation::Index(crate::core::IndexRequest::default()),
        };
        let (stop, shutdown) = watch::channel(false);
        let mut passes = 0;
        let task = run_with_intervals(
            request,
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

    fn election_request(dir: &Path, db: &str) -> Request {
        let config = dir.join("config.toml");
        Request {
            paths: crate::core::Paths {
                config: Some(config),
                db: Some(dir.join(db)),
                models_dir: Some(dir.join("models")),
            },
            operation: crate::core::Operation::Index(crate::core::IndexRequest::default()),
        }
    }

    type Events = tokio::sync::mpsc::UnboundedSender<(usize, Option<Role>)>;

    // Each candidate reports role changes as `Some(role)` and passes as `None`.
    fn candidate(
        request: Request,
        id: usize,
        events: Events,
    ) -> (
        watch::Sender<bool>,
        tokio::task::JoinHandle<Result<(), AppError>>,
    ) {
        let (stop, shutdown) = watch::channel(false);
        let roles = events.clone();
        let task = tokio::spawn(elect(
            request,
            50,
            shutdown,
            move |role| roles.send((id, Some(role))).unwrap(),
            move || {
                events.send((id, None)).unwrap();
                async { Ok(false) }
            },
            Intervals {
                retry: Duration::from_secs(600),
                reconcile: Duration::from_secs(600),
                standby: Duration::from_millis(20),
            },
        ));
        (stop, task)
    }

    async fn next(
        events: &mut tokio::sync::mpsc::UnboundedReceiver<(usize, Option<Role>)>,
    ) -> (usize, Option<Role>) {
        tokio::time::timeout(crate::test_support::HANG_GUARD, events.recv())
            .await
            .expect("watch election event")
            .unwrap()
    }

    #[tokio::test]
    async fn one_watcher_indexes_per_database_and_a_standby_takes_over_with_a_pass() {
        let dir = tempfile::tempdir().unwrap();
        // Prepare the shared configuration before any watcher starts. Rewriting it
        // for another candidate would legitimately trigger the existing leader.
        std::fs::write(dir.path().join("config.toml"), "version = 1\n[providers]\n").unwrap();
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let mut candidates = Vec::new();
        for id in 0..3 {
            candidates.push(candidate(
                election_request(dir.path(), "index.sqlite3"),
                id,
                sender.clone(),
            ));
        }
        let mut roles = [None; 3];
        let mut passes = [0; 3];
        while roles.iter().any(Option::is_none) || passes.iter().sum::<usize>() == 0 {
            match next(&mut events).await {
                (id, Some(role)) => roles[id] = Some(role),
                (id, None) => passes[id] += 1,
            }
        }
        let leaders: Vec<usize> = (0..3)
            .filter(|&id| roles[id] == Some(Role::Leader))
            .collect();
        assert_eq!(leaders.len(), 1, "{roles:?}");
        let leader = leaders[0];
        assert_eq!(passes[leader], 1);
        assert_eq!(passes.iter().sum::<usize>(), 1, "standbys never index");

        // Another database elects independently while this leader holds its lock.
        let other = candidate(
            election_request(dir.path(), "other.sqlite3"),
            3,
            sender.clone(),
        );
        assert_eq!(next(&mut events).await, (3, Some(Role::Leader)));
        assert_eq!(next(&mut events).await, (3, None));
        other.0.send(true).unwrap();
        other.1.await.unwrap().unwrap();

        // Stopping the leader releases leadership to exactly one standby,
        // which starts with its own initial pass.
        let (stop, task) = candidates.remove(leader);
        stop.send(true).unwrap();
        task.await.unwrap().unwrap();
        let (successor, role) = next(&mut events).await;
        assert_eq!(role, Some(Role::Leader));
        assert_ne!(successor, leader);
        assert_eq!(next(&mut events).await, (successor, None));
        for (stop, task) in candidates {
            stop.send(true).unwrap();
            task.await.unwrap().unwrap();
        }
        drop(sender);
        assert_eq!(
            events.recv().await,
            None,
            "the remaining standby stayed idle"
        );
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
