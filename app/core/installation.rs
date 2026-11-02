//! Read-only setup diagnostics that tolerate a broken index or configuration.
//! Client settings are observations, never proof of what an already running agent loaded.
mod clients;
mod plugin;
mod probe;
mod resources;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Agent {
    Codex,
    ClaudeCode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum State {
    Present,
    Missing,
    Disabled,
    Invalid,
    Outdated,
    Unverified,
    NotConfigured,
    Passed,
    Failed,
}

impl State {
    const fn needs_attention(self) -> bool {
        matches!(
            self,
            Self::Missing | Self::Disabled | Self::Invalid | Self::Outdated | Self::Failed
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum Component {
    Configuration,
    Plugin,
    Skill,
    Hooks,
    HookTrust,
    Mcp,
    Probe,
    Index,
    Runtime,
}

#[derive(Debug, Serialize)]
struct Finding {
    component: Component,
    state: State,
    path: Option<PathBuf>,
    message: String,
    remediation: Option<String>,
}

#[derive(Debug, Serialize)]
struct Client {
    agent: Agent,
    detected: bool,
    expected: bool,
    findings: Vec<Finding>,
    #[serde(skip)]
    launch: Option<clients::Launch>,
}

impl Client {
    fn add(
        &mut self,
        component: Component,
        state: State,
        path: Option<&Path>,
        message: &str,
        remedy: Option<&str>,
    ) {
        self.findings.push(Finding {
            component,
            state,
            path: path.map(Path::to_path_buf),
            message: message.into(),
            remediation: remedy.map(str::to_owned),
        });
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Report {
    pub(super) summary: Summary,
    clients: Vec<Client>,
    scope: &'static str,
    runtime: &'static str,
    resources: Option<resources::Report>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Summary {
    attention: usize,
    unverified: usize,
    unconfigured: usize,
    clients: Vec<ClientSummary>,
    next_action: &'static str,
}

#[derive(Debug, Serialize)]
struct ClientSummary {
    agent: Agent,
    expected: bool,
    attention: usize,
    unverified: usize,
}

impl Report {
    fn update_summary(&mut self) {
        self.summary.unconfigured = self
            .clients
            .iter()
            .filter(|client| client.detected && !client.expected)
            .count();
        self.summary.clients = self
            .clients
            .iter()
            .map(|client| ClientSummary {
                agent: client.agent,
                expected: client.expected,
                attention: client
                    .findings
                    .iter()
                    .filter(|finding| {
                        (client.expected || matches!(finding.component, Component::Configuration))
                            && finding.state.needs_attention()
                    })
                    .count(),
                unverified: client
                    .findings
                    .iter()
                    .filter(|finding| {
                        (client.expected || matches!(finding.component, Component::Configuration))
                            && finding.state == State::Unverified
                    })
                    .count(),
            })
            .collect();
        self.summary.attention = self
            .summary
            .clients
            .iter()
            .map(|client| client.attention)
            .sum();
        self.summary.unverified = self
            .summary
            .clients
            .iter()
            .map(|client| client.unverified)
            .sum();
        if let Some(resources) = &self.resources {
            self.summary.attention += resources
                .findings
                .iter()
                .filter(|f| f.state.needs_attention())
                .count();
            self.summary.unverified += resources
                .findings
                .iter()
                .filter(|f| f.state == State::Unverified)
                .count();
        }
    }
}

#[derive(Clone)]
struct Environment {
    home: PathBuf,
    codex: PathBuf,
    claude: PathBuf,
    cwd: PathBuf,
    path: std::ffi::OsString,
}

impl Environment {
    fn discover() -> Option<Self> {
        let home = directories::BaseDirs::new()?.home_dir().to_path_buf();
        Some(Self {
            codex: std::env::var_os("CODEX_HOME")
                .map_or_else(|| home.join(".codex"), PathBuf::from),
            claude: std::env::var_os("CLAUDE_CONFIG_DIR")
                .map_or_else(|| home.join(".claude"), PathBuf::from),
            home,
            cwd: std::env::current_dir().ok()?,
            path: std::env::var_os("PATH").unwrap_or_default(),
        })
    }
}

fn collect(environment: Option<&Environment>, agents: &[Agent]) -> Report {
    let mut report = Report {
        summary: Summary {
            attention: 0,
            unverified: 0,
            unconfigured: 0,
            clients: vec![],
            next_action: "mash status --installation",
        },
        clients: vec![],
        resources: None,
        scope: "Local Codex and Claude Code user settings and current-project override detection. Managed settings, profiles, remote hosts and other clients are not certified.",
        runtime: "Existing agent sessions, loaded plugins and live hook execution are unverified. Restart the agent after repairs. A probe tests a new process only.",
    };
    if let Some(environment) = environment {
        for agent in [Agent::Codex, Agent::ClaudeCode] {
            if agents.is_empty() || agents.contains(&agent) {
                report.clients.push(clients::inspect(
                    environment,
                    agent,
                    agents.contains(&agent),
                ));
            }
        }
        report.update_summary();
    } else {
        report.summary.unverified = 1;
        report.scope =
            "Cannot determine the home or working directory; installation is unverified.";
    }
    report
}

pub(super) fn summary() -> Summary {
    collect(Environment::discover().as_ref(), &[]).summary
}

pub(crate) fn help_notice(paths: &super::Paths) -> Option<&'static str> {
    if resources::index_finding(
        &paths
            .db
            .clone()
            .unwrap_or_else(super::default_database_path),
    )
    .state
    .needs_attention()
    {
        return Some(
            "Installation warning: the local index is incompatible or unreadable, or an upgrade is unfinished. Run `mash status --installation` for recovery details before indexing or replacing files.",
        );
    }
    if resources::setup_needs_attention(paths) {
        return Some(
            "Installation warning: Mash configuration or configured provider roots need attention. Run `mash status --installation` for setup details.",
        );
    }
    let summary = summary();
    if summary.attention > 0 {
        Some(
            "Installation warning: some Mash agent integrations are incomplete. Run `mash status --installation` for findings and repair steps.",
        )
    } else if summary.unconfigured > 0 {
        Some(
            "Installation notice: Mash integration is not configured for some detected agents. CLI-only use is valid; run `mash status --installation` for setup details.",
        )
    } else {
        None
    }
}

pub(super) async fn inspect(
    agents: Vec<Agent>,
    run_probe: bool,
    paths: super::Paths,
) -> Result<Report, crate::AppError> {
    let mut report = crate::blocking(move || {
        let mut report = collect(Environment::discover().as_ref(), &agents);
        report.resources = Some(resources::inspect(&paths));
        report.update_summary();
        Ok(report)
    })
    .await?;
    if run_probe {
        for client in &mut report.clients {
            probe::check(client).await;
        }
        report.update_summary();
    }
    Ok(report)
}

/// Bounded reads and deliberately generic parse failures keep credentials out of diagnostics.
fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>, &'static str> {
    use std::io::Read;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Cannot read this settings file; check its permissions."),
    };
    let mut bytes = Vec::new();
    file.take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read this settings file.")?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("Settings file exceeds the 4 MiB inspection limit.");
    }
    if path
        .extension()
        .is_some_and(|extension| extension == "toml")
    {
        let text = std::str::from_utf8(&bytes).map_err(|_| "Settings file is not UTF-8.")?;
        toml::from_str(text).map(Some).map_err(
            |_| "Invalid or unsupported TOML settings; inspect the file in the agent client.",
        )
    } else {
        serde_json::from_slice(&bytes).map(Some).map_err(
            |_| "Invalid or unsupported JSON settings; inspect the file in the agent client.",
        )
    }
}

fn executable(command: &str, environment: &Environment) -> Option<PathBuf> {
    let path = Path::new(command);
    if path.is_absolute() || path.components().count() > 1 {
        let path = environment.cwd.join(path);
        return is_executable(&path).then_some(path);
    }
    crate::platform::find_program(command, &environment.path, &environment.cwd)
}

fn is_executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|metadata| crate::platform::is_executable(&metadata))
}

#[derive(Deserialize)]
struct Stamp {
    contract_fingerprint: String,
    plugin_version: String,
}

fn expected_stamp() -> Stamp {
    // Generated alongside the bundled skill; invalid build assets are handled without panicking.
    serde_json::from_str(include_str!(
        "../../plugins/mash/skills/history/contract.json"
    ))
    .unwrap_or(Stamp {
        contract_fingerprint: String::new(),
        plugin_version: String::new(),
    })
}

#[cfg(test)]
mod tests;
