use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub repository: PathBuf,
    pub logs: PathBuf,
    pub inventory: PathBuf,
    pub runner: Command,
    pub nodes: Vec<String>,
    pub playbooks: Playbooks,
    #[serde(default = "timeout")]
    pub timeout_seconds: u64,
}

fn timeout() -> u64 {
    7200
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Playbooks {
    pub deploy: PathBuf,
    pub refresh: PathBuf,
    pub check: PathBuf,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub argv: Vec<String>,
}

impl Command {
    pub fn process(&self, directory: &Path) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(&self.argv[0]);
        command
            .args(&self.argv[1..])
            .current_dir(directory)
            .kill_on_drop(true);
        command
    }
}

pub fn load(path: &Path) -> Result<Config, String> {
    let text = std::fs::read_to_string(path).map_err(|_| "cannot read dev configuration")?;
    let config: Config = toml::from_str(&text).map_err(|_| "invalid dev TOML configuration")?;
    config.validate()?;
    Ok(config)
}

impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if !self.repository.is_absolute()
            || !self.repository.is_dir()
            || !self.logs.is_absolute()
            || !self.inventory.is_absolute()
            || !self.inventory.is_file()
            || !(1..=86_400).contains(&self.timeout_seconds)
            || self.nodes.is_empty()
            || self.nodes.len() > 32
            || self
                .nodes
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.nodes.len()
            || self.nodes.iter().any(|node| {
                node.is_empty()
                    || !node.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                    })
            })
            || self.runner.argv.is_empty()
            || self.runner.argv[0].is_empty()
            || self.runner.argv.iter().any(|value| value.contains('\0'))
        {
            return Err("use absolute repository/log/inventory paths, distinct simple target names, bounded timeout and a trusted runner command".into());
        }
        for playbook in [
            &self.playbooks.deploy,
            &self.playbooks.refresh,
            &self.playbooks.check,
        ] {
            if playbook.is_absolute()
                || playbook
                    .components()
                    .any(|component| !matches!(component, std::path::Component::Normal(_)))
                || !self.repository.join(playbook).is_file()
            {
                return Err(
                    "playbooks must be existing relative files inside the repository".into(),
                );
            }
        }
        Ok(())
    }
}
