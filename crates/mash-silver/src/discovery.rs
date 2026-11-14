//! Discover histories, apply age horizons, and verify authoritative scans.
use super::source::Dependency;
use super::{HistorySource, IndexOptions, SourceIdentity};
use crate::AppError;
use crate::provider::Provider;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

pub struct Discovery {
    pub sources: Vec<HistorySource>,
    pub complete: bool,
}

/// Provider-specific physical dependencies belong beside source discovery.
pub fn watch_roots(options: &IndexOptions) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for (&provider, paths) in &options.providers {
        for path in paths {
            if provider == Provider::Codex
                && path
                    .file_name()
                    .is_some_and(|name| name == "thread_history_1.sqlite")
            {
                roots.push(path.with_file_name("state_5.sqlite"));
            }
            if provider == Provider::Copilot
                && path.file_name().is_some_and(|name| name == "state.vscdb")
                && let Some(parent) = path.parent()
            {
                roots.push(parent.into());
            }
            if provider == Provider::Copilot {
                let global = if path.file_name().is_some_and(|name| name == "state.vscdb") {
                    path.parent()
                } else {
                    Some(path.as_path())
                };
                if let Some(global) =
                    global.filter(|p| p.file_name().is_some_and(|n| n == "globalStorage"))
                    && let Some(user) = global.parent()
                {
                    roots.push(user.join("workspaceStorage/no-workspace/chatSessions"));
                }
            }
            roots.push(path.clone());
        }
    }
    roots
}

/// A session's own content summary inside a store shared with other sessions.
/// The files' size and modification time change whenever any session is
/// written, so they cannot say that this one is unchanged.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionContent {
    pub bytes: i64,
    pub token: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceStamp {
    pub size_bytes: i64,
    pub modified_ns: i64,
    pub fingerprint: String,
    pub activity_ns: Option<i64>,
    dependencies: Vec<Dependency>,
    pub prefix: Option<super::prefix::Prefix>,
    pub content: Option<SessionContent>,
}

impl SourceStamp {
    /// The size and fingerprint a checkpoint stores and is matched against.
    /// `verify` still compares the files themselves, so a store that changes
    /// while a session is being read is noticed either way.
    pub fn checkpoint(&self) -> (i64, &str) {
        match &self.content {
            Some(content) => (content.bytes, &content.token),
            None => (self.size_bytes, &self.fingerprint),
        }
    }

    pub fn file(size_bytes: i64, modified_ns: i64) -> Self {
        Self {
            size_bytes,
            modified_ns,
            fingerprint: String::new(),
            activity_ns: None,
            dependencies: Vec::new(),
            prefix: None,
            content: None,
        }
    }

    pub fn verify(&self, source: &SourceIdentity) -> Result<(), AppError> {
        if let Some(prefix) = &self.prefix {
            return prefix.verify(&source.path);
        }
        let mut current = source_stamp_for(source)?;
        current.include_dependencies(&self.dependencies)?;
        if current.size_bytes != self.size_bytes
            || current.modified_ns != self.modified_ns
            || current.fingerprint != self.fingerprint
        {
            return Err(AppError::source_changed());
        }
        Ok(())
    }

    pub fn include_dependencies(&mut self, paths: &[Dependency]) -> Result<(), AppError> {
        if paths.is_empty() {
            return Ok(());
        }
        let mut hash = blake3::Hasher::new();
        hash.update(self.fingerprint.as_bytes());
        for dependency in paths {
            let path = dependency.path();
            let name = path.as_os_str().as_encoded_bytes();
            hash.update(&(name.len() as u64).to_le_bytes());
            hash.update(name);
            // Optional ownership markers must invalidate the source when they
            // appear or disappear, including between parsing and publication.
            if matches!(dependency, Dependency::Optional(_)) {
                let exists = path.try_exists().map_err(AppError::io)?;
                hash.update(b"optional-dependency");
                hash.update(&[u8::from(exists)]);
                if !exists {
                    continue;
                }
            }
            let stamp = source_stamp(path)?;
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

pub fn source_stamp(path: &Path) -> Result<SourceStamp, AppError> {
    let metadata = path.metadata().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            AppError::source_changed()
        } else {
            AppError::io(error)
        }
    })?;
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

pub fn source_stamp_for(source: &SourceIdentity) -> Result<SourceStamp, AppError> {
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
        prefix: None,
        content: None,
    })
}

pub fn verify_authoritative_discovery(
    provider: Provider,
    roots: &[PathBuf],
    observed_layout: &std::collections::BTreeMap<String, Vec<Dependency>>,
) -> Result<(), AppError> {
    let sources = discover_history_files(provider, roots, true)?.sources;
    if source_layout(&sources) != *observed_layout {
        return Err(AppError::configuration(
            "configured provider roots changed while indexing",
        ));
    }
    Ok(())
}

pub fn source_layout(
    sources: &[HistorySource],
) -> std::collections::BTreeMap<String, Vec<Dependency>> {
    sources
        .iter()
        .map(|s| (s.identity.key(), s.dependencies.clone()))
        .collect()
}

#[tracing::instrument(name = "discover_sources", skip_all, fields(provider = provider.as_str(), authoritative))]
pub fn discover_history_files(
    provider: Provider,
    roots: &[PathBuf],
    authoritative: bool,
) -> Result<Discovery, AppError> {
    let mut discovery = discover_physical_sources(provider, roots, authoritative)?;
    if provider == Provider::Codex {
        discovery.sources = super::codex_store::sources(discovery.sources)?;
        return Ok(discovery);
    }
    if provider == Provider::Copilot {
        discovery.sources = super::copilot::sources(discovery.sources)?;
        return Ok(discovery);
    }
    let sessions = match provider {
        Provider::Opencode => super::opencode::discover,
        Provider::Cursor => super::cursor::discover,
        Provider::Antigravity => super::antigravity::discover,
        Provider::ClaudeCode | Provider::Codex | Provider::Copilot => {
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

pub fn validate_output_path(database: &Path, options: &IndexOptions) -> Result<(), AppError> {
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
        Provider::Antigravity => path.extension().is_some_and(|e| e == "db"),
        Provider::Cursor => {
            path.file_name()
                .is_some_and(|n| n == "store.db" || n == "state.vscdb")
                || (explicit && path.extension().is_some_and(|e| e == "db"))
        }
        Provider::ClaudeCode => is_jsonl(path),
        Provider::Codex => {
            is_jsonl(path)
                || path
                    .file_name()
                    .is_some_and(|n| n == "thread_history_1.sqlite")
        }
        Provider::Copilot => {
            path.file_name().is_some_and(|name| name == "events.jsonl")
                || path.file_name().is_some_and(|name| name == "state.vscdb")
                || (explicit && is_jsonl(path))
        }
        Provider::Opencode => {
            path.file_name().is_some_and(|name| name == "opencode.db")
                || (explicit && path.extension().is_some_and(|e| e == "db"))
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

pub fn cutoff_modified_ns(
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
fn is_jsonl(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
}

/// Discover one provider's sources under authoritative roots, for tests.
#[cfg(any(test, feature = "test-support"))]
pub fn discovered_sources(provider: Provider, roots: &[PathBuf]) -> Vec<HistorySource> {
    discover_history_files(provider, roots, true)
        .expect("test discovery")
        .sources
}

/// Parse one discovered source as indexing would, for tests.
#[cfg(any(test, feature = "test-support"))]
pub fn parse_test_source(provider: Provider, source: &HistorySource) -> super::ParsedFile {
    let mut stamp = source.stamp().expect("test source stamp");
    super::parse_stable_source(source, &mut stamp, super::Parser::Native(provider))
        .expect("test parse")
}
