//! Cross-process admission for a complete local silver-to-views mutation.
//! SQLite transactions alone do not cover awaited inference and gold writes.
use crate::AppError;
use std::{fs::File, path::Path, sync::Arc};

tokio::task_local! {
    static OWNER: Option<Arc<File>>;
}

/// Blocking workers retain ownership even if their awaiting future is dropped.
pub(crate) fn current() -> Option<Arc<File>> {
    OWNER.try_with(Clone::clone).ok().flatten()
}

pub(crate) async fn scope<T>(
    owner: Option<Arc<File>>,
    work: impl std::future::Future<Output = T>,
) -> T {
    OWNER.scope(owner, work).await
}

/// Fail promptly like BEGIN IMMEDIATE. Never unlink this file: a replacement
/// inode would let two processes each own a different lock for one database.
pub(crate) fn acquire(database: &Path) -> Result<Arc<File>, AppError> {
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
    let mut path = database.into_os_string();
    path.push(".writer.lock");
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(Path::new(&path))
        .map_err(AppError::io)?;
    match file.try_lock() {
        Ok(()) => Ok(Arc::new(file)),
        Err(std::fs::TryLockError::WouldBlock) => Err(AppError::new(
            6,
            crate::ErrorKind::IndexBusy,
            "another Mash operation is updating this local index; retry after it finishes",
            true,
        )),
        Err(std::fs::TryLockError::Error(error)) => Err(AppError::io(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    async fn local_writer_admission_survives_cancelled_blocking_awaiter() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("index.sqlite3");
        let owner = acquire(&database).unwrap();
        let (entered, started) = tokio::sync::oneshot::channel();
        let (release, released) = std::sync::mpsc::channel();
        let task = tokio::spawn(scope(Some(owner), async move {
            crate::blocking(move || {
                entered.send(()).unwrap();
                released.recv().unwrap();
                Ok(())
            })
            .await
        }));
        started.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(
            acquire(&database).unwrap_err().error.kind,
            crate::ErrorKind::IndexBusy
        );
        release.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if acquire(&database).is_ok() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }
}
