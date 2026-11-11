//! One private, typed inventory for deployment and dashboard service access.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

pub type Commands = Vec<Vec<String>>;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub version: u32,
    pub controller: Controller,
    #[serde(default)]
    pub builds: BTreeMap<Platform, Commands>,
    pub machines: Vec<Machine>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Controller {
    pub node: String,
    pub repository: PathBuf,
    pub binary: PathBuf,
    pub jobs: PathBuf,
    #[serde(default)]
    pub allow_actions: Vec<Action>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Status,
    Preflight,
    Deploy,
    Sync,
    LocalViews,
    ReplayViews,
    WatchStart,
    WatchStop,
    WatchStatus,
    WatchLogs,
}

impl Action {
    #[must_use]
    pub const fn operation(self) -> &'static str {
        match self {
            Self::Status => "check",
            Self::Preflight => "preflight",
            Self::Deploy => "deploy",
            Self::Sync => "sync",
            Self::LocalViews => "local-views",
            Self::ReplayViews => "replay-views",
            Self::WatchStart => "watch-start",
            Self::WatchStop => "watch-stop",
            Self::WatchStatus => "watch-status",
            Self::WatchLogs => "watch-logs",
        }
    }

    #[must_use]
    pub fn from_operation(operation: &str) -> Option<Self> {
        [
            Self::Status,
            Self::Preflight,
            Self::Deploy,
            Self::Sync,
            Self::LocalViews,
            Self::ReplayViews,
            Self::WatchStart,
            Self::WatchStop,
            Self::WatchStatus,
            Self::WatchLogs,
        ]
        .into_iter()
        .find(|action| action.operation() == operation)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Linux,
    Darwin,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Machine {
    pub id: String,
    pub platform: Platform,
    /// Whether this machine indexes agent history. A machine that only hosts
    /// a shared server sets this to false.
    #[serde(default = "enabled")]
    pub history: bool,
    pub dashboard_url: Option<String>,
    pub token_env: Option<String>,
    #[serde(default)]
    pub services: Vec<Service>,
    #[serde(default)]
    pub databases: Vec<Database>,
    pub transport: Transport,
    #[serde(default)]
    pub deployment: Deployment,
    pub watcher: Option<Watcher>,
}

const fn enabled() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Transport {
    Local,
    Ssh {
        host: String,
        user: Option<String>,
        common_args: Option<String>,
        python: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Deployment {
    pub requires: Vec<String>,
    pub shared: bool,
    pub connection_check: Vec<String>,
    pub preflight: Commands,
    pub install: Commands,
    pub mcp: Vec<String>,
    pub node_configs: Vec<PathBuf>,
    pub build_root: Option<PathBuf>,
    pub refresh_all_history: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Watcher {
    pub launcher: PathBuf,
    pub home: PathBuf,
    #[serde(default)]
    pub allow_control: bool,
}

impl Machine {
    /// Service definitions shared with the dashboard, including the fleet-owned watcher.
    #[must_use]
    pub fn managed_services(&self) -> Vec<Service> {
        let mut services = self.services.clone();
        if let Some(watcher) = &self.watcher {
            let (manager, unit, plist, log) = match self.platform {
                Platform::Linux => (Manager::SystemdUser, "mash-index-watch.service", None, None),
                Platform::Darwin => (
                    Manager::Launchd,
                    "dev.mash.index-watch",
                    Some(
                        watcher
                            .home
                            .join("Library/LaunchAgents/dev.mash.index-watch.plist"),
                    ),
                    Some(watcher.home.join("Library/Logs/mash-index-watch.log")),
                ),
            };
            services.push(Service {
                id: "watcher".into(),
                name: "Index watcher".into(),
                kind: ServiceKind::Watcher,
                manager,
                unit: unit.into(),
                plist,
                log,
                allow_control: watcher.allow_control,
                deployment: None,
            });
        }
        services
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Database {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Service {
    pub id: String,
    pub name: String,
    pub kind: ServiceKind,
    pub manager: Manager,
    pub unit: String,
    pub plist: Option<PathBuf>,
    pub log: Option<PathBuf>,
    #[serde(default)]
    pub allow_control: bool,
    /// Present only on a shared server the fleet deploys; see [`ServerDeployment`].
    pub deployment: Option<ServerDeployment>,
}

/// Trusted commands that install and check a shared server. A server service
/// with these is a fleet target named by the service's `id`, reached through
/// the machine that declares it.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerDeployment {
    pub preflight: Commands,
    pub install: Commands,
    pub verify: Commands,
    pub replay_views: Commands,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceKind {
    Watcher,
    Qdrant,
    Server,
    Dashboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Manager {
    SystemdUser,
    SystemdSystem,
    Launchd,
    WindowsService,
    WindowsTask,
}

#[must_use]
pub fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
}

impl Inventory {
    /// Shared servers the fleet deploys, each with the machine that hosts it.
    pub fn servers(&self) -> impl Iterator<Item = (&Machine, &Service, &ServerDeployment)> {
        self.machines.iter().flat_map(|machine| {
            machine.services.iter().filter_map(move |service| {
                service
                    .deployment
                    .as_ref()
                    .map(|deployment| (machine, service, deployment))
            })
        })
    }

    /// Resolve explicit targets or all eligible targets against the same allowlist.
    /// A target is a history machine or a deployable shared server.
    ///
    /// # Errors
    /// Rejects disabled actions, unknown/duplicate targets and invalid action scopes.
    pub fn targets(&self, action: Action, requested: &[String]) -> Result<Vec<String>, String> {
        self.validate()?;
        if !self.controller.allow_actions.contains(&action) {
            return Err("fleet action is not enabled in the controller inventory".into());
        }
        let history = self.machines.iter().filter(|machine| machine.history);
        let eligible: Vec<&str> = match action {
            Action::Status | Action::Preflight | Action::Deploy => self
                .servers()
                .map(|(_, service, _)| service.id.as_str())
                .chain(history.map(|machine| machine.id.as_str()))
                .collect(),
            Action::Sync | Action::LocalViews => {
                history.map(|machine| machine.id.as_str()).collect()
            }
            Action::ReplayViews => self
                .servers()
                .map(|(_, service, _)| service.id.as_str())
                .collect(),
            Action::WatchStart | Action::WatchStop | Action::WatchStatus | Action::WatchLogs => {
                history
                    .filter(|machine| machine.watcher.is_some())
                    .map(|machine| machine.id.as_str())
                    .collect()
            }
        };
        let selected: Vec<String> = if requested.is_empty() {
            eligible.iter().map(|&id| id.to_owned()).collect()
        } else {
            requested.to_vec()
        };
        let unique: HashSet<_> = selected.iter().collect();
        if selected.is_empty()
            || unique.len() != selected.len()
            || selected.len() > 32
            || selected.iter().any(|id| !eligible.contains(&id.as_str()))
        {
            return Err("fleet action requires unique eligible target IDs".into());
        }
        if matches!(action, Action::Deploy | Action::Preflight)
            && self
                .machines
                .iter()
                .filter(|machine| selected.contains(&machine.id))
                .any(|machine| {
                    machine
                        .deployment
                        .requires
                        .iter()
                        .any(|id| !selected.contains(id))
                })
        {
            return Err("include required shared servers in the deployment selection".into());
        }
        Ok(selected)
    }

    /// Read a bounded private inventory without resolving or exposing credentials.
    ///
    /// # Errors
    /// Returns a safe configuration error for unreadable, oversized or invalid input.
    pub fn load(path: &Path) -> Result<Self, String> {
        let mut text = String::new();
        std::fs::File::open(path)
            .and_then(|file| file.take(1_048_577).read_to_string(&mut text))
            .map_err(|_| "cannot read fleet inventory")?;
        if text.len() > 1_048_576 {
            return Err("fleet inventory exceeds 1 MiB".into());
        }
        let inventory: Self = toml::from_str(&text).map_err(|_| "invalid fleet inventory TOML")?;
        inventory.validate()?;
        Ok(inventory)
    }

    /// # Errors
    /// Rejects ambiguous identities, unsafe paths and references to absent machines.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.machines.is_empty()
            || self.machines.len() > 32
            || !self.controller.repository.is_absolute()
            || !self.controller.binary.is_absolute()
            || !self.controller.jobs.is_absolute()
        {
            return Err("invalid fleet version, machine count or controller paths".into());
        }
        if self
            .controller
            .allow_actions
            .iter()
            .enumerate()
            .any(|(index, action)| self.controller.allow_actions[..index].contains(action))
        {
            return Err("duplicate fleet action".into());
        }
        for commands in self.builds.values() {
            validate_commands(commands)?;
        }
        let mut ids = HashSet::new();
        for machine in &self.machines {
            if !identifier(&machine.id) || !ids.insert(machine.id.as_str()) {
                return Err("fleet machines require unique identifiers".into());
            }
            validate_machine_inputs(machine)?;
            if machine.managed_services().len() > 16 || machine.databases.len() > 64 {
                return Err("fleet service or database limit exceeded".into());
            }
            if let Some(watcher) = &machine.watcher
                && (!watcher.home.is_absolute()
                    || !watcher.launcher.is_absolute()
                    || !machine.history
                    || !watcher
                        .launcher
                        .to_string_lossy()
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"/_.-".contains(&byte)))
            {
                return Err("invalid watcher home, launcher or non-history machine".into());
            }
            let mut services = HashSet::new();
            let managed = machine.managed_services();
            for service in &managed {
                validate_service(service)?;
                if !services.insert(&service.id) {
                    return Err("duplicate fleet service identity".into());
                }
            }
        }
        if !ids.contains(self.controller.node.as_str()) {
            return Err("fleet controller names an unknown machine".into());
        }
        // Server IDs share the target namespace with machine IDs.
        let mut servers = HashSet::new();
        for (_, service, _) in self.servers() {
            if ids.contains(service.id.as_str()) || !servers.insert(service.id.as_str()) {
                return Err("fleet shared servers require IDs unique among all targets".into());
            }
        }
        if self.machines.iter().any(|machine| {
            machine
                .deployment
                .requires
                .iter()
                .any(|id| !servers.contains(id.as_str()))
        }) {
            return Err("fleet dependency names an unknown shared server".into());
        }
        Ok(())
    }

    /// A confirmation binds to all configuration, including trusted deployment commands.
    ///
    /// # Errors
    /// Returns an encoding error rather than accepting an unbound confirmation.
    pub fn revision(&self) -> Result<String, String> {
        serde_json::to_vec(self)
            .map(|bytes| blake3::hash(&bytes).to_hex().to_string())
            .map_err(|_| "cannot fingerprint fleet inventory".into())
    }
}

fn validate_machine_inputs(machine: &Machine) -> Result<(), String> {
    if machine.token_env.as_ref().is_some_and(|name| {
        name.is_empty()
            || name.len() > 128
            || !name.bytes().enumerate().all(|(index, byte)| {
                byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit())
            })
    }) {
        return Err("invalid fleet token environment variable name".into());
    }
    let mut databases = HashSet::new();
    for database in &machine.databases {
        if !identifier(&database.id)
            || !databases.insert(&database.id)
            || database.name.trim().is_empty()
            || database.name.len() > 128
            || !database.path.is_absolute()
        {
            return Err("invalid fleet database identity or path".into());
        }
    }
    for commands in [&machine.deployment.preflight, &machine.deployment.install] {
        validate_commands(commands)?;
    }
    for command in [
        &machine.deployment.connection_check,
        &machine.deployment.mcp,
    ] {
        if !command.is_empty() {
            validate_commands(std::slice::from_ref(command))?;
        }
    }
    Ok(())
}

fn validate_service(service: &Service) -> Result<(), String> {
    if !identifier(&service.id)
        || !identifier(&service.unit)
        || service.name.trim().is_empty()
        || service.name.len() > 128
        || service
            .plist
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
        || service.log.as_ref().is_some_and(|path| !path.is_absolute())
        || (service.manager == Manager::Launchd && service.allow_control && service.plist.is_none())
    {
        return Err("invalid fleet service identity or supervisor configuration".into());
    }
    if let Some(deployment) = &service.deployment {
        if service.kind != ServiceKind::Server {
            return Err("only a server service can carry fleet deployment commands".into());
        }
        for commands in [
            &deployment.preflight,
            &deployment.install,
            &deployment.verify,
            &deployment.replay_views,
        ] {
            validate_commands(commands)?;
        }
    }
    Ok(())
}

fn validate_commands(commands: &[Vec<String>]) -> Result<(), String> {
    if commands.len() > 64
        || commands.iter().any(|command| {
            command.is_empty()
                || command.len() > 128
                || command[0].is_empty()
                || command
                    .iter()
                    .any(|argument| argument.len() > 16_384 || argument.contains('\0'))
        })
    {
        return Err("invalid or oversized fleet command arguments".into());
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn fixture() -> Inventory {
        toml::from_str(
            r#"
version = 1
[controller]
node = "xenia"
repository = "/srv/mash"
binary = "/srv/bin/mash-fleet"
jobs = "/srv/private/jobs"
allow_actions = ["status", "watch-start"]
[[machines]]
id = "xenia"
platform = "linux"
dashboard_url = "http://127.0.0.1:8790"
[machines.transport]
kind = "local"
[[machines.services]]
id = "watcher"
name = "Index watcher"
kind = "watcher"
manager = "systemd_user"
unit = "mash-index-watch.service"
allow_control = true
"#,
        )
        .unwrap()
    }

    #[test]
    fn inventory_binds_confirmations_to_commands_and_control_permissions() {
        let mut inventory = fixture();
        inventory.validate().unwrap();
        let original = inventory.revision().unwrap();
        inventory.machines[0].deployment.install = vec![vec!["/srv/install".into()]];
        assert_ne!(inventory.revision().unwrap(), original);
        let changed = inventory.revision().unwrap();
        inventory.machines[0].services[0].allow_control = false;
        assert_ne!(inventory.revision().unwrap(), changed);
    }

    #[test]
    fn inventory_rejects_ambiguous_machines_and_missing_dependencies() {
        let mut inventory = fixture();
        inventory.machines.push(inventory.machines[0].clone());
        assert!(inventory.validate().is_err());
        inventory.machines.pop();
        inventory.machines[0]
            .deployment
            .requires
            .push("missing-api".into());
        assert!(inventory.validate().is_err());
        inventory.machines[0].deployment.requires.clear();
        inventory.machines[0].services[0].unit = "--all".into();
        assert!(inventory.validate().is_err());
    }

    #[test]
    fn inventory_rejects_machine_dependencies_invalid_commands_and_database_paths() {
        let mut inventory = fixture();
        inventory.machines[0].deployment.requires = vec!["xenia".into()];
        assert!(inventory.validate().is_err());
        inventory.machines[0].deployment.requires.clear();
        inventory.machines[0].deployment.install = vec![vec![]];
        assert!(inventory.validate().is_err());
        inventory.machines[0].deployment.install.clear();
        inventory.machines[0].databases.push(Database {
            id: "history".into(),
            name: "History".into(),
            path: "relative.sqlite".into(),
        });
        assert!(inventory.validate().is_err());
        inventory.machines[0].databases.clear();
        inventory.machines[0].token_env = Some("TOKEN=secret".into());
        assert!(inventory.validate().is_err());
    }

    #[test]
    fn shared_server_is_a_service_target_of_its_machine() {
        let mut inventory = fixture();
        inventory.controller.allow_actions =
            vec![Action::Deploy, Action::Sync, Action::ReplayViews];
        let deployment = ServerDeployment {
            install: vec![vec!["/srv/install-server".into()]],
            verify: vec![vec!["/srv/check-server".into()]],
            ..ServerDeployment::default()
        };
        // Only a server may be deployed, and its ID may not shadow a machine.
        inventory.machines[0].services[0].deployment = Some(deployment.clone());
        assert!(inventory.validate().is_err());
        inventory.machines[0].services[0].deployment = None;
        inventory.machines[0].services.push(Service {
            id: "xenia".into(),
            name: "Shared server".into(),
            kind: ServiceKind::Server,
            manager: Manager::SystemdUser,
            unit: "mash-shared.service".into(),
            plist: None,
            log: None,
            allow_control: false,
            deployment: Some(deployment),
        });
        assert!(inventory.validate().is_err());
        inventory.machines[0].services[1].id = "shared".into();
        inventory.machines[0].deployment.requires = vec!["shared".into()];
        inventory.validate().unwrap();

        assert_eq!(
            inventory.targets(Action::Deploy, &[]).unwrap(),
            ["shared", "xenia"]
        );
        assert_eq!(inventory.targets(Action::Sync, &[]).unwrap(), ["xenia"]);
        assert_eq!(
            inventory.targets(Action::ReplayViews, &[]).unwrap(),
            ["shared"]
        );
        assert!(inventory.targets(Action::Sync, &["shared".into()]).is_err());
        // A machine cannot be deployed without the server it depends on.
        assert!(
            inventory
                .targets(Action::Deploy, &["xenia".into()])
                .is_err()
        );
        inventory
            .targets(Action::Deploy, &["shared".into()])
            .unwrap();

        // Ansible still sees the server as its own host in the API group,
        // reached the same way as the machine that declares it.
        let document: serde_json::Value =
            serde_json::from_slice(&crate::ansible::encode(&inventory).unwrap()).unwrap();
        let children = &document["all"]["children"];
        let server = &children["mash_api"]["hosts"]["shared"];
        assert_eq!(server["ansible_connection"], "local");
        assert_eq!(server["mash_install"][0][0], "/srv/install-server");
        assert_eq!(server["mash_verify"][0][0], "/srv/check-server");
        assert!(children["mash_api"]["hosts"].get("xenia").is_none());
        assert!(children["mash_history"]["hosts"].get("shared").is_none());
        assert_eq!(
            children["mash_history"]["hosts"]["xenia"]["mash_requires"][0],
            "shared"
        );

        // A machine that only hosts the server is not a history target.
        inventory.machines[0].deployment.requires.clear();
        inventory.machines[0].history = false;
        assert_eq!(inventory.targets(Action::Deploy, &[]).unwrap(), ["shared"]);
        assert!(inventory.targets(Action::Sync, &[]).is_err());
    }

    #[test]
    fn documented_canonical_inventory_is_valid_and_round_trips() {
        let inventory: Inventory =
            toml::from_str(include_str!("../../../deployment/fleet.example.toml")).unwrap();
        inventory.validate().unwrap();
        let decoded: Inventory = toml::from_str(&toml::to_string(&inventory).unwrap()).unwrap();
        assert_eq!(inventory.revision().unwrap(), decoded.revision().unwrap());
        assert_eq!(
            inventory.targets(Action::WatchStart, &[]).unwrap(),
            ["workstation", "macbook"]
        );
    }

    #[test]
    fn inventory_projects_one_watcher_identity_for_ansible_and_dashboard() {
        let mut inventory = fixture();
        inventory.machines[0].services.clear();
        inventory.machines[0].token_env = Some("PRIVATE_DASHBOARD_TOKEN".into());
        inventory.machines[0].watcher = Some(Watcher {
            launcher: "/home/operator/bin/mash-watch".into(),
            home: "/home/operator".into(),
            allow_control: true,
        });
        inventory.validate().unwrap();
        let services = inventory.machines[0].managed_services();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].unit, "mash-index-watch.service");
        assert!(services[0].allow_control);
        let bytes = crate::ansible::encode(&inventory).unwrap();
        let document: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let host = &document["all"]["children"]["mash_history"]["hosts"]["xenia"];
        assert_eq!(host["mash_watch_launcher"], "/home/operator/bin/mash-watch");
        assert_eq!(host["mash_watch_home"], "/home/operator");
        assert_eq!(host["mash_watch_enabled"], true);
        assert!(
            !String::from_utf8(bytes)
                .unwrap()
                .contains("PRIVATE_DASHBOARD_TOKEN")
        );
        assert_eq!(
            inventory.targets(Action::WatchStart, &[]).unwrap(),
            ["xenia"]
        );
        assert!(inventory.targets(Action::Deploy, &[]).is_err());
        assert!(
            inventory
                .targets(Action::Status, &["unknown".into()])
                .is_err()
        );
        assert!(
            inventory
                .targets(Action::Status, &["xenia".into(), "xenia".into()])
                .is_err()
        );
    }
}
