//! Private scratch directories and SQLite files for sensitive indexing state.
use crate::AppError;
use std::path::Path;

pub(crate) fn directory(prefix: &str) -> Result<tempfile::TempDir, AppError> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(prefix);
    crate::platform::private_tempdir(&mut builder);
    builder.tempdir().map_err(AppError::io)
}

pub(crate) fn sqlite(path: &Path) -> Result<rusqlite::Connection, AppError> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::internal("scratch database has no directory"))?;
    let file = tempfile::NamedTempFile::new_in(parent).map_err(AppError::io)?;
    // Retain the temporary file's 0600 permissions. The owning directory
    // removes it on failure or completion; never overwrite an existing file.
    let handle = file
        .persist_noclobber(path)
        .map_err(|e| AppError::io(e.error))?;
    drop(handle);
    let connection = rusqlite::Connection::open(path).map_err(AppError::database)?;
    // Scratch state is discarded with its directory; syncing it to disk would
    // only stall indexing behind unrelated I/O.
    connection
        .pragma_update(None, "synchronous", "OFF")
        .map_err(AppError::database)?;
    Ok(connection)
}
