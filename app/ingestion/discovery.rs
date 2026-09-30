//! Discover histories, apply age horizons, and verify authoritative scans.
use super::{IndexOptions, ParsedFile, SourceIdentity};
use crate::AppError;
use crate::provider::Provider;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

pub(super) struct Discovery {
    pub(super) files: Vec<PathBuf>,
    pub(super) complete: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SourceStamp {
    pub(crate) size_bytes: i64,
    pub(crate) modified_ns: i64,
}

impl SourceStamp {
    pub(crate) fn verify(self, path: &Path) -> Result<(), AppError> {
        if source_stamp(path)? != self {
            return Err(AppError::configuration(
                "history changed while indexing; retry index",
            ));
        }
        Ok(())
    }
}

pub(crate) fn source_stamp(path: &Path) -> Result<SourceStamp, AppError> {
    let metadata = path.metadata().map_err(AppError::io)?;
    let size_bytes = i64::try_from(metadata.len())
        .map_err(|_| AppError::internal("source file is too large"))?;
    let modified_ns = metadata
        .modified()
        .map_err(AppError::io)?
        .duration_since(UNIX_EPOCH)
        .map_err(|_| AppError::internal("source modification time predates Unix epoch"))?
        .as_nanos();
    let modified_ns = i64::try_from(modified_ns)
        .map_err(|_| AppError::internal("source modification time is out of range"))?;
    Ok(SourceStamp {
        size_bytes,
        modified_ns,
    })
}

pub(super) fn verify_authoritative_discovery(
    provider: Provider,
    roots: &[PathBuf],
    observed_paths: &BTreeSet<String>,
) -> Result<(), AppError> {
    let final_paths = discover_history_files(provider, roots, true)?
        .files
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<BTreeSet<_>>();
    if final_paths != *observed_paths {
        return Err(AppError::configuration(
            "configured provider roots changed while indexing",
        ));
    }
    Ok(())
}

pub(super) fn discover_history_files(
    provider: Provider,
    roots: &[PathBuf],
    authoritative: bool,
) -> Result<Discovery, AppError> {
    discover_files(roots, authoritative, |path, explicit| match provider {
        Provider::Aider => {
            if explicit {
                path.extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
            } else {
                path.file_name()
                    .is_some_and(|name| name == ".aider.chat.history.md")
            }
        }
        Provider::ClaudeCode | Provider::Codex => is_jsonl(path),
    })
}

fn discover_files(
    roots: &[PathBuf],
    authoritative: bool,
    accept: impl Fn(&Path, bool) -> bool,
) -> Result<Discovery, AppError> {
    let mut files = BTreeSet::new();
    let mut complete = true;
    for root in roots {
        let metadata = match root.metadata() {
            Ok(metadata) => metadata,
            Err(error) if authoritative => {
                return Err(AppError::configuration(format!(
                    "configured provider root is not accessible: {}: {error}",
                    root.display()
                )));
            }
            Err(_) => {
                complete = false;
                continue;
            }
        };
        if metadata.is_file() {
            if accept(root, true) {
                files.insert(root.clone());
            } else if authoritative {
                return Err(AppError::configuration(format!(
                    "configured provider file is not a supported history: {}",
                    root.display()
                )));
            } else {
                complete = false;
            }
            continue;
        }
        if !metadata.is_dir() {
            if authoritative {
                return Err(AppError::configuration(format!(
                    "configured provider root is not a file or directory: {}",
                    root.display()
                )));
            }
            complete = false;
            continue;
        }
        for entry in WalkDir::new(root).follow_links(false) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) if authoritative => {
                    return Err(AppError::configuration(format!(
                        "failed to inspect configured provider root {}: {error}",
                        root.display()
                    )));
                }
                Err(_) => {
                    complete = false;
                    continue;
                }
            };
            let path = entry.path();
            if entry.file_type().is_file() && accept(path, false) {
                files.insert(path.to_path_buf());
            }
        }
    }
    Ok(Discovery {
        files: files.into_iter().collect(),
        complete,
    })
}

pub(super) fn cutoff_modified_ns(
    run_started: SystemTime,
    since_days: Option<u32>,
) -> Result<Option<i64>, AppError> {
    let Some(days) = since_days else {
        return Ok(None);
    };
    let seconds = u64::from(days)
        .checked_mul(24 * 60 * 60)
        .ok_or_else(|| AppError::usage("--since-days is out of range"))?;
    let cutoff = run_started
        .checked_sub(Duration::from_secs(seconds))
        .unwrap_or(UNIX_EPOCH);
    let nanoseconds = cutoff
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let nanoseconds = i64::try_from(nanoseconds)
        .map_err(|_| AppError::internal("index cutoff is out of range"))?;
    Ok(Some(nanoseconds))
}

// Shared routing reuses discovery and concrete parsers without opening the local index.
#[derive(Clone)]
pub(crate) struct DirectScan {
    pub(crate) provider: Provider,
    roots: Vec<PathBuf>,
    pub(crate) sources: Vec<SourceIdentity>,
    pub(crate) complete: bool,
    authoritative: bool,
    pub(crate) cutoff: Option<i64>,
}

impl DirectScan {
    pub(crate) fn parse_source(
        provider: Provider,
        source: &SourceIdentity,
        stamp: SourceStamp,
    ) -> Result<ParsedFile, AppError> {
        super::parse_stable_source(&source.path, stamp, provider.parser())
    }

    pub(crate) fn verify(&self) -> Result<(), AppError> {
        if self.authoritative {
            let observed = self
                .sources
                .iter()
                .map(|source| source.path.to_string_lossy().into_owned())
                .collect();
            verify_authoritative_discovery(self.provider, &self.roots, &observed)?;
        }
        Ok(())
    }
}

pub(crate) fn direct_scans(options: &IndexOptions) -> Result<Vec<DirectScan>, AppError> {
    let cutoff = cutoff_modified_ns(SystemTime::now(), options.since_days)?;
    options
        .providers
        .iter()
        .map(|(&provider, roots)| {
            let discovery =
                discover_history_files(provider, roots, options.roots_are_authoritative)?;
            Ok(DirectScan {
                provider,
                roots: roots.clone(),
                sources: discovery
                    .files
                    .into_iter()
                    .map(SourceIdentity::file)
                    .collect(),
                complete: discovery.complete,
                authoritative: options.roots_are_authoritative,
                cutoff,
            })
        })
        .collect()
}

fn is_jsonl(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
}
