//! Discover histories, apply age horizons, and verify authoritative scans.
use super::{HistorySource, IndexOptions, ParsedFile, SourceIdentity};
use crate::AppError;
use crate::provider::Provider;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

pub(super) struct Discovery {
    pub(super) sources: Vec<HistorySource>,
    pub(super) complete: bool,
}

/// Provider-specific physical dependencies belong beside source discovery.
pub(crate) fn watch_roots(options: &IndexOptions) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for (&provider, paths) in &options.providers {
        for path in paths {
            if provider == Provider::Openclaw {
                if path.extension().is_some_and(|ext| ext == "sqlite") {
                    roots.push(super::openclaw::cold_directory(path));
                } else if path.file_name().is_some_and(|name| name == "agent") {
                    roots.push(super::openclaw::cold_directory(
                        &path.join("openclaw-agent.sqlite"),
                    ));
                }
            }
            roots.push(path.clone());
        }
    }
    roots
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceStamp {
    pub(crate) size_bytes: i64,
    pub(crate) modified_ns: i64,
    pub(crate) fingerprint: String,
    pub(crate) activity_ns: Option<i64>,
    dependencies: Vec<PathBuf>,
}

impl SourceStamp {
    pub(crate) fn file(size_bytes: i64, modified_ns: i64) -> Self {
        Self {
            size_bytes,
            modified_ns,
            fingerprint: String::new(),
            activity_ns: None,
            dependencies: Vec::new(),
        }
    }

    pub(crate) fn verify(&self, source: &SourceIdentity) -> Result<(), AppError> {
        let mut current = source_stamp_for(source)?;
        current.include_dependencies(&self.dependencies)?;
        if current.size_bytes != self.size_bytes
            || current.modified_ns != self.modified_ns
            || current.fingerprint != self.fingerprint
        {
            return Err(AppError::configuration(
                "history changed while indexing; retry index",
            ));
        }
        Ok(())
    }

    pub(super) fn include_dependencies(&mut self, paths: &[PathBuf]) -> Result<(), AppError> {
        if paths.is_empty() {
            return Ok(());
        }
        let mut hash = blake3::Hasher::new();
        hash.update(self.fingerprint.as_bytes());
        for path in paths {
            let stamp = source_stamp(path)?;
            let name = path.as_os_str().as_encoded_bytes();
            hash.update(&(name.len() as u64).to_le_bytes());
            hash.update(name);
            hash.update(&stamp.size_bytes.to_le_bytes());
            hash.update(&stamp.modified_ns.to_le_bytes());
            self.size_bytes = self.size_bytes.saturating_add(stamp.size_bytes);
            self.modified_ns = self.modified_ns.max(stamp.modified_ns);
        }
        self.fingerprint = hash.finalize().to_hex().to_string();
        self.dependencies = paths.to_vec();
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
    Ok(SourceStamp::file(size_bytes, modified_ns))
}

pub(crate) fn source_stamp_for(source: &SourceIdentity) -> Result<SourceStamp, AppError> {
    let main = source_stamp(&source.path)?;
    if source.session.is_none() {
        return Ok(main);
    }
    let mut wal_path = source.path.as_os_str().to_os_string();
    wal_path.push("-wal");
    let wal = if Path::new(&wal_path).try_exists().map_err(AppError::io)? {
        Some(source_stamp(Path::new(&wal_path))?)
    } else {
        None
    };
    let mut hash = blake3::Hasher::new();
    for stamp in [Some(&main), wal.as_ref()] {
        hash.update(&[u8::from(stamp.is_some())]);
        if let Some(stamp) = stamp {
            hash.update(&stamp.size_bytes.to_le_bytes());
            hash.update(&stamp.modified_ns.to_le_bytes());
        }
    }
    Ok(SourceStamp {
        size_bytes: main
            .size_bytes
            .saturating_add(wal.as_ref().map_or(0, |s| s.size_bytes)),
        modified_ns: main
            .modified_ns
            .max(wal.as_ref().map_or(0, |s| s.modified_ns)),
        fingerprint: hash.finalize().to_hex().to_string(),
        activity_ns: None,
        dependencies: Vec::new(),
    })
}

pub(super) fn verify_authoritative_discovery(
    provider: Provider,
    roots: &[PathBuf],
    observed_keys: &BTreeSet<String>,
) -> Result<(), AppError> {
    let final_paths = discover_history_files(provider, roots, true)?
        .sources
        .into_iter()
        .map(|source| source.identity.key())
        .collect::<BTreeSet<_>>();
    if final_paths != *observed_keys {
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
    let mut discovery = discover_physical_sources(provider, roots, authoritative)?;
    let sessions = match provider {
        Provider::Hermes => super::hermes::discover,
        Provider::Opencode => super::opencode::discover,
        Provider::Openclaw => super::openclaw::discover,
        Provider::ClaudeCode | Provider::Codex | Provider::Aider | Provider::Pi => {
            return Ok(discovery);
        }
    };
    discovery.sources = discovery
        .sources
        .into_iter()
        .map(|source| sessions(&source.identity.path))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect();
    Ok(discovery)
}

pub(crate) fn validate_output_path(
    database: &Path,
    options: &IndexOptions,
) -> Result<(), AppError> {
    let output = match database.canonicalize() {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(AppError::io(error)),
    };
    for (&provider, roots) in &options.providers {
        let discovery =
            discover_physical_sources(provider, roots, options.roots_are_authoritative)?;
        for source in discovery.sources {
            if source.identity.path.canonicalize().map_err(AppError::io)? == output {
                return Err(AppError::configuration(
                    "the mash index database must not be a provider history source",
                ));
            }
        }
    }
    Ok(())
}

fn discover_physical_sources(
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
        Provider::ClaudeCode | Provider::Codex | Provider::Pi => is_jsonl(path),
        Provider::Hermes => {
            path.file_name().is_some_and(|name| name == "state.db")
                || (explicit && path.extension().is_some_and(|e| e == "db"))
        }
        Provider::Opencode => {
            path.file_name().is_some_and(|name| name == "opencode.db")
                || (explicit && path.extension().is_some_and(|e| e == "db"))
        }
        Provider::Openclaw => {
            path.file_name()
                .is_some_and(|name| name == "openclaw-agent.sqlite")
                || (explicit && path.extension().is_some_and(|e| e == "sqlite"))
        }
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
        sources: files.into_iter().map(HistorySource::file).collect(),
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
    pub(crate) sources: Vec<HistorySource>,
    pub(crate) complete: bool,
    authoritative: bool,
    pub(crate) cutoff: Option<i64>,
}

impl DirectScan {
    pub(crate) fn parse_source(
        provider: Provider,
        source: &SourceIdentity,
        stamp: &SourceStamp,
    ) -> Result<ParsedFile, AppError> {
        super::parse_stable_source(source, stamp, provider.parser())
    }

    pub(crate) fn verify(&self) -> Result<(), AppError> {
        if self.authoritative {
            let observed = self
                .sources
                .iter()
                .map(|source| source.identity.key())
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
                sources: discovery.sources,
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
