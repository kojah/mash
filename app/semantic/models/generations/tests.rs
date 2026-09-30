use super::super::{
    EMBEDDING_MODEL, RERANKER_MODEL, RERANKER_REPOSITORY, coreml_embedding_enabled,
};
use super::*;

fn marker(root: &Path) -> InstallMarker {
    let path = root.join(RERANKER_REPOSITORY).join("refs/main");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"original revision").unwrap();
    InstallMarker {
        schema: 1,
        asset_directory: None,
        embedding_model: EMBEDDING_MODEL.to_string(),
        reranker_model: RERANKER_MODEL.to_string(),
        embedding_dimensions: 384,
        coreml_embedding: coreml_embedding_enabled(),
        embedding_available: true,
        files: assets::inventory(root).unwrap(),
    }
}

fn install_fixture(root: &Path) {
    assets::write_marker(root, &marker(root)).unwrap();
}

#[test]
fn failed_stage_preserves_active_assets_and_marker() {
    let root = tempfile::tempdir().unwrap();
    install_fixture(root.path());
    let original = fs::read(root.path().join(super::super::MARKER_NAME)).unwrap();
    let staged = StagedAssets::new(root.path()).unwrap();
    let staged_path = staged.root().to_path_buf();
    fs::write(
        staged.root().join(RERANKER_REPOSITORY).join("refs/main"),
        b"partial",
    )
    .unwrap();
    assert_eq!(
        fs::read(root.path().join(RERANKER_REPOSITORY).join("refs/main")).unwrap(),
        b"original revision"
    );
    drop(staged);
    assert!(!staged_path.exists());
    assert_eq!(
        fs::read(root.path().join(super::super::MARKER_NAME)).unwrap(),
        original
    );
    assert!(select(root.path(), false).is_ok());
}

#[test]
fn publication_pins_readers_and_collects_unleased_generations() {
    let root = tempfile::tempdir().unwrap();
    let first = StagedAssets::new(root.path()).unwrap();
    let first_marker = marker(first.root());
    first.publish(root.path(), first_marker).unwrap();
    let reader = select(root.path(), false).unwrap();
    let first_path = reader.root.clone();
    let second = StagedAssets::new(root.path()).unwrap();
    let second_marker = marker(second.root());
    let second_path = second.root().to_path_buf();
    second.publish(root.path(), second_marker).unwrap();
    assert!(first_path.is_dir());
    assert_eq!(select(root.path(), false).unwrap().root, second_path);
    assert_eq!(reader.marker.files.len(), 1); // leases are not assets
    drop(reader);
    cleanup(root.path()).unwrap();
    assert!(!first_path.exists());
    assert!(second_path.is_dir());
    assert!(select(root.path(), false).is_ok());
}

#[test]
fn rejected_publication_preserves_previous_generation() {
    let root = tempfile::tempdir().unwrap();
    let first = StagedAssets::new(root.path()).unwrap();
    let first_marker = marker(first.root());
    first.publish(root.path(), first_marker).unwrap();
    let before = fs::read(root.path().join(super::super::MARKER_NAME)).unwrap();
    let stage = StagedAssets::new(root.path()).unwrap();
    let stage_path = stage.root().to_path_buf();
    let mut invalid = marker(stage.root());
    invalid.files[0].path = "../escape".to_owned();
    assert!(stage.publish(root.path(), invalid).is_err());
    assert!(!stage_path.exists());
    assert_eq!(
        fs::read(root.path().join(super::super::MARKER_NAME)).unwrap(),
        before
    );
    assert!(select(root.path(), false).is_ok());
}

#[cfg(unix)]
#[test]
fn publication_io_failure_preserves_the_previous_marker_and_assets() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let first = StagedAssets::new(root.path()).unwrap();
    let value = marker(first.root());
    first.publish(root.path(), value).unwrap();
    let previous = fs::read(root.path().join(super::super::MARKER_NAME)).unwrap();
    let next = StagedAssets::new(root.path()).unwrap();
    let path = next.root().to_path_buf();
    let value = marker(next.root());
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o500)).unwrap();
    let privileged = tempfile::NamedTempFile::new_in(root.path()).is_ok();
    let result = if privileged {
        None
    } else {
        Some(next.publish(root.path(), value))
    };
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let Some(result) = result else { return }; // privileged runners bypass mode restrictions
    assert_eq!(result.unwrap_err().error.kind, crate::ErrorKind::Io);
    assert_eq!(
        fs::read(root.path().join(super::super::MARKER_NAME)).unwrap(),
        previous
    );
    assert!(select(root.path(), false).is_ok());
    cleanup(root.path()).unwrap();
    assert!(!path.exists());
}

mod durability;

#[test]
fn invalid_generation_paths_cannot_escape_the_model_directory() {
    let root = tempfile::tempdir().unwrap();
    let mut value = marker(root.path());
    for directory in [
        "../escape",
        "/outside",
        ".mash-assets-../outside",
        "unowned",
    ] {
        value.asset_directory = Some(directory.to_owned());
        assert!(assets::write_marker(root.path(), &value).is_err());
    }
}

#[test]
fn cleanup_removes_creation_and_marker_orphans_but_preserves_unowned_paths() {
    let root = tempfile::tempdir().unwrap();
    let empty = root.path().join(format!("{PREFIX}empty"));
    fs::create_dir(&empty).unwrap();
    let unknown = root.path().join(format!("{PREFIX}unknown"));
    fs::create_dir(&unknown).unwrap();
    fs::write(unknown.join("unowned"), b"preserve").unwrap();
    let temporary = root
        .path()
        .join(format!("{}orphan", assets::MARKER_TEMP_PREFIX));
    fs::write(&temporary, b"unfinished marker").unwrap();
    fs::write(root.path().join("other-file"), b"preserve").unwrap();
    cleanup(root.path()).unwrap();
    assert!(!empty.exists());
    assert!(!temporary.exists());
    assert!(unknown.join("unowned").is_file());
    assert!(root.path().join("other-file").is_file());
}

#[cfg(unix)]
#[test]
fn staging_copies_symlink_targets_without_mutating_the_original_cache() {
    let root = tempfile::tempdir().unwrap();
    install_fixture(root.path());
    let repository = root.path().join(RERANKER_REPOSITORY);
    fs::write(repository.join("blob"), b"model").unwrap();
    std::os::unix::fs::symlink("blob", repository.join("snapshot")).unwrap();
    let mut value = assets::read_marker(root.path(), false).unwrap();
    value.files = assets::inventory(root.path()).unwrap();
    assets::write_marker(root.path(), &value).unwrap();
    let stage = StagedAssets::new(root.path()).unwrap();
    let snapshot = stage.root().join(RERANKER_REPOSITORY).join("snapshot");
    assert!(
        !fs::symlink_metadata(&snapshot)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    fs::write(snapshot, b"changed").unwrap();
    assert_eq!(fs::read(repository.join("snapshot")).unwrap(), b"model");
}

#[test]
#[ignore = "requires MASH_TEST_MODELS_DIR containing an explicit model installation"]
fn real_sdk_install_and_failed_model_load_preserve_working_generations() {
    let source = std::env::var_os("MASH_TEST_MODELS_DIR")
        .map(PathBuf::from)
        .unwrap();
    let selected = select(&source, false).unwrap();
    let root = tempfile::tempdir().unwrap();
    let initial = StagedAssets::new(root.path()).unwrap();
    for file in &selected.marker.files {
        let target = initial.root().join(&file.path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::copy(selected.root.join(&file.path), target).unwrap();
    }
    initial.publish(root.path(), selected.marker).unwrap();
    let models = super::super::Models::new(root.path().to_path_buf());
    // Run the real explicit installation and SDK smoke validation using the
    // isolated, fully populated cache. The supplied model directory is read-only.
    models.install().unwrap();
    let before = fs::read(root.path().join(super::super::MARKER_NAME)).unwrap();
    let active = select(root.path(), false).unwrap();
    let mut working = models.load().unwrap().unwrap();
    let expected = working.embed(&["working installation"]).unwrap();
    let failed = StagedAssets::new(root.path()).unwrap();
    let snapshot = assets::snapshot(failed.root(), super::super::EMBEDDING_REPOSITORY).unwrap();
    let model = fastembed::TextEmbedding::get_model_info(&EMBEDDING_MODEL).unwrap();
    fs::write(snapshot.join(&model.model_file), b"invalid ONNX model").unwrap();
    assert!(super::super::Backend::download_and_load(failed.root()).is_err());
    drop(failed);
    assert_eq!(
        fs::read(root.path().join(super::super::MARKER_NAME)).unwrap(),
        before
    );
    assert_eq!(working.embed(&["working installation"]).unwrap(), expected);
    let mut reopened = models.load().unwrap().unwrap();
    assert_eq!(reopened.embed(&["working installation"]).unwrap(), expected);
    assert_eq!(select(root.path(), false).unwrap().root, active.root);
    // Switching to remote installation preserves complete local assets, even
    // when the configured remote embedding dimensions are different.
    let remote = super::super::remote::Configuration {
        url: "http://127.0.0.1:1/embeddings".to_owned(),
        model: "test-remote".to_owned(),
        dimensions: 1024,
        api: super::super::remote::Api::OpenaiCompatible,
        token_env: None,
        request_dimensions: false,
    };
    super::super::Models::new(root.path().to_path_buf())
        .with_remote(Some(remote))
        .install()
        .unwrap();
    assert!(models.is_installed());
    assert!(active.root.is_dir()); // still leased by an in-flight search
    assert_eq!(working.embed(&["working installation"]).unwrap(), expected);
}

// The child is a real process so kill/release behavior is covered without
// introducing a production fault-injection switch or downloading any models.
#[test]
fn interruption_child() {
    let Some(root) = std::env::var_os("MASH_ASSET_TRANSACTION_TEST_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let phase = std::env::var("MASH_ASSET_TRANSACTION_TEST_PHASE").unwrap_or_default();
    if phase == "reader" {
        let reader = select(&root, false).unwrap();
        signal_and_wait(&root, &reader.root);
    }
    let stage = StagedAssets::new(&root).unwrap();
    fs::write(stage.root().join("partial"), b"interrupted SDK download").unwrap();
    let path = stage.root().to_path_buf();
    match phase.as_str() {
        "kept" => {
            let _ = stage.directory.keep();
        }
        "published" => {
            let value = marker(stage.root());
            stage.publish(&root, value).unwrap();
        }
        _ => signal_and_wait(&root, &path),
    }
    signal_and_wait(&root, &path);
}

fn signal_and_wait(root: &Path, path: &Path) -> ! {
    fs::write(root.join("child-ready.tmp"), path.to_str().unwrap()).unwrap();
    fs::rename(root.join("child-ready.tmp"), root.join("child-ready")).unwrap();
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

struct RunningChild(std::process::Child);

impl Drop for RunningChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start_child(root: &Path, phase: &str) -> (RunningChild, PathBuf) {
    let mut child = RunningChild(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "semantic::models::generations::tests::interruption_child",
                "--nocapture",
            ])
            .env("MASH_ASSET_TRANSACTION_TEST_ROOT", root)
            .env("MASH_ASSET_TRANSACTION_TEST_PHASE", phase)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !root.join("child-ready").exists() && std::time::Instant::now() < deadline {
        if child.0.try_wait().unwrap().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let ready = fs::read_to_string(root.join("child-ready"));
    let path =
        PathBuf::from(ready.expect("child reached transaction boundary before interruption"));
    (child, path)
}

#[test]
fn killed_installers_preserve_the_active_marker_at_each_publication_boundary() {
    for phase in ["staging", "kept", "published"] {
        let root = tempfile::tempdir().unwrap();
        install_fixture(root.path());
        let before = fs::read(root.path().join(super::super::MARKER_NAME)).unwrap();
        let (child, path) = start_child(root.path(), phase);
        drop(child);
        assert!(path.is_dir());
        assert!(select(root.path(), false).is_ok());
        if phase == "published" {
            assert_eq!(select(root.path(), false).unwrap().root, path);
        } else {
            assert_eq!(
                fs::read(root.path().join(super::super::MARKER_NAME)).unwrap(),
                before
            );
        }
        let _next = StagedAssets::new(root.path()).unwrap();
        assert_eq!(path.exists(), phase == "published");
        assert!(select(root.path(), false).is_ok());
    }
}

#[test]
fn cleanup_respects_other_processes_reading_retired_generations() {
    let root = tempfile::tempdir().unwrap();
    let first = StagedAssets::new(root.path()).unwrap();
    let value = marker(first.root());
    first.publish(root.path(), value).unwrap();
    let (child, first_path) = start_child(root.path(), "reader");
    let second = StagedAssets::new(root.path()).unwrap();
    let value = marker(second.root());
    second.publish(root.path(), value).unwrap();
    assert!(first_path.is_dir());
    assert_ne!(select(root.path(), false).unwrap().root, first_path);
    drop(child);
    cleanup(root.path()).unwrap();
    assert!(!first_path.exists());
    assert!(select(root.path(), false).is_ok());
}
