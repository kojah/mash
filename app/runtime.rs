//! Live-process evidence and admission checks for long-lived local runtimes.
//! Replacing an executable prevents new work; already admitted work may finish.
use crate::{AppError, core::Paths};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Mcp,
    IndexWatch,
}

/// What identifies an installed executable. Unix adds the device and inode,
/// so any replacement is seen. Windows has only size and modification time on
/// stable Rust (its file ID API is unstable, and FFI would need `unsafe`): an
/// upgrade written later or with a different size is seen, but a same-size
/// replacement within one timestamp tick would not be.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct Identity {
    bytes: u64,
    modified_ns: u64,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

fn identity(path: &Path) -> Result<Identity, AppError> {
    let metadata = std::fs::metadata(path).map_err(|_| superseded())?;
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|duration| u64::try_from(duration.as_nanos()).ok())
        .ok_or_else(superseded)?;
    Ok(Identity {
        bytes: metadata.len(),
        modified_ns,
        #[cfg(unix)]
        device: metadata.dev(),
        #[cfg(unix)]
        inode: metadata.ino(),
    })
}

fn superseded() -> AppError {
    AppError::configuration(
        "This Mash runtime's executable was replaced or cannot be verified. Reconnect Mash MCP or restart `mash index --watch` before requesting more writes. An already admitted operation may finish.",
    )
}

// Keep the invocation path, including its final symlink: current_exe alone can
// point into an immutable old release after the installed symlink is replaced.
fn executable() -> Result<(PathBuf, Identity), AppError> {
    let argument = std::env::args_os().next().ok_or_else(superseded)?;
    let argument = PathBuf::from(argument);
    let path = if argument.is_absolute() {
        argument
    } else if argument.components().count() > 1 {
        std::env::current_dir()
            .map_err(|_| superseded())?
            .join(argument)
    } else {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|directory| directory.join(&argument))
            .find(|path| path.is_file())
            .ok_or_else(superseded)?
    };
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map_err(|_| superseded())?
            .join(path)
    };
    let running = std::env::current_exe().map_err(|_| superseded())?;
    let running_identity = identity(&running)?;
    if identity(&path)? != running_identity {
        return Err(superseded());
    }
    Ok((path, running_identity))
}

fn directory(paths: &Paths) -> Result<PathBuf, AppError> {
    let database = paths
        .db
        .clone()
        .unwrap_or_else(crate::core::default_database_path);
    let database = if database.is_absolute() {
        database
    } else {
        std::env::current_dir()
            .map_err(|_| superseded())?
            .join(database)
    };
    let database = std::fs::canonicalize(&database).unwrap_or_else(|_| {
        database
            .parent()
            .and_then(|parent| std::fs::canonicalize(parent).ok())
            .zip(database.file_name())
            .map_or_else(|| database.clone(), |(parent, name)| parent.join(name))
    });
    let mut name = database.into_os_string();
    name.push(".runtimes");
    Ok(name.into())
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u32,
    pid: u32,
    kind: Kind,
    build_revision: String,
    /// Older runtime records cannot establish schema compatibility.
    #[serde(default)]
    schema_version: Option<i64>,
    executable: PathBuf,
    identity: Identity,
}

pub struct Lease {
    // Dropped first: the record goes before its lock is released.
    _record: RecordFile,
    // An OS-held lock proves liveness without guessing from a reusable PID.
    // It is a sibling of the record because Windows locks block reading a
    // file: the record itself stays unlocked so any observer can read it.
    _lock: tempfile::NamedTempFile,
    record: Record,
}

/// A lease's readable record, removed with the lease.
struct RecordFile(PathBuf);

impl Drop for RecordFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

impl Lease {
    #[cfg(test)]
    pub(crate) fn fixture(paths: &Paths, executable: &Path) -> Self {
        Self::create(
            &directory(paths).unwrap(),
            executable.to_path_buf(),
            identity(executable).unwrap(),
            Kind::Mcp,
        )
        .unwrap()
    }

    pub fn register(paths: &Paths, kind: Kind) -> Result<Self, AppError> {
        let (executable, identity) = executable()?;
        Self::create(&directory(paths)?, executable, identity, kind)
    }

    fn create(
        directory: &Path,
        executable: PathBuf,
        identity: Identity,
        kind: Kind,
    ) -> Result<Self, AppError> {
        let record = Record {
            version: 1,
            pid: std::process::id(),
            kind,
            build_revision: env!("MASH_BUILD_REVISION").into(),
            schema_version: Some(crate::storage::schema::SCHEMA_VERSION),
            identity,
            executable,
        };
        std::fs::create_dir_all(directory)
            .map_err(|_| AppError::configuration("cannot create Mash runtime directory"))?;
        let lock = tempfile::Builder::new()
            .prefix("runtime-")
            .suffix(".lock")
            .tempfile_in(directory)
            .map_err(|_| AppError::configuration("cannot register Mash runtime"))?;
        lock.as_file()
            .try_lock()
            .map_err(|_| AppError::configuration("cannot lock Mash runtime record"))?;
        let path = lock.path().with_extension("json");
        let bytes = serde_json::to_vec(&record)
            .map_err(|_| AppError::internal("serialize Mash runtime record"))?;
        let mut file = std::fs::File::create_new(&path)
            .map_err(|_| AppError::configuration("cannot register Mash runtime"))?;
        let record_file = RecordFile(path);
        file.write_all(&bytes)
            .and_then(|()| file.flush())
            .map_err(|_| AppError::configuration("cannot write Mash runtime record"))?;
        Ok(Self {
            _record: record_file,
            _lock: lock,
            record,
        })
    }

    pub fn ensure_current(&self) -> Result<(), AppError> {
        if identity(&self.record.executable)? == self.record.identity {
            Ok(())
        } else {
            Err(superseded())
        }
    }

    /// The invocation path whose replacement supersedes this runtime.
    pub fn executable(&self) -> &Path {
        &self.record.executable
    }
}

/// How long a replaced executable must keep one identity before a stopped
/// watcher starts it, so an installer still writing it is not executed.
#[cfg(unix)]
const SETTLE: std::time::Duration = std::time::Duration::from_secs(2);
/// After this long without a settled replacement, the watcher exits instead.
#[cfg(unix)]
const SETTLE_LIMIT: std::time::Duration = std::time::Duration::from_secs(120);

/// A stopped `index --watch` starts the executable that replaced it in its own
/// process: same arguments, environment, working directory and process ID.
/// Its supervisor sees no exit, so the restart needs neither its respawn
/// (which launchd can defer indefinitely) nor a deploy step. The old runtime
/// has already stopped and released its lease; the new image registers as a
/// fresh runtime. Returns only when that is impossible.
#[cfg(unix)]
pub fn restart(executable: &Path, arguments: &[std::ffi::OsString]) -> AppError {
    use std::os::unix::process::CommandExt;
    if !settled(executable, SETTLE, SETTLE_LIMIT) {
        return cannot_restart(executable, "it did not settle");
    }
    let mut command = std::process::Command::new(executable);
    if let Some((first, rest)) = arguments.split_first() {
        command.arg0(first).args(rest);
    }
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let error = command.exec();
    cannot_restart(executable, &error.to_string())
}

/// Windows cannot replace a running process image; its scheduled task starts
/// the replacement instead.
#[cfg(not(unix))]
pub(crate) fn restart(_executable: &Path, _arguments: &[std::ffi::OsString]) -> AppError {
    superseded()
}

#[cfg(unix)]
fn cannot_restart(executable: &Path, reason: &str) -> AppError {
    AppError::configuration(format!(
        "This Mash watcher stopped because its executable was replaced, but it could not start the replacement at {} ({reason}). Restart `mash index --watch` or its service.",
        executable.display()
    ))
}

/// Whether `path` exists with one identity across a `settle` interval within
/// `limit`.
#[cfg(unix)]
fn settled(path: &Path, settle: std::time::Duration, limit: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + limit;
    let mut previous = identity(path).ok();
    loop {
        std::thread::sleep(settle);
        let current = identity(path).ok();
        if current.is_some() && current == previous {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        previous = current;
    }
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Current,
    Superseded,
    DifferentBuild,
    Unverified,
}

#[derive(Debug, Serialize)]
pub struct Process {
    pub pid: u32,
    pub kind: Kind,
    pub build_revision: String,
    pub schema_version: Option<i64>,
    executable: PathBuf,
    pub state: State,
}

#[derive(Debug, Serialize)]
pub struct Inventory {
    observer_revision: &'static str,
    pub processes: Vec<Process>,
    pub complete: bool,
    scope: &'static str,
}

pub fn inspect(paths: &Paths) -> Inventory {
    match directory(paths) {
        Ok(directory) => inspect_directory(&directory),
        Err(_) => Inventory {
            complete: false,
            ..empty_inventory()
        },
    }
}

fn empty_inventory() -> Inventory {
    Inventory {
        observer_revision: env!("MASH_BUILD_REVISION"),
        processes: vec![],
        complete: true,
        scope: "Registered live MCP and index-watch processes for this database only. Releases predating runtime registration and processes using other databases are not inventoried. A fresh probe does not verify existing sessions.",
    }
}

fn inspect_directory(directory: &Path) -> Inventory {
    let mut report = empty_inventory();
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            report.complete = error.kind() == std::io::ErrorKind::NotFound;
            return report;
        }
    };
    for (index, entry) in entries.enumerate() {
        if index >= 256 {
            report.complete = false;
            break;
        }
        let Ok(entry) = entry else {
            report.complete = false;
            continue;
        };
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "lock")
        {
            continue; // Read through its record.
        }
        let record = read_live(&path).ok();
        let Some(record) = record else {
            report.complete = false;
            continue;
        };
        let Some(record) = record else { continue };
        let state = match identity(&record.executable) {
            Ok(current) if current != record.identity => State::Superseded,
            Ok(_) if record.build_revision != env!("MASH_BUILD_REVISION") => State::DifferentBuild,
            Ok(_) => State::Current,
            Err(_) => State::Unverified,
        };
        report.processes.push(Process {
            pid: record.pid,
            kind: record.kind,
            build_revision: record.build_revision,
            schema_version: record.schema_version,
            executable: record.executable,
            state,
        });
    }
    report.processes.sort_by_key(|process| process.pid);
    report
}

fn read_live(path: &Path) -> Result<Option<Record>, ()> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| ())?;
    if !metadata.is_file() || metadata.len() > 16_384 {
        return Err(());
    }
    let mut file = File::open(path).map_err(|_| ())?;
    // The lease holds a sibling `.lock`; records from builds before that
    // split hold the lock on the record itself.
    let lock = path.with_extension("lock");
    let lock = match File::open(&lock) {
        Ok(lock) => lock,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            file.try_clone().map_err(|_| ())?
        }
        Err(_) => return Err(()),
    };
    match lock.try_lock() {
        Ok(()) => return Ok(None), // Abandoned record, not a live process.
        Err(std::fs::TryLockError::WouldBlock) => (),
        Err(std::fs::TryLockError::Error(_)) => return Err(()),
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(16_385)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() > 16_384 {
        return Err(());
    }
    let record: Record = serde_json::from_slice(&bytes).map_err(|_| ())?;
    if record.version != 1 || !record.executable.is_absolute() {
        return Err(());
    }
    Ok(Some(record))
}

#[cfg(test)]
mod tests;
