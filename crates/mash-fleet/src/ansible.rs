//! Project the private fleet inventory into Ansible's existing host variables.
use crate::inventory::{Commands, Inventory, Machine, Platform, ServerDeployment, Transport};
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

#[derive(Serialize)]
struct Document<'a> {
    all: All<'a>,
}

#[derive(Serialize)]
struct All<'a> {
    vars: Variables<'a>,
    children: BTreeMap<&'static str, Group<'a>>,
}

#[derive(Serialize)]
struct Variables<'a> {
    mash_builds: &'a BTreeMap<Platform, Commands>,
    mash_fleet: &'a Path,
}

#[derive(Default, Serialize)]
struct Group<'a> {
    hosts: BTreeMap<&'a str, Host<'a>>,
}

#[derive(Serialize)]
struct Host<'a> {
    ansible_connection: &'static str,
    #[serde(rename = "ansible_host", skip_serializing_if = "Option::is_none")]
    address: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ansible_user: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ansible_ssh_common_args: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ansible_python_interpreter: Option<&'a Path>,
    mash_platform: Platform,
    mash_requires: &'a [String],
    mash_shared: bool,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    mash_preflight: &'a [String],
    mash_deploy_preflight: &'a [Vec<String>],
    mash_install: &'a [Vec<String>],
    mash_verify: &'a [Vec<String>],
    mash_replay_views: &'a [Vec<String>],
    mash_mcp: &'a [String],
    mash_node_configs: &'a [std::path::PathBuf],
    #[serde(skip_serializing_if = "Option::is_none")]
    mash_build_root: Option<&'a Path>,
    mash_refresh_all_history: bool,
    mash_watch_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    mash_watch_launcher: Option<&'a Path>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mash_watch_home: Option<&'a Path>,
}

impl<'a> From<&'a Machine> for Host<'a> {
    fn from(machine: &'a Machine) -> Self {
        let (connection, host, user, args, python) = match &machine.transport {
            Transport::Local => ("local", None, None, None, None),
            Transport::Ssh {
                host,
                user,
                common_args,
                python,
            } => (
                "ssh",
                Some(host.as_str()),
                user.as_deref(),
                common_args.as_deref(),
                python.as_deref(),
            ),
        };
        Self {
            ansible_connection: connection,
            address: host,
            ansible_user: user,
            ansible_ssh_common_args: args,
            ansible_python_interpreter: python,
            mash_platform: machine.platform,
            mash_requires: &machine.deployment.requires,
            mash_shared: machine.deployment.shared,
            mash_preflight: &machine.deployment.connection_check,
            mash_deploy_preflight: &machine.deployment.preflight,
            mash_install: &machine.deployment.install,
            mash_verify: &[],
            mash_replay_views: &[],
            mash_mcp: &machine.deployment.mcp,
            mash_node_configs: &machine.deployment.node_configs,
            mash_build_root: machine.deployment.build_root.as_deref(),
            mash_refresh_all_history: machine.deployment.refresh_all_history,
            mash_watch_enabled: machine.watcher.is_some(),
            mash_watch_launcher: machine
                .watcher
                .as_ref()
                .map(|watcher| watcher.launcher.as_path()),
            mash_watch_home: machine
                .watcher
                .as_ref()
                .map(|watcher| watcher.home.as_path()),
        }
    }
}

impl<'a> Host<'a> {
    /// A shared server is its own Ansible host, reached like the machine that declares it.
    fn server(machine: &'a Machine, deployment: &'a ServerDeployment) -> Self {
        Self {
            mash_requires: &[],
            mash_shared: false,
            mash_deploy_preflight: &deployment.preflight,
            mash_install: &deployment.install,
            mash_verify: &deployment.verify,
            mash_replay_views: &deployment.replay_views,
            mash_mcp: &[],
            mash_node_configs: &[],
            mash_refresh_all_history: false,
            mash_watch_enabled: false,
            mash_watch_launcher: None,
            mash_watch_home: None,
            ..Self::from(machine)
        }
    }
}

/// Generate Ansible input without dashboard endpoints, tokens or service-log paths.
///
/// # Errors
/// Rejects invalid inventory and reports serialization failures.
pub fn encode<'a>(inventory: &'a Inventory) -> Result<Vec<u8>, String> {
    inventory.validate()?;
    let mut children = BTreeMap::from([
        ("mash_history", Group::default()),
        ("mash_api", Group::default()),
        ("mash_darwin", Group::default()),
    ]);
    let mut add = |group, id: &'a str, host| {
        children.entry(group).or_default().hosts.insert(id, host);
    };
    for machine in inventory.machines.iter().filter(|machine| machine.history) {
        add("mash_history", &machine.id, Host::from(machine));
        if machine.platform == Platform::Darwin {
            add("mash_darwin", &machine.id, Host::from(machine));
        }
    }
    for (machine, service, deployment) in inventory.servers() {
        add("mash_api", &service.id, Host::server(machine, deployment));
        if machine.platform == Platform::Darwin {
            add(
                "mash_darwin",
                &service.id,
                Host::server(machine, deployment),
            );
        }
    }
    serde_json::to_vec(&Document {
        all: All {
            vars: Variables {
                mash_builds: &inventory.builds,
                mash_fleet: &inventory.controller.binary,
            },
            children,
        },
    })
    .map_err(|_| "cannot encode Ansible inventory".into())
}

/// Save a private, immutable input snapshot for one invocation.
///
/// # Errors
/// Refuses an existing path and fails if the snapshot cannot be durably written.
pub fn write(inventory: &Inventory, path: &Path) -> Result<(), String> {
    let bytes = encode(inventory)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .and_then(|mut file| file.write_all(&bytes).and_then(|()| file.sync_all()))
        .map_err(|_| "cannot write private Ansible inventory snapshot".into())
}
