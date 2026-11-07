//! Bounded filesystem reads and platform defaults for node configuration.
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use directories::{BaseDirs, ProjectDirs};

use super::document::parse_document;
use super::{DEFAULT_SINCE_DAYS, IndexDestination, ResolvedConfig};
use crate::AppError;
use crate::provider::{Provider, ProviderRoots};

pub(super) const MAX_CONFIG_BYTES: usize = 1024 * 1024;

pub(crate) fn load(explicit_path: Option<&Path>) -> Result<ResolvedConfig, AppError> {
    load_from_paths(default_path(), explicit_path.map(Path::to_path_buf))
}

pub(crate) fn default_path() -> Result<PathBuf, AppError> {
    let path = ProjectDirs::from("dev", "jamesqo", "mash")
        .ok_or_else(|| AppError::configuration("platform configuration directory is unavailable"))?
        .config_dir()
        .join("config.toml");
    if !path.is_absolute() {
        return Err(AppError::configuration(
            "platform configuration path is not absolute",
        ));
    }
    Ok(path)
}

pub(super) fn load_from_paths(
    default_path: Result<PathBuf, AppError>,
    explicit_path: Option<PathBuf>,
) -> Result<ResolvedConfig, AppError> {
    let explicit = explicit_path.is_some();
    let path = match explicit_path {
        Some(path) => path,
        None => default_path?,
    };

    if !explicit && !path.is_absolute() {
        return Err(AppError::configuration(
            "platform configuration path is not absolute",
        ));
    }

    match fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound && !explicit => {
            let providers = built_in_providers()?;
            return Ok(ResolvedConfig {
                qdrant: super::QdrantConfig::default(),
                attachments: crate::attachments::Configuration::default(),
                redaction: crate::redaction::Settings::default(),
                path,
                loaded: false,
                shared: None,
                views: crate::views::config::Configuration::default(),
                providers,
                since_days: Some(DEFAULT_SINCE_DAYS),
                destination: IndexDestination::Local,
            });
        }
        Err(error) => return Err(path_error("inspect", &path, &error)),
    }

    let canonical =
        fs::canonicalize(&path).map_err(|error| path_error("resolve", &path, &error))?;
    let target_metadata =
        fs::metadata(&canonical).map_err(|error| path_error("inspect", &canonical, &error))?;
    if !target_metadata.is_file() {
        return Err(not_regular_file(&canonical));
    }
    let mut file =
        File::open(&canonical).map_err(|error| path_error("open", &canonical, &error))?;
    let metadata = file
        .metadata()
        .map_err(|error| path_error("inspect", &canonical, &error))?;
    if !metadata.is_file() {
        return Err(not_regular_file(&canonical));
    }

    let metadata_size = usize::try_from(metadata.len()).unwrap_or(MAX_CONFIG_BYTES);
    let mut bytes = Vec::with_capacity(MAX_CONFIG_BYTES.min(metadata_size));
    (&mut file)
        .take((MAX_CONFIG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| path_error("read", &canonical, &error))?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(AppError::configuration(format!(
            "configuration exceeds {MAX_CONFIG_BYTES} bytes: {}",
            canonical.display()
        )));
    }

    let mut parsed = parse_document(&bytes)?;
    if let Some(shared) = &mut parsed.shared {
        shared.publication_redaction = parsed.redaction.publication();
    }
    Ok(ResolvedConfig {
        qdrant: parsed.qdrant,
        attachments: parsed.attachments,
        redaction: parsed.redaction,
        path: canonical,
        loaded: true,
        providers: parsed.providers,
        since_days: parsed.since_days,
        shared: parsed.shared,
        views: parsed.views,
        destination: parsed.destination,
    })
}

fn built_in_providers() -> Result<ProviderRoots, AppError> {
    let directories = BaseDirs::new()
        .ok_or_else(|| AppError::configuration("platform home directory is unavailable"))?;
    let home = directories.home_dir();
    if !home.is_absolute() {
        return Err(AppError::configuration(
            "platform home directory is not absolute",
        ));
    }
    Ok(ProviderRoots::from([
        (
            Provider::ClaudeCode,
            vec![
                home.join(".claude/projects"),
                home.join(".config/claude/projects"),
            ],
        ),
        (
            Provider::Codex,
            vec![
                home.join(".codex/sessions"),
                home.join(".codex/thread_history_1.sqlite"),
                home.join(".local/share/codex/sessions"),
                home.join(".local/share/codex/thread_history_1.sqlite"),
            ],
        ),
    ]))
}

fn not_regular_file(path: &Path) -> AppError {
    AppError::configuration(format!(
        "configuration path is not a regular file: {}",
        path.display()
    ))
}

fn path_error(operation: &str, path: &Path, error: &io::Error) -> AppError {
    AppError::configuration(format!(
        "failed to {operation} configuration {}: {error}",
        path.display()
    ))
}
