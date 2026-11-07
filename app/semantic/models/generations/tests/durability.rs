//! Filesystem persistence barriers and activation failure recovery.
use super::*;
use crate::semantic::MARKER_NAME;

#[test]
fn failed_activation_retains_prepared_assets_until_confirmed_cleanup() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(MARKER_NAME)).unwrap();
    let next = StagedAssets::new(root.path()).unwrap();
    let path = next.root().to_path_buf();
    let value = marker(next.root());
    let error = next.publish(root.path(), value).unwrap_err();
    assert_eq!(error.error.kind, crate::ErrorKind::Io);
    assert!(
        error
            .error
            .message
            .contains("could not confirm model activation")
    );
    assert_eq!(
        error.error.recommended_action,
        Some(crate::Action::ModelsInstall)
    );
    assert!(path.join(RERANKER_REPOSITORY).join("refs/main").is_file());
    fs::remove_dir(root.path().join(MARKER_NAME)).unwrap();
    cleanup(root.path()).unwrap();
    assert!(!path.exists());
}

#[test]
fn directory_flush_failures_preserve_assets_on_both_sides_of_activation() {
    #[derive(Clone, Copy)]
    enum FailurePoint {
        Assets,
        Activation,
    }
    for point in [FailurePoint::Assets, FailurePoint::Activation] {
        let root = tempfile::tempdir().unwrap();
        let first = StagedAssets::new(root.path()).unwrap();
        let first_path = first.root().to_path_buf();
        let value = marker(first.root());
        first.publish(root.path(), value).unwrap();
        let before = fs::read(root.path().join(MARKER_NAME)).unwrap();
        let next = StagedAssets::new(root.path()).unwrap();
        let next_path = next.root().to_path_buf();
        let name = next_path.file_name().unwrap().to_str().unwrap().to_owned();
        let mut value = marker(next.root());
        let error = next
            .publish_with_directory_sync(root.path(), &mut value, |path| {
                let fail = match point {
                    FailurePoint::Assets => path == next_path,
                    FailurePoint::Activation => {
                        path == root.path()
                            && assets::read_marker(root.path())
                                .unwrap()
                                .asset_directory
                                .as_deref()
                                == Some(&name)
                    }
                };
                if fail {
                    Err(AppError::io(std::io::Error::other(
                        "injected directory flush failure",
                    )))
                } else {
                    sync_directory(path)
                }
            })
            .unwrap_err();
        assert_eq!(error.error.kind, crate::ErrorKind::Io);
        assert!(error.error.retryable);
        assert!(first_path.is_dir());
        match point {
            FailurePoint::Assets => {
                assert_eq!(fs::read(root.path().join(MARKER_NAME)).unwrap(), before);
                assert!(!next_path.exists());
                assert_eq!(select(root.path()).unwrap().root, first_path);
            }
            FailurePoint::Activation => {
                assert!(error.error.message.contains("installation is active"));
                assert_eq!(
                    error.error.recommended_action,
                    Some(crate::Action::ModelsInstall)
                );
                assert!(next_path.is_dir());
                assert_eq!(select(root.path()).unwrap().root, next_path);
                cleanup(root.path()).unwrap();
                assert!(!first_path.exists());
                assert!(next_path.is_dir());
            }
        }
    }
}

#[test]
fn asset_directories_are_flushed_before_activation_and_marker_directory_afterward() {
    let root = tempfile::tempdir().unwrap();
    let first = StagedAssets::new(root.path()).unwrap();
    let value = marker(first.root());
    first.publish(root.path(), value).unwrap();
    let next = StagedAssets::new(root.path()).unwrap();
    let path = next.root().to_path_buf();
    let name = path.file_name().unwrap().to_str().unwrap().to_owned();
    let mut value = marker(next.root());
    let mut events = Vec::new();
    next.publish_with_directory_sync(root.path(), &mut value, |directory| {
        let active = assets::read_marker(root.path())
            .unwrap()
            .asset_directory
            .as_deref()
            == Some(&name);
        events.push((directory.to_path_buf(), active));
        sync_directory(directory)
    })
    .unwrap();
    let refs = events
        .iter()
        .position(|(directory, _)| *directory == path.join(RERANKER_REPOSITORY).join("refs"))
        .unwrap();
    let repository = events
        .iter()
        .position(|(directory, _)| *directory == path.join(RERANKER_REPOSITORY))
        .unwrap();
    let generation = events
        .iter()
        .position(|(directory, _)| *directory == path)
        .unwrap();
    let parent = events
        .iter()
        .position(|(directory, _)| *directory == root.path())
        .unwrap();
    assert!(refs < repository && repository < generation && generation < parent);
    assert!(events[..=parent].iter().all(|(_, active)| !active));
    assert_eq!(events.last().unwrap(), &(root.path().to_path_buf(), true));
}

#[cfg(unix)]
#[test]
fn incomplete_staged_asset_tree_cannot_replace_a_working_marker() {
    let root = tempfile::tempdir().unwrap();
    install_fixture(root.path());
    let before = fs::read(root.path().join(MARKER_NAME)).unwrap();
    let next = StagedAssets::new(root.path()).unwrap();
    let path = next.root().to_path_buf();
    let value = marker(next.root());
    std::os::unix::fs::symlink("missing-download", next.root().join("incomplete")).unwrap();
    assert_eq!(
        next.publish(root.path(), value).unwrap_err().error.kind,
        crate::ErrorKind::Io
    );
    assert!(!path.exists());
    assert_eq!(fs::read(root.path().join(MARKER_NAME)).unwrap(), before);
    assert!(select(root.path()).is_ok());
}
