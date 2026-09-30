//! Serialize SDK construction and retire only unleased cache generations.
use crate::AppError;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const PREFIX: &str = ".mash-coreml-";
const LEASE: &str = ".use.lock";
const MARKER: &str = ".active.toml";

pub(super) enum BuildError {
    Input(AppError),
    Compile(AppError),
}

impl From<AppError> for BuildError {
    fn from(error: AppError) -> Self {
        Self::Input(error)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    directory: String,
}

fn lock_file(path: &Path) -> Result<File, AppError> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(AppError::io)
}

fn selected(root: &Path) -> Result<PathBuf, AppError> {
    let file = match File::open(root.join(MARKER)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(root.to_path_buf()),
        Err(error) => return Err(AppError::io(error)),
    };
    let mut text = String::new();
    file.take(4097)
        .read_to_string(&mut text)
        .map_err(AppError::io)?;
    if text.len() > 4096 {
        return Err(AppError::model("Core ML cache marker is too large"));
    }
    let marker: Marker = toml::from_str(&text)
        .map_err(|error| AppError::model(format!("invalid Core ML cache marker: {error}")))?;
    if !marker.directory.starts_with(PREFIX)
        || !marker
            .directory
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    {
        return Err(AppError::model("invalid Core ML cache generation name"));
    }
    let directory = root.join(marker.directory);
    if !fs::symlink_metadata(&directory)
        .map_err(AppError::io)?
        .file_type()
        .is_dir()
    {
        return Err(AppError::model(
            "Core ML cache generation is not a directory",
        ));
    }
    Ok(directory)
}

fn lease(directory: &Path) -> Result<File, AppError> {
    let file = lock_file(&directory.join(LEASE))?;
    file.lock_shared().map_err(AppError::io)?;
    Ok(file)
}

fn publish(root: &Path, directory: &Path) -> Result<(), AppError> {
    let name = directory
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::internal("invalid Core ML cache directory"))?;
    let text = toml::to_string(&Marker {
        directory: name.to_owned(),
    })
    .map_err(|error| AppError::internal(format!("serialize Core ML cache marker: {error}")))?;
    let mut file = tempfile::NamedTempFile::new_in(root).map_err(AppError::io)?;
    file.write_all(text.as_bytes()).map_err(AppError::io)?;
    file.as_file().sync_all().map_err(AppError::io)?;
    file.persist(root.join(MARKER))
        .map_err(|error| AppError::io(error.error))?;
    File::open(root)
        .and_then(|file| file.sync_all())
        .map_err(AppError::io)
}

fn collect(root: &Path, active: &Path) -> Result<(), AppError> {
    for entry in fs::read_dir(root).map_err(AppError::io)? {
        let entry = entry.map_err(AppError::io)?;
        if entry.path() == active
            || !entry.file_name().to_string_lossy().starts_with(PREFIX)
            || !entry.file_type().map_err(AppError::io)?.is_dir()
        {
            continue;
        }
        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .open(entry.path().join(LEASE))
        {
            Ok(file) => file,
            // A creation interrupted before leasing can leave an empty stage.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if fs::read_dir(entry.path())
                    .map_err(AppError::io)?
                    .next()
                    .is_none()
                {
                    fs::remove_dir(entry.path()).map_err(AppError::io)?;
                }
                continue;
            }
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => continue,
            Err(error) => return Err(AppError::io(error)),
        };
        match file.try_lock() {
            Ok(()) => match fs::remove_dir_all(entry.path()) {
                Ok(()) => (),
                // An externally read-only retired cache cannot prevent a
                // healthy replacement from loading. Keep it for later cleanup.
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => (),
                Err(error) => return Err(AppError::io(error)),
            },
            Err(fs::TryLockError::WouldBlock) => (),
            Err(fs::TryLockError::Error(error)) => return Err(AppError::io(error)),
        }
    }
    Ok(())
}

pub(super) fn load<T>(
    root: &Path,
    mut build: impl FnMut(&Path) -> Result<T, BuildError>,
) -> Result<(T, File), AppError> {
    fs::create_dir_all(root).map_err(AppError::io)?;
    let guard = lock_file(&root.join(".compile.lock"))?;
    guard.lock().map_err(AppError::io)?;
    let directory = selected(root)?;
    collect(root, &directory)?;
    let reader = lease(&directory)?;
    match build(&directory) {
        Ok(model) => return Ok((model, reader)),
        Err(BuildError::Input(error)) => return Err(error),
        Err(BuildError::Compile(_)) => (),
    }
    drop(reader);
    // Never alter the failed namespace: another loaded session may still use it.
    // Only one fresh compilation attempt is made; failed stages are removed.
    let stage = tempfile::Builder::new()
        .prefix(PREFIX)
        .tempdir_in(root)
        .map_err(AppError::io)?;
    let reader = lease(stage.path())?;
    let model = build(stage.path()).map_err(|error| match error {
        BuildError::Input(error) | BuildError::Compile(error) => error,
    })?;
    let directory = stage.keep();
    publish(root, &directory)?;
    collect(root, &directory)?;
    Ok((model, reader))
}

#[cfg(test)]
mod tests;
