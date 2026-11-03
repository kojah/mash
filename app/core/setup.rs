//! `mash init`: write a configuration for the histories on this machine,
//! register Mash with installed agents through their own CLIs, then index.
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use super::models::{IndexMode, IndexRequest, Operation, Paths, Response};
use crate::AppError;

/// The plugin marketplace that `init` adds to agents unless told otherwise.
pub(crate) const MARKETPLACE: &str = "kojah/mash";
const AGENT_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
pub(crate) struct InitRequest {
    /// Replace an existing configuration (kept as `config.toml.previous`).
    pub(crate) force: bool,
    pub(crate) agents: bool,
    pub(crate) index: bool,
    pub(crate) marketplace: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct InitResponse {
    config: ConfigOutcome,
    agents: Vec<AgentOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::core) index: Option<Box<Response>>,
    next: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ConfigOutcome {
    path: PathBuf,
    /// False when an existing configuration was kept.
    written: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    providers: Option<crate::provider::ProviderRoots>,
    #[serde(skip_serializing_if = "Option::is_none")]
    search: Option<&'static str>,
    /// Where `--force` kept the configuration it replaced.
    #[serde(skip_serializing_if = "Option::is_none")]
    previous: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
struct AgentOutcome {
    agent: &'static str,
    mcp: Step,
    plugin: Step,
}

#[derive(Debug, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
enum Step {
    Done { command: String },
    AlreadyPresent,
    Failed { command: String, message: String },
}

pub(super) async fn init(paths: Paths, request: InitRequest) -> Result<InitResponse, AppError> {
    let config_path = match &paths.config {
        Some(path) => path.clone(),
        None => crate::config::default_path()?,
    };
    let models = paths
        .models_dir
        .clone()
        .unwrap_or_else(super::default_models_path);
    let mut next = Vec::new();
    let config = if config_path.exists() && !request.force {
        ConfigOutcome {
            path: config_path,
            written: false,
            providers: None,
            search: None,
            previous: None,
        }
    } else {
        let semantic = crate::semantic::Models::new(models).is_installed();
        let path = config_path.clone();
        let (providers, previous) = crate::blocking(move || {
            let providers = crate::config::init::detect()?;
            if providers.is_empty() {
                return Err(AppError::configuration(format!(
                    "no supported agent history was found in its default location; write {} by hand (see the README's Configuration section)",
                    path.display()
                )));
            }
            let previous = crate::config::init::write(
                &path,
                &crate::config::init::render(&providers, semantic)?,
            )?;
            Ok((providers, previous))
        })
        .await?;
        if !semantic {
            next.push("For semantic search: mash models install && mash init --force".to_owned());
        }
        ConfigOutcome {
            path: config_path,
            written: true,
            providers: Some(providers),
            search: Some(if semantic {
                "semantic"
            } else {
                "exact-mention"
            }),
            previous,
        }
    };
    let agents = if request.agents {
        let exe = std::env::current_exe().map_err(AppError::io)?;
        let mut command = vec![exe.into_os_string()];
        for (flag, value) in [
            ("--config", &paths.config),
            ("--db", &paths.db),
            ("--models-dir", &paths.models_dir),
        ] {
            if let Some(value) = value {
                command.push(flag.into());
                command.push(std::path::absolute(value).map_err(AppError::io)?.into());
            }
        }
        command.push("mcp".into());
        register(&command, &request.marketplace).await
    } else {
        Vec::new()
    };
    if agents.is_empty() && request.agents {
        next.push("No Claude Code or Codex CLI was found on PATH; see the README to connect another agent.".to_owned());
    } else if !agents.is_empty() {
        next.push("Restart your agents so they load Mash.".to_owned());
    }
    if !request.index {
        next.push("Index your history: mash index".to_owned());
    }
    Ok(InitResponse {
        config,
        agents,
        index: None,
        next,
    })
}

/// The first index: `mash index` with the configured defaults.
pub(super) fn first_index() -> Operation {
    Operation::Index(IndexRequest {
        mode: IndexMode::from_rebuild_flag(false),
        full: false,
        provider: Vec::new(),
        since_days: None,
        all_history: false,
        defer_publication: false,
        backfill: None,
    })
}

/// Register the MCP server and plugin with each agent CLI found on PATH. A
/// failure is reported per step; it never stops the others.
async fn register(command: &[OsString], marketplace: &str) -> Vec<AgentOutcome> {
    let mut outcomes = Vec::new();
    for (agent, kind, name, install) in [
        (
            "claude-code",
            super::installation::Agent::ClaudeCode,
            "claude",
            "install",
        ),
        ("codex", super::installation::Agent::Codex, "codex", "add"),
    ] {
        let Some(program) = which(name) else {
            continue;
        };
        let program = program.as_path();
        let mcp = if run(program, &["mcp", "get", "mash"]).await.is_ok() {
            Step::AlreadyPresent
        } else {
            let mut arguments: Vec<OsString> = ["mcp", "add"].map(OsString::from).to_vec();
            if name == "claude" {
                arguments.extend(["--scope", "user"].map(OsString::from));
            }
            arguments.extend(["mash", "--"].map(OsString::from));
            arguments.extend(command.iter().cloned());
            step(program, &arguments).await
        };
        let plugin = if crate::blocking(move || Ok(super::installation::plugin_current(kind)))
            .await
            .unwrap_or(false)
        {
            Step::AlreadyPresent
        } else {
            // An existing marketplace makes `add` fail; installing then still
            // works. When installing fails too, the marketplace error is
            // usually the cause (no network, for example), so report both.
            let added = run(program, &["plugin", "marketplace", "add", marketplace]).await;
            match step(
                program,
                &["plugin", install, "mash@mash"].map(OsString::from),
            )
            .await
            {
                Step::Failed { command, message } => Step::Failed {
                    command,
                    message: match added {
                        Err(cause) => format!(
                            "{message} (adding the {marketplace} marketplace failed first: {cause})"
                        ),
                        Ok(()) => message,
                    },
                },
                done => done,
            }
        };
        outcomes.push(AgentOutcome { agent, mcp, plugin });
    }
    outcomes
}

async fn step(program: &Path, arguments: &[OsString]) -> Step {
    let command = std::iter::once(program.display().to_string())
        .chain(arguments.iter().map(|a| a.to_string_lossy().into_owned()))
        .collect::<Vec<_>>()
        .join(" ");
    match run(program, arguments).await {
        Ok(()) => Step::Done { command },
        Err(message) => Step::Failed { command, message },
    }
}

async fn run(program: &Path, arguments: &[impl AsRef<std::ffi::OsStr>]) -> Result<(), String> {
    let output = tokio::process::Command::new(program)
        .args(arguments)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    match tokio::time::timeout(AGENT_TIMEOUT, output).await {
        Err(_) => Err(format!(
            "timed out after {} seconds",
            AGENT_TIMEOUT.as_secs()
        )),
        Ok(Err(error)) => Err(error.to_string()),
        Ok(Ok(output)) if output.status.success() => Ok(()),
        Ok(Ok(output)) => {
            let text = String::from_utf8_lossy(&output.stderr);
            let text = if text.trim().is_empty() {
                String::from_utf8_lossy(&output.stdout)
            } else {
                text
            };
            Err(text.trim().chars().take(500).collect())
        }
    }
}

fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    crate::platform::find_program(program, &path, &std::env::current_dir().ok()?)
}
