//! Inspect known user settings. Project overrides are surfaced rather than treated as active
//! without the host's project-trust/profile/policy resolution.
use super::{Agent, Client, Component, Environment, Expectation, State, executable, plugin, read};
use serde::Deserialize;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct Server {
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub env_vars: Vec<String>,
    pub enabled: Option<bool>,
    pub disabled: Option<bool>,
    pub url: Option<String>,
    #[serde(rename = "type")]
    pub transport: Option<String>,
    pub cwd: Option<PathBuf>,
}

#[derive(Debug)]
pub(super) struct Launch {
    pub server: Server,
    pub executable: PathBuf,
    pub cwd: PathBuf,
    pub source: PathBuf,
}

#[derive(Default, Deserialize)]
struct Codex {
    #[serde(default)]
    plugins: BTreeMap<String, Enabled>,
    #[serde(default)]
    mcp_servers: BTreeMap<String, Server>,
    #[serde(default)]
    hooks: Hooks,
}

#[derive(Default, Deserialize)]
pub(super) struct Hooks {
    #[serde(default)]
    pub state: BTreeMap<String, HookState>,
}

#[derive(Deserialize)]
pub(super) struct HookState {
    pub enabled: Option<bool>,
    pub trusted_hash: Option<String>,
}

#[derive(Deserialize)]
struct Enabled {
    enabled: Option<bool>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Claude {
    #[serde(default)]
    enabled_plugins: BTreeMap<String, bool>,
    disable_all_hooks: Option<bool>,
    #[serde(default)]
    disabled_mcp_servers: Vec<String>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeMcp {
    #[serde(default)]
    mcp_servers: BTreeMap<String, Server>,
    #[serde(default)]
    projects: BTreeMap<PathBuf, Project>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Project {
    #[serde(default)]
    mcp_servers: BTreeMap<String, Server>,
    #[serde(default)]
    disabled_mcp_servers: Vec<String>,
}

fn settings<T: serde::de::DeserializeOwned + Default>(
    client: &mut Client,
    path: &std::path::Path,
) -> T {
    match read(path) {
        Ok(value) => value.unwrap_or_default(),
        Err(message) => {
            client.add(
                Component::Configuration,
                State::Invalid,
                Some(path),
                message,
                Some("Repair the agent settings file, then rerun mash status --installation."),
            );
            T::default()
        }
    }
}

pub(super) fn inspect(
    environment: &Environment,
    agent: Agent,
    explicitly_expected: bool,
    recorded: bool,
) -> Client {
    let root = match agent {
        Agent::Codex => &environment.codex,
        Agent::ClaudeCode => &environment.claude,
    };
    let mut client = Client {
        agent,
        data_directory_found: root.exists(),
        cli_path: executable(agent.program(), environment),
        expected: explicitly_expected || recorded,
        expectation: if explicitly_expected {
            Expectation::Named
        } else if recorded {
            Expectation::Recorded
        } else {
            Expectation::None
        },
        findings: vec![],
        launch: None,
        probe_skipped: None,
    };
    match agent {
        Agent::Codex => codex(&mut client, environment),
        Agent::ClaudeCode => claude(&mut client, environment),
    }
    if client.cli_path.is_none() {
        for finding in &mut client.findings {
            if matches!(finding.component, Component::Mcp | Component::Plugin)
                && finding.state.needs_attention()
            {
                finding.remediation = finding.remediation.take().map(|remedy| {
                    format!("The agent CLI was not found on PATH. Install it or fix PATH first. {remedy}")
                });
            }
        }
    }
    if client.expected && client.expectation == Expectation::None {
        client.expectation = Expectation::Configured;
    }
    if !client.expected {
        client.add(
            Component::Configuration,
            State::NotConfigured,
            Some(root),
            "No enabled Mash integration was found in user settings, and `mash init` did not record connecting this agent; CLI-only use is valid.",
            Some(&format!(
                "To require this agent, run `mash status --installation --agent {}`.",
                agent.as_str()
            )),
        );
    }
    client
}

fn codex(client: &mut Client, environment: &Environment) {
    let path = environment.codex.join("config.toml");
    let config: Codex = settings(client, &path);
    let plugins: Vec<_> = config
        .plugins
        .iter()
        .filter(|(id, _)| id.starts_with("mash@"))
        .collect();
    client.expected |= config.mcp_servers.contains_key("mash")
        || plugins
            .iter()
            .any(|(_, setting)| setting.enabled == Some(true));
    if plugins.is_empty() {
        plugin::missing(client, &path);
    }
    for (id, setting) in plugins {
        if setting.enabled != Some(true) {
            client.add(
                Component::Plugin,
                State::Disabled,
                Some(&path),
                "Mash plugin is not explicitly enabled in user settings.",
                Some("Enable the Mash plugin in Codex's plugin settings."),
            );
            continue;
        }
        let marketplace = id.strip_prefix("mash@").unwrap_or_default();
        if marketplace.is_empty() || marketplace.contains(['/', '\\']) || marketplace == ".." {
            client.add(
                Component::Plugin,
                State::Invalid,
                Some(&path),
                "Invalid Mash marketplace identifier.",
                Some("Reinstall the Mash plugin through the client."),
            );
            continue;
        }
        let cache = environment
            .codex
            .join("plugins/cache")
            .join(marketplace)
            .join("mash");
        let roots = plugin::cached_roots(client, &cache);
        match roots.as_slice() {
            [root] => plugin::inspect(client, root, environment, Some((id, &config.hooks))),
            [] => plugin::missing(client, &cache),
            _ => client.add(Component::Plugin, State::Unverified, Some(&cache), "Multiple cached versions exist; the active copy cannot be inferred from directory order.", Some("Inspect the installed Mash version in Codex's plugin settings and refresh it.")),
        }
    }
    inspect_server(client, config.mcp_servers.get("mash"), &path, environment);
    // Codex project layers stop at the repository root. Outside a repository inspect
    // only cwd; unrelated ancestor directories must not supply project settings.
    let root = environment
        .cwd
        .ancestors()
        .take_while(|path| {
            !environment.cwd.starts_with(&environment.home) || path.starts_with(&environment.home)
        })
        .find(|path| path.join(".git").exists())
        .unwrap_or(&environment.cwd);
    for directory in environment
        .cwd
        .ancestors()
        .take_while(|path| path.starts_with(root))
    {
        let path = directory.join(".codex/config.toml");
        if path == environment.codex.join("config.toml") {
            continue;
        }
        match read::<Codex>(&path) {
            Ok(Some(config))
                if config.mcp_servers.contains_key("mash")
                    || config.plugins.keys().any(|key| key.starts_with("mash@"))
                    || !config.hooks.state.is_empty() =>
            {
                project_override(client, &path);
            }
            Err(message) => {
                client.add(
                    Component::Configuration,
                    State::Unverified,
                    Some(&path),
                    message,
                    Some("Check project settings and trust in Codex."),
                );
                client.launch = None;
            }
            _ => {}
        }
    }
}

/// Where Claude Code keeps user-level MCP registrations.
pub(super) fn claude_mcp_path(environment: &Environment) -> PathBuf {
    if environment.claude == environment.home.join(".claude") {
        environment.home.join(".claude.json")
    } else {
        environment.claude.join(".claude.json")
    }
}

/// The user settings files and plugin directories an agent's own CLI edits
/// when `mash init` registers Mash with it.
pub(super) fn edited_files(environment: &Environment, agent: Agent) -> Vec<PathBuf> {
    match agent {
        Agent::Codex => vec![
            environment.codex.join("config.toml"),
            environment.codex.join("plugins"),
        ],
        Agent::ClaudeCode => vec![
            claude_mcp_path(environment),
            environment.claude.join("settings.json"),
            environment.claude.join("plugins"),
        ],
    }
}

/// The user-level Mash registration `mash init` would leave in place, or
/// the command of one it must replace.
pub(super) fn registration(environment: &Environment, agent: Agent) -> super::Registration {
    let server = match agent {
        Agent::Codex => read::<Codex>(&environment.codex.join("config.toml"))
            .ok()
            .flatten()
            .and_then(|config| config.mcp_servers.get("mash").cloned()),
        Agent::ClaudeCode => read::<ClaudeMcp>(&claude_mcp_path(environment))
            .ok()
            .flatten()
            .and_then(|config| config.mcp_servers.get("mash").cloned()),
    };
    let Some(server) = server else {
        return super::Registration::Unknown;
    };
    if server.url.is_some()
        || server
            .transport
            .as_deref()
            .is_some_and(|value| value != "stdio")
        || server.cwd.is_some()
        || server
            .command
            .as_deref()
            .is_some_and(|command| command.contains('$'))
    {
        return super::Registration::Unknown;
    }
    let command = server.command.clone().unwrap_or_default();
    let launch_environment = Environment {
        path: server
            .env
            .get("PATH")
            .map_or_else(|| environment.path.clone(), Into::into),
        ..environment.clone()
    };
    let current = std::env::current_exe().and_then(std::fs::canonicalize).ok();
    let registered = (!command.is_empty())
        .then(|| executable(&command, &launch_environment))
        .flatten()
        .and_then(|path| std::fs::canonicalize(path).ok());
    match (registered, current) {
        (Some(registered), Some(current)) if registered == current => super::Registration::Current,
        // This binary's own path is unknown, so any working command stays.
        (Some(_), None) => super::Registration::Unknown,
        _ => super::Registration::Stale { command },
    }
}

fn claude(client: &mut Client, environment: &Environment) {
    let path = environment.claude.join("settings.json");
    let config: Claude = settings(client, &path);
    let mcp_path = claude_mcp_path(environment);
    let mcp: ClaudeMcp = settings(client, &mcp_path);
    let plugins: Vec<_> = config
        .enabled_plugins
        .iter()
        .filter(|(id, _)| id.starts_with("mash@"))
        .collect();
    client.expected |=
        mcp.mcp_servers.contains_key("mash") || plugins.iter().any(|(_, enabled)| **enabled);
    if plugins.is_empty() {
        plugin::missing(client, &path);
    }
    for (id, enabled) in plugins {
        if *enabled {
            plugin::claude_install(client, id, environment);
        } else {
            client.add(
                Component::Plugin,
                State::Disabled,
                Some(&path),
                "Mash plugin is disabled.",
                Some("Enable mash@mash in Claude Code's plugin settings, or run `mash init`."),
            );
        }
    }
    if config.disable_all_hooks == Some(true) {
        client.add(
            Component::Hooks,
            State::Disabled,
            Some(&path),
            "Claude Code disables all hooks in user settings.",
            Some("Review disableAllHooks in Claude Code settings."),
        );
    }
    if config
        .disabled_mcp_servers
        .iter()
        .any(|name| name == "mash")
    {
        client.add(
            Component::Mcp,
            State::Disabled,
            Some(&path),
            "Mash MCP is disabled in user settings.",
            Some("Enable Mash in Claude Code's MCP settings."),
        );
    } else {
        inspect_server(client, mcp.mcp_servers.get("mash"), &mcp_path, environment);
    }
    if mcp.projects.get(&environment.cwd).is_some_and(|project| {
        project.mcp_servers.contains_key("mash")
            || project
                .disabled_mcp_servers
                .iter()
                .any(|name| name == "mash")
    }) {
        project_override(client, &mcp_path);
    }
    for relative in [
        ".mcp.json",
        ".claude/settings.json",
        ".claude/settings.local.json",
    ] {
        let path = environment.cwd.join(relative);
        if path != environment.claude.join("settings.json") && path.exists() {
            project_override(client, &path);
        }
    }
}

fn project_override(client: &mut Client, path: &std::path::Path) {
    client.add(Component::Configuration, State::Unverified, Some(path),
        "Project settings may override this user-level inspection; effective client trust and settings are unverified. MCP probe skipped.",
        Some("Review Mash in the agent's project plugin/MCP settings, or run this check outside the project to inspect the user installation."));
    client.launch = None;
    client.probe_skipped =
        Some("Skipped: project settings may override the user-level Mash MCP registration.");
}

fn inspect_server(
    client: &mut Client,
    server: Option<&Server>,
    path: &std::path::Path,
    environment: &Environment,
) {
    let current = std::env::current_exe()
        .map_or_else(|_| "mash".to_owned(), |path| path.display().to_string());
    let register = client.agent.add_mcp(&current);
    let Some(server) = server else {
        missing_registration(client, path, &register);
        return;
    };
    if server.enabled == Some(false) || server.disabled == Some(true) {
        client.add(
            Component::Mcp,
            State::Disabled,
            Some(path),
            "Mash MCP registration is disabled.",
            Some("Enable the Mash MCP registration in the client."),
        );
        return;
    }
    if server.url.is_some()
        || server
            .transport
            .as_deref()
            .is_some_and(|value| value != "stdio")
    {
        client.add(
            Component::Mcp,
            State::Unverified,
            Some(path),
            "Only local stdio Mash MCP registrations can be checked.",
            Some("Inspect this transport in the client; Mash's MCP server uses stdio."),
        );
        return;
    }
    let Some(command) = server.command.as_deref().filter(|value| !value.is_empty()) else {
        client.add(
            Component::Mcp,
            State::Invalid,
            Some(path),
            "Mash MCP registration has no executable.",
            Some("Set the command to the installed Mash executable or private launcher."),
        );
        return;
    };
    if server.args.iter().any(|arg| {
        arg == "--watch"
            || arg.starts_with("--watch=")
            || arg == "--debounce-ms"
            || arg.starts_with("--debounce-ms=")
    }) {
        client.add(Component::Mcp, State::Outdated, Some(path), "Mash MCP registration contains retired watch arguments.", Some("Remove --watch and --debounce-ms from the MCP registration and reconnect. Run `mash index --watch` separately for automatic ingestion."));
        return;
    }
    if command.contains('$')
        || server.args.iter().any(|arg| arg.contains("${"))
        || server.env.values().any(|value| value.contains("${"))
        || server.cwd.is_some()
    {
        client.add(Component::Mcp, State::Unverified, Some(path), "Registration requires client-specific environment expansion or working-directory resolution; probe skipped.", Some("Verify this registration with the agent's MCP connection controls."));
        return;
    }
    let launch_environment = Environment {
        path: server
            .env
            .get("PATH")
            .map_or_else(|| environment.path.clone(), Into::into),
        ..environment.clone()
    };
    let Some(executable) = executable(command, &launch_environment) else {
        missing_executable(client, path, command, &register);
        return;
    };
    client.add(
        Component::Mcp,
        State::Present,
        Some(path),
        "Enabled stdio registration and executable found; arguments and handshake have not been tested.",
        Some(&format!(
            "Run `mash status --installation --agent {} --probe` to test a fresh MCP process.",
            client.agent.as_str()
        )),
    );
    client.launch = Some(Launch {
        server: server.clone(),
        executable,
        cwd: environment.cwd.clone(),
        source: path.to_path_buf(),
    });
}

fn missing_registration(client: &mut Client, path: &std::path::Path, register: &str) {
    client.probe_skipped = Some("Skipped: there is no user-level Mash MCP registration.");
    if client.expectation == Expectation::Recorded {
        client.add(
            Component::Mcp,
            State::Missing,
            Some(path),
            "`mash init` connected this agent, but its user-level Mash MCP registration is missing.",
            Some(&format!(
                "Run `{register}`, then restart the agent."
            )),
        );
    } else {
        client.add(
            Component::Mcp,
            State::Missing,
            Some(path),
            "No user-level Mash MCP registration found.",
            Some(&format!(
                "Run `mash init`, or register it yourself: `{register}`. Then restart the agent."
            )),
        );
    }
}

fn missing_executable(client: &mut Client, path: &std::path::Path, command: &str, register: &str) {
    client.probe_skipped = Some("Skipped: the registered MCP executable does not exist.");
    client.add(
        Component::Mcp,
        State::Missing,
        Some(path),
        &format!(
            "Registered MCP command {command} does not exist or is not executable on this CLI's PATH."
        ),
        Some(&format!(
            "Run `{} && {register}`, then restart the agent. Rerunning `mash init` also replaces it.",
            client.agent.remove_mcp()
        )),
    );
}
