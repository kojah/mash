//! Independent installed-model fixtures shared by real SDK tests.
use super::{assets, generations};
use std::{fs, path::PathBuf};

pub(super) fn copied_models() -> tempfile::TempDir {
    let source = std::env::var_os("MASH_TEST_MODELS_DIR")
        .map(PathBuf::from)
        .expect("explicitly installed test models");
    let selected = generations::select(&source).unwrap();
    let directory = tempfile::tempdir().unwrap();
    for file in &selected.marker.files {
        let target = directory.path().join(&file.path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::copy(selected.root.join(&file.path), target).unwrap();
    }
    let mut marker = selected.marker;
    marker.asset_directory = None;
    assets::write_marker(directory.path(), &marker).unwrap();
    directory
}
