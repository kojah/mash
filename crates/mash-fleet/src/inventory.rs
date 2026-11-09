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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    History,
    Api,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Machine {
    pub id: String,
    pub platform: Platform,
    pub roles: Vec<Role>,
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
    pub verify: Commands,
    pub replay_views: Commands,
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
    /// Resolve explicit hosts or all eligible hosts against the same allowlist.
    ///
    /// # Errors
    /// Rejects disabled actions, unknown/duplicate hosts and invalid action scopes.
    pub fn targets(&self, action: Action, requested: &[String]) -> Result<Vec<String>, String> {
        self.validate()?;
        if !self.controller.allow_actions.contains(&action) {
            return Err("fleet action is not enabled in the controller inventory".into());
        }
        let eligible = |machine: &Machine| match action {
            Action::Status | Action::Preflight | Action::Deploy => true,
            Action::Sync | Action::LocalViews => machine.roles.contains(&Role::History),
            Action::ReplayViews => machine.roles.contains(&Role::Api),
            Action::WatchStart | Action::WatchStop | Action::WatchStatus | Action::WatchLogs => {
                machine.roles.contains(&Role::History) && machine.watcher.is_some()
            }
        };
        let selected: Vec<String> = if requested.is_empty() {
            self.machines
                .iter()
                .filter(|machine| eligible(machine))
                .map(|machine| machine.id.clone())
                .collect()
        } else {
            requested.to_vec()
        };
        let unique: HashSet<_> = selected.iter().collect();
        if selected.is_empty()
            || unique.len() != selected.len()
            || selected.len() > 32
            || selected.iter().any(|id| {
                !self
                    .machines
                    .iter()
                    .any(|machine| machine.id == *id && eligible(machine))
            })
        {
            return Err("fleet action requires unique eligible machine IDs".into());
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
            return Err("include required API machines in the deployment selection".into());
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
            if !identifier(&machine.id)
                || !ids.insert(machine.id.as_str())
                || machine.roles.is_empty()
                || machine
                    .roles
                    .iter()
                    .enumerate()
                    .any(|(index, role)| machine.roles[..index].contains(role))
            {
                return Err("fleet machines require unique identifiers and explicit roles".into());
            }
            validate_machine_inputs(machine)?;
            if machine.managed_services().len() > 16 || machine.databases.len() > 64 {
                return Err("fleet service or database limit exceeded".into());
            }
            if let Some(watcher) = &machine.watcher
                && (!watcher.home.is_absolute()
                    || !watcher.launcher.is_absolute()
                    || !machine.roles.contains(&Role::History)
                    || !watcher
                        .launcher
                        .to_string_lossy()
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"/_.-".contains(&byte)))
            {
                return Err("invalid watcher home, launcher or machine role".into());
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
        self.validate_dependencies(&ids)
    }

    fn validate_dependencies(&self, ids: &HashSet<&str>) -> Result<(), String> {
        if !ids.contains(self.controller.node.as_str())
            || self.machines.iter().any(|machine| {
                machine
                    .deployment
                    .requires
                    .iter()
                    .any(|id| !ids.contains(id.as_str()))
            })
        {
            return Err("fleet controller or dependency names an unknown machine".into());
        }
        for machine in &self.machines {
            let mut pending = machine.deployment.requires.clone();
            let mut visited = HashSet::new();
            while let Some(id) = pending.pop() {
                if id == machine.id {
                    return Err("cyclic fleet deployment dependencies".into());
                }
                if visited.insert(id.clone())
                    && let Some(dependency) = self.machines.iter().find(|entry| entry.id == id)
                {
                    pending.extend(dependency.deployment.requires.iter().cloned());
                }
            }
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
    for commands in [
        &machine.deployment.preflight,
        &machine.deployment.install,
        &machine.deployment.verify,
        &machine.deployment.replay_views,
    ] {
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
roles = ["history"]
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
    fn inventory_rejects_cycles_invalid_commands_and_database_paths() {
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
