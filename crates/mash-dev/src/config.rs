use serde::Deserialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub repository: PathBuf,
    pub logs: PathBuf,
    #[serde(default = "timeout")]
    pub timeout_seconds: u64,
    pub builds: BTreeMap<String, Vec<Command>>,
    pub nodes: BTreeMap<String, Node>,
}

fn timeout() -> u64 {
    7200
}

#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Node {
    History {
        build: String,
        install: Vec<Command>,
        mcp: Command,
        #[serde(default)]
        shared: bool,
        #[serde(default)]
        requires: Vec<String>,
    },
    Api {
        build: String,
        install: Vec<Command>,
        /// Read-only checks, including authenticated readiness and release identity.
        verify: Vec<Command>,
    },
}

impl Node {
    pub fn build(&self) -> &str {
        match self {
            Self::History { build, .. } | Self::Api { build, .. } => build,
        }
    }
    pub fn install(&self) -> &[Command] {
        match self {
            Self::History { install, .. } | Self::Api { install, .. } => install,
        }
    }
    pub fn requires(&self) -> &[String] {
        match self {
            Self::History { requires, .. } => requires,
            Self::Api { .. } => &[],
        }
    }
    fn commands(&self) -> Vec<&Command> {
        match self {
            Self::History { install, mcp, .. } => {
                install.iter().chain(std::iter::once(mcp)).collect()
            }
            Self::Api {
                install, verify, ..
            } => install.iter().chain(verify).collect(),
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    /// argv, executed directly; no implicit shell expansion.
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
    let input = std::fs::read_to_string(path).map_err(|_| "cannot read dev configuration")?;
    let config: Config = toml::from_str(&input).map_err(|_| "invalid dev TOML configuration")?;
    config.validate()?;
    Ok(config)
}

impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if !self.repository.is_absolute()
            || !self.repository.is_dir()
            || !self.logs.is_absolute()
            || self.timeout_seconds == 0
            || self.timeout_seconds > 86_400
            || self.nodes.is_empty()
        {
            return Err("use an existing absolute repository, absolute logs, 1..86400-second timeout and named nodes".into());
        }
        for (name, node) in &self.nodes {
            if name.is_empty()
                || name.len() > 64
                || !name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
            {
                return Err(
                    "node names must contain only letters, digits, hyphens or underscores".into(),
                );
            }
            if !self.builds.contains_key(node.build()) || node.install().is_empty() {
                return Err(
                    "each node needs a configured build and nonempty install workflow".into(),
                );
            }
            if let Node::Api { verify, .. } = node
                && verify.is_empty()
            {
                return Err("API targets need nonempty read-only checks".into());
            }
            let mut unique = std::collections::BTreeSet::new();
            if node.requires().iter().any(|name| {
                !unique.insert(name) || !matches!(self.nodes.get(name), Some(Node::Api { .. }))
            }) {
                return Err("history prerequisites must be distinct configured API targets".into());
            }
        }
        let commands = self
            .builds
            .values()
            .flatten()
            .chain(self.nodes.values().flat_map(Node::commands));
        if self.builds.values().any(Vec::is_empty)
            || commands.into_iter().any(|c| {
                c.argv.is_empty() || c.argv[0].is_empty() || c.argv.iter().any(|s| s.contains('\0'))
            })
        {
            return Err("workflows need nonempty commands without NUL bytes".into());
        }
        Ok(())
    }
}
