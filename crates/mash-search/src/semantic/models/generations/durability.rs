//! File and directory flush ordering for model asset publication.
use crate::AppError;
use std::fs;
use std::path::Path;

pub(super) fn sync_generation(
    root: &Path,
    sync: &mut impl FnMut(&Path) -> Result<(), AppError>,
) -> Result<(), AppError> {
    let mut directories = Vec::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(|error| AppError::io(std::io::Error::other(error)))?;
        if entry.file_type().is_dir() {
            directories.push(entry.into_path());
        } else {
            if !fs::metadata(entry.path()).map_err(AppError::io)?.is_file() {
                return Err(AppError::model(
                    "model generation contains a non-file asset",
                ));
            }
            crate::platform::sync_file(entry.path()).map_err(AppError::io)?;
        }
    }
    // WalkDir visits parents first. Reverse directory order persists children
    // before their containing entries, including SDK symlinks and Core ML caches.
    for directory in directories.into_iter().rev() {
        sync(&directory)?;
    }
    Ok(())
}

pub(super) fn sync_directory(path: &Path) -> Result<(), AppError> {
    crate::platform::sync_directory(path).map_err(AppError::io)
}
