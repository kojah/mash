//! Explicit service inventory. Never accept executable names or shell fragments
//! from a browser: operating-system managers receive configured identifiers.
use crate::AppError;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Configuration {
    pub node: String,
    #[serde(default)]
    pub token_env: Option<String>,
    #[serde(default)]
    pub services: Vec<Service>,
    #[serde(default)]
    pub machines: Vec<Machine>,
    #[serde(default)]
    pub databases: Vec<Database>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Database {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Machine {
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub token_env: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Service {
    #[cfg(test)]
    #[serde(skip)]
    pub interrupt_after_command: bool,
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub manager: Manager,
    pub unit: String,
    /// launchd's existing plist, required for starting an unloaded job.
    pub plist: Option<PathBuf>,
    /// Optional existing log file; no directory browsing is exposed.
    pub log: Option<PathBuf>,
    #[serde(default)]
    pub allow_control: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    Qdrant,
    Watcher,
    Mcp,
    Server,
    Dashboard,
    Sqlite,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Manager {
    SystemdUser,
    SystemdSystem,
    Launchd,
    WindowsService,
    WindowsTask,
}

impl Service {
    pub fn control_revision(&self) -> String {
        blake3::hash(
            format!(
                "{:?}|{:?}|{}|{:?}|{}",
                self.manager, self.kind, self.unit, self.plist, self.allow_control
            )
            .as_bytes(),
        )
        .to_hex()
        .to_string()
    }
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
        && !value.starts_with('-')
}

pub(super) fn token(name: &str) -> Result<String, AppError> {
    if name.is_empty()
        || name.as_bytes()[0].is_ascii_digit()
        || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
    {
        return Err(AppError::configuration("invalid services token_env"));
    }
    let value = std::env::var(name)
        .map_err(|_| AppError::configuration("services authentication token is unavailable"))?;
    if !(32..=4096).contains(&value.len()) || !value.bytes().all(|c| (33..=126).contains(&c)) {
        return Err(AppError::configuration(
            "services tokens require 32..4096 printable ASCII bytes",
        ));
    }
    Ok(value)
}

impl Configuration {
    pub(super) fn from_fleet(
        inventory: &mash_fleet::inventory::Inventory,
        node: Option<&str>,
    ) -> Result<Self, AppError> {
        let node = node.unwrap_or(&inventory.controller.node);
        let machine = inventory
            .machines
            .iter()
            .find(|machine| machine.id == node)
            .ok_or_else(|| {
                AppError::configuration("dashboard machine is absent from fleet inventory")
            })?;
        let config = Self {
            node: node.into(),
            token_env: machine.token_env.clone(),
            services: machine
                .managed_services()
                .into_iter()
                .map(Service::from)
                .collect(),
            databases: machine
                .databases
                .iter()
                .map(|database| Database {
                    id: database.id.clone(),
                    name: database.name.clone(),
                    path: database.path.clone(),
                })
                .collect(),
            machines: inventory
                .machines
                .iter()
                .filter(|peer| node == inventory.controller.node && peer.id != node)
                .filter_map(|peer| {
                    peer.dashboard_url.as_ref().map(|url| Machine {
                        id: peer.id.clone(),
                        url: url.clone(),
                        token_env: peer.token_env.clone(),
                    })
                })
                .collect(),
        };
        config.validate()?;
        Ok(config)
    }

    pub fn load(path: &Path) -> Result<Self, AppError> {
        let mut text = String::new();
        std::fs::File::open(path)
            .and_then(|file| file.take(1_048_577).read_to_string(&mut text))
            .map_err(|_| AppError::configuration("cannot read services configuration"))?;
        if text.len() > 1_048_576 {
            return Err(AppError::configuration(
                "services configuration is too large",
            ));
        }
        let config: Self = toml::from_str(&text)
            .map_err(|_| AppError::configuration("invalid services configuration TOML"))?;
        config.validate()?;
        Ok(config)
    }

    pub(super) fn validate(&self) -> Result<(), AppError> {
        let invalid = || {
            AppError::configuration(
                "invalid services inventory: use unique identifiers, explicit manager units and absolute file paths",
            )
        };
        if !identifier(&self.node)
            || self.services.len() > 16
            || self.machines.len() > 32
            || self.databases.len() > 64
        {
            return Err(invalid());
        }
        let mut ids = HashSet::new();
        for service in &self.services {
            if !identifier(&service.id)
                || !ids.insert(&service.id)
                || service.name.trim().is_empty()
                || service.name.len() > 128
                || !identifier(&service.unit)
                || matches!(service.kind, Kind::Mcp | Kind::Sqlite)
                || service
                    .plist
                    .as_ref()
                    .is_some_and(|path| !path.is_absolute())
                || service.log.as_ref().is_some_and(|path| !path.is_absolute())
                || (service.manager == Manager::Launchd
                    && service.allow_control
                    && service.plist.is_none())
            {
                return Err(invalid());
            }
        }
        let mut machines = HashSet::from([self.node.as_str()]);
        let mut databases = HashSet::new();
        for database in &self.databases {
            if !identifier(&database.id)
                || !databases.insert(&database.id)
                || database.name.trim().is_empty()
                || database.name.len() > 128
                || !database.path.is_absolute()
            {
                return Err(invalid());
            }
        }
        for machine in &self.machines {
            if !identifier(&machine.id) || !machines.insert(machine.id.as_str()) {
                return Err(invalid());
            }
            let url = reqwest::Url::parse(&machine.url).map_err(|_| invalid())?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || url.path() != "/"
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
}

impl From<mash_fleet::inventory::Service> for Service {
    fn from(service: mash_fleet::inventory::Service) -> Self {
        use mash_fleet::inventory::{Manager as FleetManager, ServiceKind};
        Self {
            #[cfg(test)]
            interrupt_after_command: false,
            id: service.id,
            name: service.name,
            kind: match service.kind {
                ServiceKind::Watcher => Kind::Watcher,
                ServiceKind::Qdrant => Kind::Qdrant,
                ServiceKind::Server => Kind::Server,
                ServiceKind::Dashboard => Kind::Dashboard,
            },
            manager: match service.manager {
                FleetManager::SystemdUser => Manager::SystemdUser,
                FleetManager::SystemdSystem => Manager::SystemdSystem,
                FleetManager::Launchd => Manager::Launchd,
                FleetManager::WindowsService => Manager::WindowsService,
                FleetManager::WindowsTask => Manager::WindowsTask,
            },
            unit: service.unit,
            plist: service.plist,
            log: service.log,
            allow_control: service.allow_control,
        }
    }
}

#[cfg(test)]
#[test]
fn fleet_projection_shares_watcher_identity_and_limits_peer_credentials() {
    let inventory: mash_fleet::inventory::Inventory =
        toml::from_str(include_str!("../../../deployment/fleet.example.toml")).unwrap();
    let controller = Configuration::from_fleet(&inventory, None).unwrap();
    assert_eq!(controller.node, "workstation");
    assert_eq!(controller.machines.len(), 1);
    assert_eq!(controller.machines[0].id, "macbook");
    assert_eq!(controller.services[0].unit, "mash-index-watch.service");
    let peer = Configuration::from_fleet(&inventory, Some("macbook")).unwrap();
    assert!(peer.machines.is_empty());
    assert_eq!(
        peer.token_env.as_deref(),
        Some("MASH_MACBOOK_SERVICES_TOKEN")
    );
    assert_eq!(peer.services[0].unit, "dev.mash.index-watch");
    assert_eq!(
        peer.services[0].plist.as_deref(),
        Some(Path::new(
            "/Users/operator/Library/LaunchAgents/dev.mash.index-watch.plist"
        ))
    );
    assert!(Configuration::from_fleet(&inventory, Some("missing")).is_err());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn services_inventory_allows_missing_authentication_tokens() {
        let config: Configuration = toml::from_str("node = \"local\"\n").unwrap();
        assert!(config.token_env.is_none());
        config.validate().unwrap();
    }

    #[test]
    fn services_inventory_rejects_shell_fragments_and_ambiguous_targets() {
        let input = r#"node = "xenia"
token_env = "MASH_SERVICES_TOKEN"
[[services]]
id = "watcher"
name = "Index watcher"
kind = "watcher"
manager = "systemd_user"
unit = "mash-watch.service"
allow_control = true
"#;
        let valid: Configuration = toml::from_str(input).unwrap();
        valid.validate().unwrap();
        for bad in ["--all", "a;reboot", "$(whoami)", "a/b"] {
            let value = input.replace("mash-watch.service", bad);
            assert!(
                toml::from_str::<Configuration>(&value)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        let duplicate = format!("{input}{}", &input[input.find("[[services]]").unwrap()..]);
        assert!(
            toml::from_str::<Configuration>(&duplicate)
                .unwrap()
                .validate()
                .is_err()
        );
        for kind in ["mcp", "sqlite"] {
            let value = input.replace("kind = \"watcher\"", &format!("kind = \"{kind}\""));
            assert!(
                toml::from_str::<Configuration>(&value)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
    }
}
