//! Bounded installation metadata, offline asset discovery and atomic markers.
use super::{
    EMBEDDING_MODEL, MARKER_NAME, RERANKER_MODEL, RERANKER_REPOSITORY, coreml_embedding_enabled,
};
use crate::AppError;
use crate::semantic::COREML_CACHE_DIRECTORY;
use fastembed::TokenizerFiles;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct InstallMarker {
    pub(super) schema: u32,
    pub(super) embedding_model: String,
    pub(super) reranker_model: String,
    pub(super) embedding_dimensions: usize,
    #[serde(default)]
    pub(super) coreml_embedding: bool,
    #[serde(default = "default_embedding_available")]
    pub(super) embedding_available: bool,
    pub(super) files: Vec<InstalledFile>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct InstalledFile {
    pub(super) path: String,
    pub(super) size: u64,
}

pub(super) const MAX_MARKER_BYTES: usize = 1024 * 1024;
const MAX_MARKER_FILES: usize = 4096;
const MAX_REVISION_BYTES: usize = 128;
const MAX_TOKENIZER_BYTES: usize = 16 * 1024 * 1024;
pub(super) const MARKER_TEMP_PREFIX: &str = ".mash-install-";
pub(super) const INSTALL_LOCK_NAME: &str = ".mash-install.lock";

// Metadata must be bounded before deserialization. Model weights themselves
// are intentionally read separately and can be much larger.
fn read_metadata(path: &Path, limit: usize) -> Result<Vec<u8>, AppError> {
    let metadata = fs::metadata(path).map_err(|_| {
        AppError::model("installed model metadata is unavailable; run `mash models install`")
    })?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err(AppError::model(
            "installed model metadata is not a regular file or exceeds its size limit; run `mash models install`",
        ));
    }
    let file = fs::File::open(path).map_err(|_| {
        AppError::model("cannot open installed model metadata; run `mash models install`")
    })?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| {
            AppError::model("cannot read installed model metadata; run `mash models install`")
        })?;
    if bytes.len() > limit {
        return Err(AppError::model(
            "installed model metadata exceeds its size limit; run `mash models install`",
        ));
    }
    Ok(bytes)
}

fn validate_marker(marker: &InstallMarker, remote: bool) -> Result<(), AppError> {
    if (!remote && !marker.embedding_available)
        || marker.schema != 1
        || marker.embedding_model != EMBEDDING_MODEL.to_string()
        || marker.reranker_model != RERANKER_MODEL.to_string()
        || marker.coreml_embedding != coreml_embedding_enabled()
        || !(1..=2048).contains(&marker.embedding_dimensions)
        || (marker.embedding_available && marker.embedding_dimensions != 384)
        || marker.files.is_empty()
        || marker.files.len() > MAX_MARKER_FILES
        || (remote
            && !marker
                .files
                .iter()
                .any(|file| Path::new(&file.path).starts_with(RERANKER_REPOSITORY)))
    {
        return Err(AppError::model(
            "model marker does not match this mash build; run `mash models install`",
        ));
    }
    let mut paths = HashSet::new();
    for file in &marker.files {
        let path = Path::new(&file.path);
        if file.path.is_empty()
            || file.path.len() > 4096
            || file.path.chars().any(char::is_control)
            || path
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
            || !paths.insert(path)
        {
            return Err(AppError::model(
                "model marker contains invalid or duplicate asset paths; run `mash models install`",
            ));
        }
    }
    Ok(())
}

fn default_embedding_available() -> bool {
    true
}

pub(super) fn remote_install_marker(
    root: &Path,
    dimensions: usize,
    files: Vec<InstalledFile>,
) -> Result<InstallMarker, AppError> {
    let local = read_valid_marker(root, false).ok();
    let marker = InstallMarker {
        schema: 1,
        embedding_model: EMBEDDING_MODEL.to_string(),
        reranker_model: RERANKER_MODEL.to_string(),
        // A complete local installation also supports remote inference. Keep
        // its local metadata so returning to local does not require a reinstall.
        embedding_dimensions: local
            .as_ref()
            .map_or(dimensions, |m| m.embedding_dimensions),
        coreml_embedding: coreml_embedding_enabled(),
        embedding_available: local.is_some(),
        files,
    };
    validate_marker(&marker, true)?;
    Ok(marker)
}

pub(super) fn read_valid_marker(root: &Path, remote: bool) -> Result<InstallMarker, AppError> {
    let bytes = read_metadata(&root.join(MARKER_NAME), MAX_MARKER_BYTES)?;
    let marker: InstallMarker = serde_json::from_slice(&bytes)
        .map_err(|error| AppError::model(format!("invalid model marker: {error}")))?;
    validate_marker(&marker, remote)?;
    for file in &marker.files {
        if remote && !Path::new(&file.path).starts_with(RERANKER_REPOSITORY) {
            continue;
        }
        let path = root.join(&file.path);
        let metadata = fs::metadata(path).map_err(|error| {
            AppError::model(format!("installed model asset is unavailable: {error}"))
        })?;
        if !metadata.is_file() || metadata.len() != file.size {
            return Err(AppError::model("installed model assets are incomplete"));
        }
    }
    Ok(marker)
}

pub(crate) fn snapshot(root: &Path, repository: &str) -> Result<PathBuf, AppError> {
    let bytes = read_metadata(&root.join(repository).join("refs/main"), MAX_REVISION_BYTES)?;
    let revision = std::str::from_utf8(&bytes)
        .map_err(|_| AppError::model("installed model revision is invalid"))?;
    let revision = revision.trim();
    if revision.len() != 40 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::model("installed model revision is invalid"));
    }
    let snapshot = root.join(repository).join("snapshots").join(revision);
    if !snapshot.is_dir() {
        return Err(AppError::model("installed model snapshot is unavailable"));
    }
    Ok(snapshot)
}

pub(crate) fn read_model_file(path: &Path) -> Result<Vec<u8>, AppError> {
    fs::read(path).map_err(|error| AppError::model(format!("cannot read installed model: {error}")))
}

pub(crate) fn read_tokenizer_files(snapshot: &Path) -> Result<TokenizerFiles, AppError> {
    Ok(TokenizerFiles {
        tokenizer_file: read_metadata(&snapshot.join("tokenizer.json"), MAX_TOKENIZER_BYTES)?,
        config_file: read_metadata(&snapshot.join("config.json"), MAX_TOKENIZER_BYTES)?,
        special_tokens_map_file: read_metadata(
            &snapshot.join("special_tokens_map.json"),
            MAX_TOKENIZER_BYTES,
        )?,
        tokenizer_config_file: read_metadata(
            &snapshot.join("tokenizer_config.json"),
            MAX_TOKENIZER_BYTES,
        )?,
    })
}

pub(super) fn inventory(root: &Path) -> Result<Vec<InstalledFile>, AppError> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(|error| AppError::model(error.to_string()))?;
        if entry.path() == root.join(MARKER_NAME)
            || entry.path() == root.join(INSTALL_LOCK_NAME)
            || entry.path() == root.join("installed.json.tmp")
            || (entry.depth() == 1
                && entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(MARKER_TEMP_PREFIX))
            || !fs::metadata(entry.path()).is_ok_and(|m| m.is_file())
        {
            continue;
        }
        let path = entry
            .path()
            .strip_prefix(root)
            .map_err(|error| AppError::model(error.to_string()))?;
        if path.starts_with(COREML_CACHE_DIRECTORY) {
            continue;
        }
        let metadata = fs::metadata(entry.path()).map_err(AppError::io)?;
        files.push(InstalledFile {
            path: path.to_string_lossy().into_owned(),
            size: metadata.len(),
        });
        if files.len() > MAX_MARKER_FILES {
            return Err(AppError::model(
                "installed model inventory contains too many files",
            ));
        }
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

pub(super) fn write_marker(root: &Path, marker: &InstallMarker) -> Result<(), AppError> {
    let path = root.join(MARKER_NAME);
    validate_marker(marker, !marker.embedding_available)?;
    let bytes = serde_json::to_vec_pretty(marker).map_err(|error| {
        AppError::internal(format!("failed to serialize model marker: {error}"))
    })?;
    if bytes.len() > MAX_MARKER_BYTES {
        return Err(AppError::model(
            "installed model inventory exceeds its size limit",
        ));
    }
    // Unique staging files keep simultaneous explicit installers from racing
    // over one temporary path. A failure leaves the previous marker intact.
    let mut temporary = tempfile::Builder::new()
        .prefix(MARKER_TEMP_PREFIX)
        .tempfile_in(root)
        .map_err(AppError::io)?;
    temporary.write_all(&bytes).map_err(AppError::io)?;
    temporary.as_file().sync_all().map_err(AppError::io)?;
    temporary
        .persist(path)
        .map_err(|error| AppError::io(error.error))?;
    Ok(())
}

#[cfg(test)]
mod tests;
