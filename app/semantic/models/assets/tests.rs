use super::*;
use crate::semantic::models::Models;
use crate::semantic::{EMBEDDING_REPOSITORY, remote};

fn fixture_marker(root: &Path) -> InstallMarker {
    fs::write(root.join("asset"), b"fixture").unwrap();
    InstallMarker {
        schema: 1,
        asset_directory: None,
        embedding_model: EMBEDDING_MODEL.to_string(),
        reranker_model: RERANKER_MODEL.to_string(),
        embedding_dimensions: 384,
        coreml_embedding: coreml_embedding_enabled(),
        embedding_available: true,
        files: vec![InstalledFile {
            path: "asset".to_owned(),
            size: 7,
        }],
    }
}

fn store_fixture(root: &Path, marker: &InstallMarker) {
    fs::write(root.join(MARKER_NAME), serde_json::to_vec(marker).unwrap()).unwrap();
}

#[test]
fn remote_install_preserves_valid_local_readiness_without_claiming_missing_embeddings() {
    #[derive(Debug, PartialEq, Eq)]
    enum State {
        Complete,
        MissingMarker,
        MissingAsset,
        RemoteOnly,
    }
    for state in [
        State::Complete,
        State::MissingMarker,
        State::MissingAsset,
        State::RemoteOnly,
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut previous = fixture_marker(root.path());
        let repository = root.path().join(RERANKER_REPOSITORY);
        fs::create_dir(&repository).unwrap();
        fs::write(repository.join("reranker"), b"fixture").unwrap();
        if state == State::RemoteOnly {
            previous.embedding_available = false;
            previous.embedding_dimensions = 1024;
            previous.files = inventory(root.path()).unwrap();
        }
        if state != State::MissingMarker {
            write_marker(root.path(), &previous).unwrap();
        }
        if state == State::MissingAsset {
            fs::remove_file(root.path().join("asset")).unwrap();
        }
        let marker =
            remote_install_marker(root.path(), 1024, inventory(root.path()).unwrap()).unwrap();
        assert_eq!(
            marker.embedding_available,
            state == State::Complete,
            "{state:?}"
        );
        assert_eq!(
            marker.embedding_dimensions,
            if state == State::Complete { 384 } else { 1024 }
        );
        write_marker(root.path(), &marker).unwrap();
        let local = Models::new(root.path().to_path_buf());
        let remote =
            Models::new(root.path().to_path_buf()).with_remote(Some(remote::Configuration {
                url: "http://127.0.0.1:9/embeddings".to_owned(),
                model: "remote-fixture".to_owned(),
                dimensions: 1024,
                api: remote::Api::OpenaiCompatible,
                token_env: None,
                request_dimensions: false,
                redaction: crate::redaction::Policy::ON,
            }));
        assert_eq!(local.is_installed(), state == State::Complete, "{state:?}");
        assert!(remote.is_installed(), "{state:?}");
        assert_ne!(
            crate::semantic::configured_generation(local.remote.as_ref()),
            crate::semantic::configured_generation(remote.remote.as_ref())
        );
    }
}

#[test]
fn invalid_remote_inventory_leaves_the_existing_marker_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let marker = fixture_marker(root.path());
    write_marker(root.path(), &marker).unwrap();
    let before = fs::read(root.path().join(MARKER_NAME)).unwrap();
    assert!(remote_install_marker(root.path(), 1024, vec![]).is_err());
    assert_eq!(fs::read(root.path().join(MARKER_NAME)).unwrap(), before);
    assert!(Models::new(root.path().to_path_buf()).is_installed());
}

#[test]
#[cfg(unix)]
fn failed_marker_staging_preserves_the_previous_installation() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let marker = fixture_marker(root.path());
    write_marker(root.path(), &marker).unwrap();
    let before = fs::read(root.path().join(MARKER_NAME)).unwrap();
    let original = fs::metadata(root.path()).unwrap().permissions();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o500)).unwrap();
    // Privileged runners may bypass filesystem permissions entirely.
    let enforced = fs::write(root.path().join("probe"), b"probe").is_err();
    let result = write_marker(root.path(), &marker);
    fs::set_permissions(root.path(), original).unwrap();
    if enforced {
        assert!(result.is_err());
    }
    assert_eq!(fs::read(root.path().join(MARKER_NAME)).unwrap(), before);
    assert!(Models::new(root.path().to_path_buf()).is_installed());
    assert!(!fs::read_dir(root.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(MARKER_TEMP_PREFIX)
    }));
}

#[test]
fn model_metadata_is_bounded_before_loading_assets() {
    let root = tempfile::tempdir().unwrap();
    let models = Models::new(root.path().to_path_buf());
    assert!(models.load().unwrap().is_none());
    assert!(models.load_indexer().unwrap().is_none());
    assert!(
        models
            .require_installed()
            .unwrap_err()
            .error
            .message
            .contains("models install")
    );
    fs::write(
        root.path().join(MARKER_NAME),
        vec![b' '; MAX_MARKER_BYTES + 1],
    )
    .unwrap();
    assert!(!models.is_installed());
    assert!(
        models
            .load()
            .err()
            .unwrap()
            .error
            .message
            .contains("size limit")
    );
    assert!(models.load_indexer().is_err());
    fs::remove_file(root.path().join(MARKER_NAME)).unwrap();
    fs::create_dir(root.path().join(MARKER_NAME)).unwrap();
    assert!(models.read_valid_marker().is_err());
}

#[test]
fn model_marker_rejects_paths_outside_the_inventory_and_duplicate_assets() {
    let root = tempfile::tempdir().unwrap();
    let mut marker = fixture_marker(root.path());
    let models = Models::new(root.path().to_path_buf());
    for path in ["", "../asset", "/asset", "asset/../../asset", "asset\n"] {
        marker.files[0].path = path.to_owned();
        store_fixture(root.path(), &marker);
        assert!(
            models
                .read_valid_marker()
                .unwrap_err()
                .error
                .message
                .contains("asset paths"),
            "{path:?}"
        );
    }
    marker.files[0].path = "asset".to_owned();
    marker.files.push(InstalledFile {
        path: "asset".to_owned(),
        size: 7,
    });
    store_fixture(root.path(), &marker);
    assert!(
        models
            .read_valid_marker()
            .unwrap_err()
            .error
            .message
            .contains("duplicate")
    );
}

#[test]
fn model_marker_rejects_wrong_dimensions_and_incomplete_assets() {
    let root = tempfile::tempdir().unwrap();
    let mut marker = fixture_marker(root.path());
    let models = Models::new(root.path().to_path_buf());
    for dimensions in [0, 256, 2049] {
        marker.embedding_dimensions = dimensions;
        store_fixture(root.path(), &marker);
        assert!(!models.is_installed());
    }
    marker.embedding_dimensions = 384;
    store_fixture(root.path(), &marker);
    assert!(models.is_installed());
    fs::write(root.path().join("asset"), b"truncated").unwrap();
    assert!(
        models
            .require_installed()
            .unwrap_err()
            .error
            .message
            .contains("incomplete")
    );
    fs::remove_file(root.path().join("asset")).unwrap();
    assert!(
        models
            .require_installed()
            .unwrap_err()
            .error
            .message
            .contains("unavailable")
    );
}

#[test]
fn remote_model_readiness_requires_the_actual_reranker_repository() {
    let root = tempfile::tempdir().unwrap();
    let mut marker = fixture_marker(root.path());
    marker.embedding_available = false;
    marker.embedding_dimensions = 1024;
    let models = Models::new(root.path().to_path_buf()).with_remote(Some(remote::Configuration {
        url: "http://127.0.0.1:9/embeddings".to_owned(),
        model: "remote-fixture".to_owned(),
        dimensions: 1024,
        api: remote::Api::OpenaiCompatible,
        token_env: None,
        request_dimensions: false,
        redaction: crate::redaction::Policy::ON,
    }));
    marker.files[0].path = format!("unrelated-{RERANKER_REPOSITORY}/asset");
    store_fixture(root.path(), &marker);
    assert!(!models.is_installed());
    let repository = root.path().join(RERANKER_REPOSITORY);
    fs::create_dir(&repository).unwrap();
    fs::write(repository.join("asset"), b"fixture").unwrap();
    marker.files[0].path = format!("{RERANKER_REPOSITORY}/asset");
    store_fixture(root.path(), &marker);
    assert!(models.is_installed());
    assert!(!Models::new(root.path().to_path_buf()).is_installed());
}

#[test]
fn bounded_revision_metadata_preserves_valid_snapshot_resolution() {
    let root = tempfile::tempdir().unwrap();
    let repository = root.path().join(EMBEDDING_REPOSITORY);
    fs::create_dir_all(repository.join("refs")).unwrap();
    let revision_path = repository.join("refs/main");
    for revision in [
        b"../escape".to_vec(),
        vec![b'a'; MAX_REVISION_BYTES + 1],
        vec![0xff; 40],
    ] {
        fs::write(&revision_path, revision).unwrap();
        assert!(snapshot(root.path(), EMBEDDING_REPOSITORY).is_err());
    }
    let revision = "a".repeat(40);
    fs::write(&revision_path, format!("{revision}\n")).unwrap();
    assert!(
        snapshot(root.path(), EMBEDDING_REPOSITORY)
            .unwrap_err()
            .error
            .message
            .contains("snapshot is unavailable")
    );
    let expected = repository.join("snapshots").join(revision);
    fs::create_dir_all(&expected).unwrap();
    assert_eq!(
        snapshot(root.path(), EMBEDDING_REPOSITORY).unwrap(),
        expected
    );
}

#[test]
fn invalid_installation_does_not_replace_previous_marker_or_leave_staging_files() {
    let root = tempfile::tempdir().unwrap();
    let mut marker = fixture_marker(root.path());
    write_marker(root.path(), &marker).unwrap();
    let previous = fs::read(root.path().join(MARKER_NAME)).unwrap();
    marker.embedding_dimensions = 0;
    assert!(write_marker(root.path(), &marker).is_err());
    assert_eq!(fs::read(root.path().join(MARKER_NAME)).unwrap(), previous);
    let staging = tempfile::Builder::new()
        .prefix(MARKER_TEMP_PREFIX)
        .tempfile_in(root.path())
        .unwrap();
    fs::write(root.path().join("installed.json.tmp"), b"obsolete stage").unwrap();
    let files = inventory(root.path()).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "asset");
    drop(staging);
}

#[test]
fn tokenizer_metadata_is_bounded_and_missing_files_are_actionable() {
    let root = tempfile::tempdir().unwrap();
    let error = read_tokenizer_files(root.path()).unwrap_err();
    assert_eq!(error.error.kind, crate::ErrorKind::Model);
    assert_eq!(
        error.error.recommended_action,
        Some(crate::Action::ModelsInstall)
    );
    for name in [
        "tokenizer.json",
        "config.json",
        "special_tokens_map.json",
        "tokenizer_config.json",
    ] {
        fs::write(root.path().join(name), b"{}").unwrap();
    }
    assert_eq!(
        read_tokenizer_files(root.path()).unwrap().tokenizer_file,
        b"{}"
    );
    let file = fs::File::create(root.path().join("tokenizer.json")).unwrap();
    file.set_len(MAX_TOKENIZER_BYTES as u64 + 1).unwrap();
    assert!(
        read_tokenizer_files(root.path())
            .unwrap_err()
            .error
            .message
            .contains("size limit")
    );
}

#[test]
fn marker_asset_count_is_bounded_before_filesystem_checks() {
    let root = tempfile::tempdir().unwrap();
    let mut marker = fixture_marker(root.path());
    marker.files = (0..=MAX_MARKER_FILES)
        .map(|i| InstalledFile {
            path: format!("missing-{i}"),
            size: 1,
        })
        .collect();
    store_fixture(root.path(), &marker);
    let error = Models::new(root.path().to_path_buf())
        .read_valid_marker()
        .unwrap_err();
    assert!(
        error
            .error
            .message
            .contains("does not match this mash build")
    );
}
