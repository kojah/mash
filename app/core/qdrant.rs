//! `mash qdrant install`: the one explicit path that downloads Qdrant. It
//! installs a pinned, checksum-verified release for this platform under
//! Mash's data directory and runs it under the user's own supervisor, bound
//! to 127.0.0.1 on the configured ports. Rerunning repairs or upgrades it
//! without touching its storage; `mash qdrant uninstall` removes it.
use crate::AppError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

mod probe;
mod release;
mod supervisor;
#[cfg(test)]
mod tests;

pub(crate) use release::Platform;
pub(crate) use supervisor::{ServiceState, Supervisor};

/// The installed record, kept beside the binaries in strict TOML. It names
/// the service that was registered so status and uninstall find it again.
const MANIFEST: &str = "install.toml";
const QDRANT_CONFIG: &str = "config.yaml";
const LOCK: &str = "install.lock";
const STAGING_PREFIX: &str = ".download-";

/// The default service names; `MASH_TEST_QDRANT_SERVICE` replaces them so a
/// real end-to-end check can register a throwaway service.
const SYSTEMD_UNIT: &str = "mash-qdrant";
const LAUNCHD_LABEL: &str = "dev.mash.qdrant";
const SCHEDULED_TASK: &str = "MashQdrant";

/// What one `mash qdrant install` run did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InstallOutcome {
    /// Downloaded, verified, registered and started for the first time.
    Installed,
    /// Replaced an earlier Qdrant version with the pinned one.
    Upgraded,
    /// Rewrote the service definition, restored the binary or restarted it.
    Repaired,
    /// Already installed, current and running; nothing changed.
    Current,
    /// A compatible Qdrant that Mash did not install already answers on the
    /// configured ports, so nothing was downloaded or registered.
    AlreadyRunning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UninstallOutcome {
    Removed,
    NotInstalled,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct InstallResponse {
    pub(crate) outcome: InstallOutcome,
    /// The Qdrant version now answering on the configured ports.
    pub(crate) version: String,
    pub(crate) url: String,
    pub(crate) grpc_url: String,
    /// Bytes downloaded by this run; zero when the pinned binary was present.
    pub(crate) downloaded_bytes: u64,
    /// The installed service; absent when another Qdrant already answers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) service: Option<Managed>,
    pub(crate) recommended_action: crate::Action,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct UninstallResponse {
    pub(crate) outcome: UninstallOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) service: Option<String>,
    /// Paths removed by this run.
    pub(crate) removed: Vec<PathBuf>,
    /// Qdrant's data, kept unless `--delete-storage` was given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) kept_storage: Option<PathBuf>,
}

pub(crate) use crate::views::qdrant::service::Managed;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: String,
    asset: String,
    supervisor: Supervisor,
    service: String,
    definition: PathBuf,
    binary: PathBuf,
    storage: PathBuf,
    http_port: u16,
    grpc_port: u16,
}

/// The loopback ports Qdrant serves, taken from Mash's `[qdrant]` settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ports {
    pub(crate) http: u16,
    pub(crate) grpc: u16,
}

impl Ports {
    /// Only a plain-HTTP loopback endpoint without an API key describes a
    /// service this command can provide; any other one belongs to the user.
    pub(crate) fn from_config(config: &crate::config::QdrantConfig) -> Result<Self, AppError> {
        if config.api_key_env.is_some() {
            return Err(AppError::configuration(
                "the [qdrant] configuration names an API key, so it describes a Qdrant you run yourself; `mash qdrant install` provides a local one without a key. Remove `api_key_env` to use it, or keep running your own service",
            ));
        }
        let port = |url: &str| {
            let parsed = crate::views::qdrant::endpoint(url)?;
            let loopback = matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
            if parsed.scheme() != "http" || !loopback {
                return Err(AppError::configuration(format!(
                    "the configured Qdrant {url} is not a plain-HTTP service on this machine; `mash qdrant install` runs Qdrant on 127.0.0.1. Point [qdrant] url and grpc_url at 127.0.0.1 to use it, or keep using that service"
                )));
            }
            parsed
                .port_or_known_default()
                .ok_or_else(|| AppError::configuration("invalid Qdrant port"))
        };
        let ports = Self {
            http: port(&config.url)?,
            grpc: port(&config.grpc_url)?,
        };
        if ports.http == ports.grpc {
            return Err(AppError::configuration(
                "Qdrant's HTTP and gRPC endpoints must use different ports",
            ));
        }
        Ok(ports)
    }

    fn http_url(self) -> String {
        format!("http://127.0.0.1:{}", self.http)
    }

    fn grpc_url(self) -> String {
        format!("http://127.0.0.1:{}", self.grpc)
    }
}

/// Where the installation lives on one machine. Everything is derived from
/// the user's directories, so a disposable `HOME` isolates a test run.
#[derive(Debug, Clone)]
pub(crate) struct Layout {
    pub(crate) platform: Platform,
    /// Binaries, Qdrant's configuration, the manifest and the lock.
    pub(crate) root: PathBuf,
    /// Qdrant's `storage` and `snapshots`. Windows needs a short path.
    pub(crate) data: PathBuf,
    /// The directory holding the supervisor's definition file.
    pub(crate) definitions: PathBuf,
    pub(crate) service: String,
    pub(crate) account: Account,
}

/// The Windows account details a logon task and its data folder need.
#[derive(Debug, Clone, Default)]
pub(crate) struct Account {
    /// `DOMAIN\user`, or empty when unknown.
    pub(crate) user: String,
    pub(crate) system_drive: String,
    pub(crate) system_root: String,
}

impl Account {
    fn current() -> Self {
        let variable = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
        let user = match (variable("USERDOMAIN"), variable("USERNAME")) {
            (Some(domain), Some(name)) => format!("{domain}\\{name}"),
            (None, Some(name)) => name,
            (_, None) => String::new(),
        };
        Self {
            user,
            system_drive: variable("SystemDrive").unwrap_or_else(|| "C:".into()),
            system_root: variable("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()),
        }
    }
}

impl Layout {
    /// The host's layout, or `None` on a platform without a Qdrant release.
    pub(crate) fn host() -> Result<Self, AppError> {
        let platform = Platform::host().ok_or_else(|| {
            install_error(format!(
                "Qdrant publishes no release for this platform ({} {}); run Qdrant yourself, for example with Docker, as the installation guide describes",
                std::env::consts::OS,
                std::env::consts::ARCH
            ))
        })?;
        let directories = directories::BaseDirs::new()
            .ok_or_else(|| install_error("could not determine the home directory"))?;
        let root = crate::platform::mash_home()
            .ok_or_else(|| install_error("could not determine Mash's data directory"))?
            .join("qdrant");
        let service = std::env::var("MASH_TEST_QDRANT_SERVICE").ok();
        Ok(Self::for_platform(
            platform,
            root,
            directories.home_dir(),
            directories.config_dir(),
            service,
            Account::current(),
        ))
    }

    fn for_platform(
        platform: Platform,
        root: PathBuf,
        home: &Path,
        config: &Path,
        service: Option<String>,
        account: Account,
    ) -> Self {
        let supervisor = platform.supervisor();
        let (definitions, data, default) = match supervisor {
            Supervisor::SystemdUser => (config.join("systemd/user"), root.clone(), SYSTEMD_UNIT),
            Supervisor::LaunchAgent => (
                home.join("Library/LaunchAgents"),
                root.clone(),
                LAUNCHD_LABEL,
            ),
            // Qdrant 1.20 cannot open its payload indexes for Mash's collection
            // names beneath a profile-length path on Windows (no long-path
            // support), so data goes to the short folder the guide used.
            Supervisor::ScheduledTask => (
                root.clone(),
                PathBuf::from(format!("{}\\mash-qdrant", account.system_drive)),
                SCHEDULED_TASK,
            ),
        };
        let mut service = service.unwrap_or_else(|| default.to_owned());
        if supervisor == Supervisor::SystemdUser && !service.ends_with(".service") {
            service.push_str(".service");
        }
        Self {
            platform,
            root,
            data,
            definitions,
            service,
            account,
        }
    }

    fn binary(&self, version: &str) -> PathBuf {
        self.root
            .join("bin")
            .join(version)
            .join(self.platform.executable())
    }

    fn storage(&self) -> PathBuf {
        self.data.join("storage")
    }

    fn snapshots(&self) -> PathBuf {
        self.data.join("snapshots")
    }

    fn qdrant_config(&self) -> PathBuf {
        self.root.join(QDRANT_CONFIG)
    }

    fn log(&self) -> PathBuf {
        self.root.join("qdrant.log")
    }

    fn definition(&self) -> PathBuf {
        let name = match self.platform.supervisor() {
            Supervisor::SystemdUser => self.service.clone(),
            Supervisor::LaunchAgent => format!("{}.plist", self.service),
            Supervisor::ScheduledTask => format!("{}.xml", self.service),
        };
        self.definitions.join(name)
    }

    fn logs(&self) -> String {
        match self.platform.supervisor() {
            Supervisor::SystemdUser => format!("journalctl --user --unit {}", self.service),
            Supervisor::LaunchAgent | Supervisor::ScheduledTask => self.log().display().to_string(),
        }
    }
}

/// Everything an installation run depends on outside the filesystem: where
/// releases come from and how supervisor commands run. Tests replace both.
pub(crate) struct Host {
    pub(crate) layout: Layout,
    pub(crate) source: release::Source,
    pub(crate) control: supervisor::Control,
    /// How long a started service may take to answer.
    pub(crate) ready_within: std::time::Duration,
}

impl Host {
    pub(crate) fn system() -> Result<Self, AppError> {
        let layout = Layout::host()?;
        Ok(Self {
            source: release::Source::github(layout.platform),
            layout,
            control: supervisor::Control::System,
            ready_within: std::time::Duration::from_secs(120),
        })
    }
}

/// Install, repair or upgrade the pinned Qdrant and leave it answering.
pub(crate) async fn install(
    host: &Host,
    config: &crate::config::QdrantConfig,
    configured: bool,
) -> Result<InstallResponse, AppError> {
    let ports = Ports::from_config(config)?;
    let layout = &host.layout;
    let _lock = lock(&layout.root)?;
    clean_staging(&layout.root)?;
    let previous = read_manifest(&layout.root)?;
    let supervisor = layout.platform.supervisor();
    let next = if configured {
        crate::Action::Index
    } else {
        crate::Action::Init
    };
    let (state, foreign) = claim_ports(host, previous.as_ref(), ports).await?;
    if let Some(version) = foreign {
        return Ok(InstallResponse {
            outcome: InstallOutcome::AlreadyRunning,
            version,
            url: ports.http_url(),
            grpc_url: ports.grpc_url(),
            downloaded_bytes: 0,
            service: None,
            recommended_action: next,
        });
    }
    let version = release::VERSION;
    let binary = layout.binary(version);
    // The versioned binary only ever arrives by renaming a verified download
    // into place, so one that is present is the pinned release.
    let staged = if binary.is_file() {
        None
    } else {
        Some(host.source.fetch(&layout.root).await?)
    };
    let downloaded_bytes = staged.as_ref().map_or(0, |staged| staged.bytes);
    let qdrant_config = supervisor::qdrant_config(layout, ports);
    let definition = supervisor::definition(layout, &binary)?;
    let definition_changed = read_text(&layout.definition())?.as_ref() != Some(&definition);
    let config_changed = read_text(&layout.qdrant_config())?.as_ref() != Some(&qdrant_config);
    let changed = staged.is_some() || definition_changed || config_changed;
    if changed && state.is_active() {
        host.control.stop(layout).await?;
    }
    if let Some(staged) = staged {
        staged.place(&binary)?;
    }
    for directory in [&layout.data, &layout.storage(), &layout.snapshots()] {
        create_private_directory(directory)?;
    }
    // Unchanged files are left alone: systemd warns about a rewritten unit.
    if config_changed {
        write_atomically(&layout.qdrant_config(), qdrant_config.as_bytes())?;
    }
    if definition_changed {
        create_private_directory(&layout.definitions)?;
        write_atomically(&layout.definition(), definition.as_bytes())?;
    }
    if changed || state != ServiceState::Running {
        host.control.start(layout).await?;
    }
    let answered = probe::ready(ports, host.ready_within)
        .await
        .map_err(|error| {
            install_error(format!(
                "{} after it was started; read its output with `{}`",
                error.error.message,
                layout.logs()
            ))
        })?;
    let manifest = Manifest {
        version: version.to_owned(),
        asset: host.source.asset_name().to_owned(),
        supervisor,
        service: layout.service.clone(),
        definition: layout.definition(),
        binary: binary.clone(),
        storage: layout.storage(),
        http_port: ports.http,
        grpc_port: ports.grpc,
    };
    write_atomically(
        &layout.root.join(MANIFEST),
        toml::to_string(&manifest)
            .map_err(|error| AppError::internal(format!("manifest: {error}")))?
            .as_bytes(),
    )?;
    remove_other_versions(layout, version)?;
    let outcome = match &previous {
        None if downloaded_bytes > 0 => InstallOutcome::Installed,
        Some(previous) if previous.version != version => InstallOutcome::Upgraded,
        _ if changed || state != ServiceState::Running => InstallOutcome::Repaired,
        _ => InstallOutcome::Current,
    };
    Ok(InstallResponse {
        outcome,
        version: answered,
        url: ports.http_url(),
        grpc_url: ports.grpc_url(),
        downloaded_bytes,
        service: Some(managed(layout, &manifest, ServiceState::Running)),
        recommended_action: next,
    })
}

/// The installed service's state once the configured ports are free for it,
/// or the version of a compatible Qdrant that Mash did not install already
/// serving them. Mash's own service on these ports needs no probe; on other
/// ports it is moving to these, so it stops before they are checked.
async fn claim_ports(
    host: &Host,
    previous: Option<&Manifest>,
    ports: Ports,
) -> Result<(ServiceState, Option<String>), AppError> {
    let layout = &host.layout;
    let mut state = if previous.is_some() || layout.definition().is_file() {
        host.control.state(layout).await?
    } else {
        ServiceState::NotInstalled
    };
    let serving = state.is_active()
        && previous.is_some_and(|previous| {
            (previous.http_port, previous.grpc_port) == (ports.http, ports.grpc)
        });
    if serving {
        return Ok((state, None));
    }
    if state.is_active() {
        host.control.stop(layout).await?;
        state = ServiceState::Stopped;
    }
    Ok(match probe::ports(ports).await? {
        probe::Ports::Free => (state, None),
        probe::Ports::Qdrant { version } => (state, Some(version)),
    })
}

/// Stop and unregister the installed service and remove its binaries.
/// Storage stays unless `delete_storage` asks for it.
pub(crate) async fn uninstall(
    host: &Host,
    delete_storage: bool,
) -> Result<UninstallResponse, AppError> {
    let layout = &host.layout;
    if !layout.root.exists() && !layout.definition().exists() {
        return Ok(UninstallResponse {
            outcome: UninstallOutcome::NotInstalled,
            service: None,
            removed: vec![],
            kept_storage: None,
        });
    }
    let lock = lock(&layout.root)?;
    let manifest = read_manifest(&layout.root)?;
    // The manifest names what was registered, whatever the defaults are now.
    let mut layout = layout.clone();
    let mut definition = layout.definition();
    if let Some(manifest) = &manifest {
        layout.service.clone_from(&manifest.service);
        definition.clone_from(&manifest.definition);
    }
    let mut removed = vec![];
    let registered = definition.is_file() || manifest.is_some();
    if registered {
        host.control.remove(&layout).await?;
        remove_path(&definition)?;
        removed.push(definition);
    }
    for path in [
        layout.root.join("bin"),
        layout.qdrant_config(),
        layout.root.join(MANIFEST),
        layout.log(),
        layout.root.join(".qdrant-initialized"),
    ] {
        if remove_path(&path)? {
            removed.push(path);
        }
    }
    let kept_storage = if delete_storage {
        for path in [layout.storage(), layout.snapshots()] {
            if remove_path(&path)? {
                removed.push(path);
            }
        }
        None
    } else {
        layout.storage().exists().then(|| layout.storage())
    };
    // Leave no empty folder behind; anything else in it stays.
    drop(lock);
    if std::fs::read_dir(&layout.root)
        .map_err(AppError::io)?
        .all(|entry| entry.is_ok_and(|entry| entry.file_name() == LOCK))
    {
        remove_path(&layout.root)?;
    }
    Ok(UninstallResponse {
        outcome: if registered || !removed.is_empty() {
            UninstallOutcome::Removed
        } else {
            UninstallOutcome::NotInstalled
        },
        service: registered.then(|| layout.service.clone()),
        removed,
        kept_storage,
    })
}

/// The installed service for `mash status`, or `None` when Mash installed
/// none. Never downloads, starts or changes anything.
pub(crate) async fn report() -> Option<Managed> {
    let host = Host::system().ok()?;
    report_on(&host).await
}

async fn report_on(host: &Host) -> Option<Managed> {
    let manifest = read_manifest(&host.layout.root).ok()??;
    let mut layout = host.layout.clone();
    layout.service.clone_from(&manifest.service);
    let state = host
        .control
        .state(&layout)
        .await
        .unwrap_or(ServiceState::Unknown);
    Some(managed(&layout, &manifest, state))
}

fn managed(layout: &Layout, manifest: &Manifest, state: ServiceState) -> Managed {
    Managed {
        version: manifest.version.clone(),
        state,
        supervisor: manifest.supervisor,
        service: manifest.service.clone(),
        definition: manifest.definition.clone(),
        binary: manifest.binary.clone(),
        storage: manifest.storage.clone(),
        logs: layout.logs(),
    }
}

pub(crate) fn install_error(message: impl Into<String>) -> AppError {
    AppError::new(14, crate::ErrorKind::QdrantInstall, message, false)
}

fn read_manifest(root: &Path) -> Result<Option<Manifest>, AppError> {
    let Some(text) = read_text(&root.join(MANIFEST))? else {
        return Ok(None);
    };
    toml::from_str(&text).map(Some).map_err(|error| {
        install_error(format!(
            "{} is not a valid installation record ({error}); remove it and rerun `mash qdrant install`",
            root.join(MANIFEST).display()
        ))
    })
}

fn read_text(path: &Path) -> Result<Option<String>, AppError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(AppError::io(error)),
    }
}

/// One installer at a time; a second one stops instead of racing the first.
fn lock(root: &Path) -> Result<std::fs::File, AppError> {
    create_private_directory(root)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(root.join(LOCK))
        .map_err(AppError::io)?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(std::fs::TryLockError::WouldBlock) => Err(AppError::new(
            6,
            crate::ErrorKind::Conflict,
            "another `mash qdrant` command is running; retry after it finishes",
            true,
        )),
        Err(std::fs::TryLockError::Error(error)) => Err(AppError::io(error)),
    }
}

/// A download interrupted by a killed process leaves its staging folder;
/// the lock guarantees no other installer still owns one.
fn clean_staging(root: &Path) -> Result<(), AppError> {
    for entry in std::fs::read_dir(root).map_err(AppError::io)? {
        let entry = entry.map_err(AppError::io)?;
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(STAGING_PREFIX))
        {
            std::fs::remove_dir_all(entry.path()).map_err(AppError::io)?;
        }
    }
    Ok(())
}

fn remove_other_versions(layout: &Layout, keep: &str) -> Result<(), AppError> {
    let bin = layout.root.join("bin");
    for entry in std::fs::read_dir(&bin).map_err(AppError::io)? {
        let entry = entry.map_err(AppError::io)?;
        if entry.file_name() != keep {
            remove_path(&entry.path())?;
        }
    }
    Ok(())
}

fn remove_path(path: &Path) -> Result<bool, AppError> {
    let result = match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => Err(error),
    };
    result.map(|()| true).map_err(AppError::io)
}

fn create_private_directory(path: &Path) -> Result<(), AppError> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    crate::platform::private_directory(&mut builder);
    builder.create(path).map_err(AppError::io)
}

/// Replace `path` whole, so a reader never sees a partly written file.
fn write_atomically(path: &Path, contents: &[u8]) -> Result<(), AppError> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| AppError::internal("file without a parent directory"))?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(AppError::io)?;
    file.write_all(contents).map_err(AppError::io)?;
    file.as_file().sync_all().map_err(AppError::io)?;
    file.persist(path)
        .map_err(|error| AppError::io(error.error))?;
    crate::platform::sync_directory(parent).map_err(AppError::io)
}
