//! Isolated SDK caches, atomic activation, and leases for lazy model readers.
use super::assets::{self, InstallMarker};
use crate::AppError;
use std::fs;
use std::path::{Path, PathBuf};
mod durability;
use durability::{sync_directory, sync_generation};

pub(super) const PREFIX: &str = ".mash-assets-";
pub(super) const LEASE_NAME: &str = ".mash-use.lock";

pub(super) struct SelectedAssets {
    pub(super) marker: InstallMarker,
    pub(super) root: PathBuf,
    pub(super) lease: Option<fs::File>,
}

pub(super) fn select(root: &Path) -> Result<SelectedAssets, AppError> {
    // A collector can retire the generation between reading the marker and
    // opening its lease. Retry selection, never load from an unleased directory.
    for _ in 0..3 {
        let marker = assets::read_marker(root)?;
        let asset_root = marker
            .asset_directory
            .as_ref()
            .map_or_else(|| root.to_path_buf(), |directory| root.join(directory));
        let lease = if marker.asset_directory.is_some() {
            match fs::symlink_metadata(&asset_root) {
                Ok(metadata) if metadata.file_type().is_dir() => (),
                Ok(_) => return Err(AppError::model("model asset generation is not a directory")),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(AppError::io(error)),
            }
            let file = match fs::File::open(asset_root.join(LEASE_NAME)) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(AppError::io(error)),
            };
            match file.try_lock_shared() {
                Ok(()) if asset_root.is_dir() => Some(file),
                Ok(()) | Err(fs::TryLockError::WouldBlock) => continue,
                Err(fs::TryLockError::Error(error)) => return Err(AppError::io(error)),
            }
        } else {
            // The original root is never changed by staged installers or GC.
            None
        };
        assets::validate_files(&asset_root, &marker)?;
        return Ok(SelectedAssets {
            marker,
            root: asset_root,
            lease,
        });
    }
    Err(AppError::model(
        "model assets changed during selection; retry the operation",
    ))
}

pub(super) struct StagedAssets {
    directory: tempfile::TempDir,
}

impl StagedAssets {
    pub(super) fn new(root: &Path) -> Result<Self, AppError> {
        cleanup(root)?;
        let directory = tempfile::Builder::new()
            .prefix(PREFIX)
            .tempdir_in(root)
            .map_err(AppError::io)?;
        fs::File::create(directory.path().join(LEASE_NAME)).map_err(AppError::io)?;
        let stage = Self { directory };
        if let Ok(selected) = select(root) {
            // Copy inventory entries, including snapshot symlink targets. Never
            // hard-link mutable SDK files or follow arbitrary directory trees.
            for file in &selected.marker.files {
                let source = selected.root.join(&file.path);
                let target = stage.root().join(&file.path);
                fs::create_dir_all(
                    target
                        .parent()
                        .ok_or_else(|| AppError::model("invalid staged asset path"))?,
                )
                .map_err(AppError::io)?;
                fs::copy(source, target).map_err(AppError::io)?;
            }
        }
        Ok(stage)
    }

    pub(super) fn root(&self) -> &Path {
        self.directory.path()
    }

    pub(super) fn publish(self, root: &Path, mut marker: InstallMarker) -> Result<(), AppError> {
        self.publish_with_directory_sync(root, &mut marker, sync_directory)
    }

    fn publish_with_directory_sync(
        self,
        root: &Path,
        marker: &mut InstallMarker,
        mut sync: impl FnMut(&Path) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        marker.asset_directory = Some(
            self.root()
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| AppError::model("invalid staged asset directory"))?
                .to_owned(),
        );
        sync_generation(self.root(), &mut sync)?;
        // Flush the generation's entry and any newly created model-path
        // ancestors before a durable marker can point at it.
        for directory in root.ancestors() {
            let directory = if directory.as_os_str().is_empty() {
                Path::new(".")
            } else {
                directory
            };
            sync(directory)?;
        }
        let prepared = assets::prepare_marker(root, marker)?;
        // Keep before publication: an unwind/kill after the atomic marker rename
        // must not delete the assets it selected. An interrupted earlier publish
        // leaves an orphan that the next explicit installation can collect.
        let _ = self.directory.keep();
        // An activation error can be ambiguous (for example after a filesystem
        // server processed a rename but its acknowledgment failed). Retain the
        // generation and let the next confirmed cleanup determine ownership.
        prepared
            .persist(root.join(super::MARKER_NAME))
            .map_err(|error| {
                let mut error = AppError::io(error.error);
                error.error.message = format!(
                    "could not confirm model activation; staged assets retained for recovery: {}",
                    error.error.message,
                );
                error.error.recommended_action = Some(crate::Action::ModelsInstall);
                error
            })?;
        // The rename already happened. An error here must preserve the newly
        // selected assets AND previous generations; deleting either could make
        // the currently visible or previously durable marker unusable.
        sync(root).map_err(|mut error| {
            error.error.message = format!(
                "model installation is active, but its directory flush failed; durability is unconfirmed: {}",
                error.error.message,
            );
            error.error.recommended_action = Some(crate::Action::ModelsInstall);
            error
        })?;
        // Cleanup failure cannot turn a successfully activated model into a
        // reported failed installation. Retry it at the next explicit install.
        let _ = cleanup(root);
        Ok(())
    }
}

fn cleanup(root: &Path) -> Result<(), AppError> {
    // Called only while the explicit installer holds its directory lock.
    // Confirm the visible marker's directory entry before retiring anything:
    // a previous activation may have succeeded but failed its final flush.
    sync_directory(root)?;
    // Do not retire anything if an existing marker cannot be interpreted.
    let active = if root.join(super::MARKER_NAME).exists() {
        match assets::read_marker(root) {
            Ok(marker) => marker.asset_directory,
            Err(_) => return Ok(()),
        }
    } else {
        None
    };
    for entry in fs::read_dir(root).map_err(AppError::io)? {
        let entry = entry.map_err(AppError::io)?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with(assets::MARKER_TEMP_PREFIX)
            && entry.file_type().map_err(AppError::io)?.is_file()
        {
            fs::remove_file(entry.path()).map_err(AppError::io)?;
            continue;
        }
        if !name.starts_with(PREFIX)
            || active.as_deref() == Some(name)
            || !entry.file_type().map_err(AppError::io)?.is_dir()
        {
            continue;
        }
        let file = match fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(entry.path().join(LEASE_NAME))
        {
            Ok(file) => file,
            // A kill during stage creation can leave an empty directory. It is
            // never readable or published without its lease file.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // A valid staging directory gets its lease before any assets.
                // Without one, only an empty creation orphan is ours to remove.
                if fs::read_dir(entry.path())
                    .map_err(AppError::io)?
                    .next()
                    .is_none()
                {
                    fs::remove_dir(entry.path()).map_err(AppError::io)?;
                }
                continue;
            }
            Err(error) => return Err(AppError::io(error)),
        };
        match file.try_lock() {
            Ok(()) => fs::remove_dir_all(entry.path()).map_err(AppError::io)?,
            Err(fs::TryLockError::WouldBlock) => (),
            Err(fs::TryLockError::Error(error)) => return Err(AppError::io(error)),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
