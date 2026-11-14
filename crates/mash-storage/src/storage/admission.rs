//! Cross-process admission for a complete local silver-to-views mutation.
//! SQLite transactions alone do not cover awaited inference and gold writes.
use crate::AppError;
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug)]
pub struct Owner {
    _file: File,
    database: PathBuf,
}

impl Owner {
    pub fn covers(&self, database: &Path) -> Result<bool, AppError> {
        Ok(self.database == canonical(database)?)
    }
}

tokio::task_local! {
    static OWNER: Option<Arc<Owner>>;
}

thread_local! {
    static BLOCKING_OWNER: std::cell::RefCell<Option<Arc<Owner>>> = const { std::cell::RefCell::new(None) };
}

/// Blocking workers retain ownership even if their awaiting future is dropped.
pub fn current() -> Option<Arc<Owner>> {
    OWNER
        .try_with(Clone::clone)
        .ok()
        .flatten()
        .or_else(|| BLOCKING_OWNER.with_borrow(Clone::clone))
}

/// Carry admission into synchronous SQLite work, including schema upgrades.
pub fn on_blocking_thread<T>(owner: Option<Arc<Owner>>, work: impl FnOnce() -> T) -> T {
    struct Restore(Option<Arc<Owner>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            BLOCKING_OWNER.set(self.0.take());
        }
    }
    let _restore = Restore(BLOCKING_OWNER.replace(owner));
    work()
}

pub async fn scope<T>(owner: Option<Arc<Owner>>, work: impl std::future::Future<Output = T>) -> T {
    OWNER.scope(owner, work).await
}

/// Fail promptly like BEGIN IMMEDIATE. Never unlink this file: a replacement
/// inode would let two processes each own a different lock for one database.
fn canonical(database: &Path) -> Result<PathBuf, AppError> {
    let absolute = if database.is_absolute() {
        database.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(AppError::io)?
            .join(database)
    };
    let parent = absolute
        .parent()
        .ok_or_else(|| AppError::usage("database requires a parent directory"))?;
    std::fs::create_dir_all(parent).map_err(AppError::io)?;
    // Resolve existing database symlinks and parent aliases before choosing
    // the lock, including a new database whose file does not yet exist.
    let database = if absolute.exists() {
        std::fs::canonicalize(&absolute).map_err(AppError::io)?
    } else {
        std::fs::canonicalize(parent).map_err(AppError::io)?.join(
            absolute
                .file_name()
                .ok_or_else(|| AppError::usage("invalid database path"))?,
        )
    };
    Ok(database)
}

/// Fail promptly if another process owns the exact database's writer lock.
pub fn acquire(database: &Path) -> Result<Arc<Owner>, AppError> {
    let database = canonical(database)?;
    let mut path = database.clone().into_os_string();
    path.push(".writer.lock");
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(Path::new(&path))
        .map_err(AppError::io)?;
    match file.try_lock() {
        Ok(()) => Ok(Arc::new(Owner {
            _file: file,
            database,
        })),
        Err(std::fs::TryLockError::WouldBlock) => Err(AppError::new(
            6,
            crate::ErrorKind::IndexBusy,
            "another Mash operation is updating this local index; retry after it finishes",
            true,
        )),
        Err(std::fs::TryLockError::Error(error)) => Err(AppError::io(error)),
    }
}

/// How often a waiting writer retries admission.
const RETRY: std::time::Duration = std::time::Duration::from_millis(100);

/// Retry `acquire` until it succeeds, `deadline` passes or `cancelled`
/// resolves. A waiter holds nothing between attempts: each is the same
/// non-blocking lock attempt, so exclusion is unchanged and there is no queue
/// or fairness claim. Other failures return at once.
pub async fn acquire_until(
    database: &Path,
    deadline: tokio::time::Instant,
    cancelled: impl std::future::Future<Output = ()>,
) -> Result<Arc<Owner>, AppError> {
    tokio::pin!(cancelled);
    loop {
        let path = database.to_path_buf();
        // The attempt carries no caller context: it only tries the lock file.
        let attempt = tokio::task::spawn_blocking(move || acquire(&path))
            .await
            .map_err(|_| AppError::internal("blocking worker failed"))?;
        match attempt {
            Err(error) if error.error.kind == crate::ErrorKind::IndexBusy => {}
            result => return result,
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Err(still_busy());
        }
        tokio::select! {
            () = &mut cancelled => return Err(abandoned()),
            () = tokio::time::sleep_until(deadline.min(now + RETRY)) => {}
        }
    }
}

/// A bounded wait for admission ran out. Nothing was written.
pub fn still_busy() -> AppError {
    AppError::new(
        6,
        crate::ErrorKind::IndexBusy,
        "an index pass is still updating this local index, so nothing was saved; \
         retry this call with the same operation_id",
        true,
    )
}

/// The caller cancelled while waiting for admission. Nothing was written.
pub fn abandoned() -> AppError {
    AppError::new(
        6,
        crate::ErrorKind::IndexBusy,
        "cancelled while waiting for the local index, so nothing was saved",
        true,
    )
}

/// Whether some process is updating this database right now. A reader uses it
/// to tell search that is about to catch up from search nobody is refreshing.
/// The probe takes a shared lock for an instant, so a writer arriving in that
/// instant reports the index busy, as it would beside any other writer.
pub fn held(database: &Path) -> Result<bool, AppError> {
    let mut path = canonical(database)?.into_os_string();
    path.push(".writer.lock");
    let file = match File::open(Path::new(&path)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(AppError::io(error)),
    };
    match file.try_lock_shared() {
        Ok(()) => Ok(false),
        Err(std::fs::TryLockError::WouldBlock) => Ok(true),
        Err(std::fs::TryLockError::Error(error)) => Err(AppError::io(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reader_can_tell_whether_the_index_is_being_updated() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("index.sqlite3");
        assert!(!held(&database).unwrap());
        let owner = acquire(&database).unwrap();
        assert!(held(&database).unwrap());
        drop(owner);
        assert!(!held(&database).unwrap());
        // Asking leaves the lock free for the next writer.
        drop(acquire(&database).unwrap());
    }

    #[test]
    fn local_writer_admission_spans_transactions_and_releases_on_drop() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("index.sqlite3");
        let owner = acquire(&database).unwrap();
        assert!(!database.exists());
        let error = acquire(&database).unwrap_err();
        assert_eq!(error.error.kind, crate::ErrorKind::IndexBusy);
        let other = acquire(&directory.path().join("other.sqlite3")).unwrap();
        drop(owner);
        let next = acquire(&database).unwrap();
        drop((other, next));
    }

    #[cfg(unix)]
    #[test]
    fn local_writer_admission_resolves_existing_symlink_aliases() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("index.sqlite3");
        File::create(&database).unwrap();
        let alias = directory.path().join("alias.sqlite3");
        std::os::unix::fs::symlink(&database, &alias).unwrap();
        let _owner = acquire(&database).unwrap();
        assert_eq!(
            acquire(&alias).unwrap_err().error.kind,
            crate::ErrorKind::IndexBusy
        );
    }

    #[tokio::test]
    async fn bounded_admission_waits_for_release_and_gives_up_retryably() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("index.sqlite3");
        let deadline = |patience| tokio::time::Instant::now() + patience;
        let never = std::future::pending::<()>;
        let owner = acquire(&database).unwrap();
        // Exceeding the bound reports a retryable busy index.
        let error = acquire_until(
            &database,
            deadline(std::time::Duration::from_millis(250)),
            never(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.error.kind, crate::ErrorKind::IndexBusy);
        assert!(error.error.retryable);
        assert!(
            error.error.message.contains("retry"),
            "{}",
            error.error.message
        );
        // Cancellation ends the wait long before the bound.
        let (cancel, cancelled) = tokio::sync::oneshot::channel::<()>();
        let waiting = tokio::spawn({
            let database = database.clone();
            async move {
                acquire_until(
                    &database,
                    deadline(std::time::Duration::from_secs(60)),
                    async {
                        let _ = cancelled.await;
                    },
                )
                .await
            }
        });
        cancel.send(()).unwrap();
        let error = tokio::time::timeout(std::time::Duration::from_secs(5), waiting)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert_eq!(error.error.kind, crate::ErrorKind::IndexBusy);
        // A writer released within the bound admits the waiter, exclusively.
        let waiting = tokio::spawn({
            let database = database.clone();
            async move {
                acquire_until(
                    &database,
                    deadline(std::time::Duration::from_secs(60)),
                    never(),
                )
                .await
            }
        });
        tokio::time::sleep(3 * RETRY).await;
        assert!(!waiting.is_finished());
        drop(owner);
        let admitted = tokio::time::timeout(std::time::Duration::from_secs(5), waiting)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(held(&database).unwrap());
        assert_eq!(
            acquire(&database).unwrap_err().error.kind,
            crate::ErrorKind::IndexBusy
        );
        drop(admitted);
    }
}
