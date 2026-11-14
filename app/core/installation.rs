//! Read-only setup diagnostics that tolerate a broken index or configuration.
//! Client settings are observations, never proof of what an already running agent loaded.
mod clients;
mod plugin;
mod probe;
mod resources;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
#[schemars(rename = "SetupAgent")]
pub enum Agent {
    Codex,
    ClaudeCode,
}

impl Agent {
    /// The `--agent` value and JSON name.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::ClaudeCode => "claude-code",
        }
    }

    /// The agent's own command-line program.
    pub(crate) const fn program(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::ClaudeCode => "claude",
        }
    }

    /// The agent CLI command that removes Mash's user-level MCP registration.
    pub(crate) const fn remove_mcp(self) -> &'static str {
        match self {
            Self::Codex => "codex mcp remove mash",
            Self::ClaudeCode => "claude mcp remove --scope user mash",
        }
    }

    /// The agent CLI command that registers `executable` as Mash's MCP server.
    pub(crate) fn add_mcp(self, executable: &str) -> String {
        match self {
            Self::Codex => format!("codex mcp add mash -- {executable} mcp"),
            Self::ClaudeCode => format!("claude mcp add --scope user mash -- {executable} mcp"),
        }
    }

    /// The online commands that install and enable the mash@mash plugin.
    pub(crate) const fn install_plugin(self) -> &'static str {
        match self {
            Self::ClaudeCode => {
                "claude plugin marketplace add kojah/mash && claude plugin install mash@mash"
            }
            Self::Codex => "codex plugin marketplace add kojah/mash && codex plugin add mash@mash",
        }
    }

    /// The agent CLI command that removes the mash@mash plugin.
    pub(crate) const fn remove_plugin(self) -> &'static str {
        match self {
            Self::Codex => "codex plugin remove mash@mash",
            Self::ClaudeCode => "claude plugin uninstall mash@mash",
        }
    }
}

/// Why an integration is expected for an agent, strongest reason first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Expectation {
    /// Named with `--agent`.
    Named,
    /// `mash init` recorded connecting it under `[agents] connected`, so it
    /// is expected whatever its settings say now.
    Recorded,
    /// Its own settings enable the Mash plugin or register Mash's MCP server.
    Configured,
    /// Not expected: CLI-only use is valid.
    None,
}

/// How far an agent can use Mash, judged from its user settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(rename = "AgentConnection")]
pub(crate) enum Connection {
    /// No usable Mash MCP registration: the agent cannot reach Mash.
    None,
    /// The Mash MCP tools are registered and usable; the plugin (history
    /// skill and session hooks) or another part needs attention.
    McpOnly,
    /// Expected, with nothing needing attention.
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "SetupState")]
pub(crate) enum State {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Component {
    Configuration,
    Plugin,
    Skill,
    Hooks,
    HookTrust,
    Mcp,
    Probe,
    Index,
    Runtime,
    /// The local Qdrant search service that indexing and every search use.
    Qdrant,
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
    data_directory_found: bool,
    cli_path: Option<PathBuf>,
    expected: bool,
    /// Why an integration is expected for this agent.
    expectation: Expectation,
    findings: Vec<Finding>,
    #[serde(skip)]
    launch: Option<clients::Launch>,
    /// Why there is nothing to probe, when the settings say so.
    #[serde(skip)]
    probe_skipped: Option<&'static str>,
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
pub struct Report {
    pub(super) summary: Summary,
    clients: Vec<Client>,
    scope: &'static str,
    runtime: &'static str,
    resources: Option<resources::Report>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub(crate) struct Summary {
    attention: usize,
    unverified: usize,
    unconfigured: usize,
    clients: Vec<ClientSummary>,
    /// The command that would make progress, or null when nothing is left.
    next_action: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct ClientSummary {
    agent: Agent,
    expected: bool,
    connection: Connection,
    /// Parts of an expected integration that are not installed.
    missing: Vec<Component>,
    #[serde(skip)]
    required: bool,
    attention: usize,
    unverified: usize,
    /// What would repair this agent, or null when nothing needs attention.
    next_action: Option<String>,
}

impl Report {
    fn update_summary(&mut self) {
        self.summary.unconfigured = self
            .clients
            .iter()
            .filter(|client| client.data_directory_found && !client.expected)
            .count();
        self.summary.clients = self
            .clients
            .iter()
            .map(|client| {
                let attention = client
                    .findings
                    .iter()
                    .filter(|finding| {
                        (client.expected || matches!(finding.component, Component::Configuration))
                            && finding.state.needs_attention()
                    })
                    .count();
                let mut missing: Vec<_> = client
                    .findings
                    .iter()
                    .filter(|finding| client.expected && finding.state == State::Missing)
                    .map(|finding| finding.component)
                    .filter(|component| matches!(component, Component::Mcp | Component::Plugin))
                    .collect();
                missing.dedup();
                let mcp_usable = client.findings.iter().any(|finding| {
                    finding.component == Component::Mcp && finding.state == State::Present
                }) && !client.findings.iter().any(|finding| {
                    matches!(finding.component, Component::Mcp | Component::Probe)
                        && finding.state.needs_attention()
                });
                let connection = if client.expected && attention == 0 {
                    Connection::Full
                } else if client.expected && mcp_usable {
                    Connection::McpOnly
                } else {
                    Connection::None
                };
                ClientSummary {
                    agent: client.agent,
                    expected: client.expected,
                    connection,
                    next_action: (attention > 0)
                        .then(|| client.next_action(&missing))
                        .flatten(),
                    missing,
                    required: client.expectation == Expectation::Named,
                    attention,
                    unverified: client
                        .findings
                        .iter()
                        .filter(|finding| {
                            (client.expected
                                || matches!(finding.component, Component::Configuration))
                                && finding.state == State::Unverified
                        })
                        .count(),
                }
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
        self.summary.next_action = self.next_action();
    }

    fn next_action(&self) -> Option<String> {
        // Agents named with `--agent` are what the caller asked about.
        if let Some((agent, action)) = self
            .summary
            .clients
            .iter()
            .filter(|client| client.required)
            .find_map(|client| Some((client.agent, client.next_action.as_deref()?)))
        {
            return Some(format!("{}: {action}", agent.as_str()));
        }
        // Without Qdrant nothing searches, whatever the agents' state.
        if let Some(remedy) = self.resources.as_ref().and_then(|resources| {
            resources
                .findings
                .iter()
                .find(|finding| {
                    finding.component == Component::Qdrant && finding.state.needs_attention()
                })
                .and_then(|finding| finding.remediation.clone())
        }) {
            return Some(remedy);
        }
        for client in &self.clients {
            if client.expected
                && client.cli_path.is_none()
                && client.findings.iter().any(|finding| {
                    matches!(finding.component, Component::Mcp | Component::Plugin)
                        && finding.state.needs_attention()
                })
            {
                return Some(install_cli(client.agent));
            }
        }
        // Unconfigured agents are valid CLI-only use. A missing MCP server is
        // what `init` repairs; a missing plugin alone needs the agent's own
        // plugin commands and network access, since rerunning `init` offline
        // would only fail the same way.
        let mcp = self
            .summary
            .clients
            .iter()
            .any(|client| client.missing.contains(&Component::Mcp));
        let plugins: Vec<_> = self
            .summary
            .clients
            .iter()
            .filter(|client| client.missing == [Component::Plugin])
            .map(|client| client.agent.install_plugin())
            .collect();
        if mcp {
            Some("mash init".to_owned())
        } else if !plugins.is_empty() {
            Some(format!(
                "{} (needs network access; the plugin adds the history skill and session hooks, and the Mash MCP tools are available without it)",
                plugins.join("; ")
            ))
        } else if self.summary.attention > 0 {
            Some("mash status --installation".to_owned())
        } else if self.summary.unverified > 0 {
            Some("mash status --installation --probe".to_owned())
        } else {
            None
        }
    }

    /// An expected agent (named with `--agent`, recorded by `mash init` or
    /// configured for Mash) needs attention; the command then exits 1.
    pub(crate) fn agents_need_attention(&self) -> bool {
        self.summary
            .clients
            .iter()
            .any(|client| client.expected && client.attention > 0)
    }
}

fn install_cli(agent: Agent) -> String {
    let command = agent.program();
    format!(
        "Install the {command} CLI and make `{command}` available on PATH, then run `mash init`. CLI search and the dashboard work without this integration."
    )
}

impl Client {
    /// One agent's repair: its CLI first, then its MCP registration, then
    /// the plugin, then anything else needing attention.
    fn next_action(&self, missing: &[Component]) -> Option<String> {
        let attention = |component: Component| {
            self.findings
                .iter()
                .find(|finding| finding.component == component && finding.state.needs_attention())
        };
        if self.cli_path.is_none()
            && (attention(Component::Mcp).is_some() || attention(Component::Plugin).is_some())
        {
            return Some(install_cli(self.agent));
        }
        if let Some(remedy) = attention(Component::Mcp).and_then(|f| f.remediation.clone()) {
            return Some(remedy);
        }
        if missing == [Component::Plugin] {
            return Some(plugin::remedy(self.agent));
        }
        self.findings
            .iter()
            .filter(|finding| {
                (self.expected || finding.component == Component::Configuration)
                    && finding.state.needs_attention()
            })
            .find_map(|finding| finding.remediation.clone())
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

/// One machine's agent integrations for a dashboard, step by step. A peer
/// dashboard sends this to the one asking, so local paths are left out.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Setup {
    /// The build of the Mash binary that inspected the machine.
    pub build: String,
    pub agents: Vec<AgentSetup>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AgentSetup {
    pub(crate) agent: Agent,
    /// The machine has this agent's data or CLI, so an integration is expected.
    pub(crate) expected: bool,
    /// Expected, with nothing needing attention.
    pub(crate) connected: bool,
    pub(crate) steps: Vec<Step>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub(crate) struct Step {
    pub(crate) component: Component,
    pub(crate) state: State,
    pub(crate) message: String,
    pub(crate) remediation: Option<String>,
}

/// Inspect this machine's agent integrations without launching anything.
pub fn setup(paths: &super::Paths) -> Setup {
    let report = collect(
        Environment::discover().as_ref(),
        &[],
        &recorded_agents(paths),
    );
    let agents = report
        .clients
        .iter()
        .zip(&report.summary.clients)
        .map(|(client, summary)| AgentSetup {
            agent: client.agent,
            expected: client.expected,
            connected: summary.connection == Connection::Full,
            steps: client
                .findings
                .iter()
                // Project overrides depend on the directory an agent starts
                // in; a dashboard's own directory says nothing about them.
                .filter(|finding| !matches!(finding.component, Component::Configuration))
                .map(|finding| Step {
                    component: finding.component,
                    state: finding.state,
                    message: finding.message.clone(),
                    remediation: finding.remediation.clone(),
                })
                .collect(),
        })
        .collect();
    Setup {
        build: env!("MASH_BUILD_REVISION").to_owned(),
        agents,
    }
}

/// The agents `mash init` recorded in the configuration; none when it
/// cannot be loaded, which the resources section reports.
fn recorded_agents(paths: &super::Paths) -> Vec<Agent> {
    crate::config::load(paths.config.as_deref()).map_or_default(|config| config.connected_agents)
}

fn collect(environment: Option<&Environment>, agents: &[Agent], recorded: &[Agent]) -> Report {
    let mut report = Report {
        summary: Summary {
            attention: 0,
            unverified: 0,
            unconfigured: 0,
            clients: vec![],
            next_action: None,
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
                    recorded.contains(&agent),
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

/// The Mash plugin is installed, current and enabled for `agent`, as
/// `mash status --installation` would report it.
pub(super) fn plugin_current(agent: Agent) -> bool {
    Environment::discover().is_some_and(|environment| {
        clients::inspect(&environment, agent, true, false)
            .findings
            .iter()
            .all(|finding| {
                !matches!(
                    finding.component,
                    Component::Plugin | Component::Skill | Component::Hooks
                ) || !(finding.state.needs_attention() || finding.state == State::Unverified)
            })
    })
}

/// The user-level Mash MCP registration `mash init` finds for an agent.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Registration {
    /// This Mash binary.
    Current,
    /// Its command is missing, not executable, or another binary.
    Stale { command: String },
    /// Absent from user settings, or not resolvable without the agent's own
    /// environment expansion; `init` leaves it as it is.
    Unknown,
}

pub(super) fn registration(agent: Agent) -> Registration {
    Environment::discover().map_or(Registration::Unknown, |environment| {
        clients::registration(&environment, agent)
    })
}

/// The files an agent's own CLI edits when `init` registers Mash with it.
pub(super) fn edited_files(agent: Agent) -> Vec<PathBuf> {
    Environment::discover().map_or_default(|environment| clients::edited_files(&environment, agent))
}

pub(super) fn summary(recorded: &[Agent]) -> Summary {
    collect(Environment::discover().as_ref(), &[], recorded).summary
}

pub fn help_notice(paths: &super::Paths) -> Option<&'static str> {
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
    let summary = summary(&recorded_agents(paths));
    if summary.attention > 0 {
        Some(
            "Installation warning: some Mash agent integrations are incomplete. Run `mash status --installation` for findings and repair steps.",
        )
    } else if summary.unconfigured > 0 {
        Some(
            "Installation notice: agent data exists without a configured Mash integration. CLI-only use is valid; run `mash status --installation` for setup details.",
        )
    } else {
        None
    }
}

pub(super) async fn inspect(
    agents: Vec<Agent>,
    probe: Option<String>,
    paths: super::Paths,
) -> Result<Report, crate::AppError> {
    let (mut report, qdrant) = crate::blocking(move || {
        let config = crate::config::load(paths.config.as_deref()).ok();
        let recorded = config
            .as_ref()
            .map_or(&[][..], |config| config.connected_agents.as_slice());
        let mut report = collect(Environment::discover().as_ref(), &agents, recorded);
        report.resources = Some(resources::inspect(&paths));
        Ok((report, config.map(|config| config.qdrant)))
    })
    .await?;
    if let (Some(config), Some(resources)) = (qdrant, &mut report.resources) {
        let observed = crate::views::qdrant::reachability(&config).await;
        resources.add_qdrant(&observed);
    }
    report.update_summary();
    if let Some(contract) = &probe {
        for client in &mut report.clients {
            probe::check(client, contract).await;
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
        "../../plugins/mash/skills/mash-history/contract.json"
    ))
    .unwrap_or(Stamp {
        contract_fingerprint: String::new(),
        plugin_version: String::new(),
    })
}

#[cfg(test)]
mod tests;
