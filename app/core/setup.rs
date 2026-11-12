//! `mash init`: write a configuration for the histories on this machine,
//! register Mash with installed agents through their own CLIs, then index.
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use super::models::{IndexMode, IndexRequest, Operation, Paths, Response};
use crate::AppError;

pub(super) mod backfill;

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
    /// Explicit wizard selections; absent for unattended setup.
    pub(crate) plan: Option<InitPlan>,
    pub(crate) selected_agents: Option<Vec<super::installation::Agent>>,
    pub(crate) backfill_memory: Option<super::installation::Agent>,
}

#[derive(Debug, Clone)]
pub(crate) struct InitPlan {
    pub(crate) choices: crate::config::init::Choices,
    /// Compare before writing so a config edited while prompting is not replaced.
    pub(crate) previous: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct InitResponse {
    config: ConfigOutcome,
    agents: Vec<AgentOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::core) index: Option<Box<Response>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::core) memory: Option<backfill::Outcome>,
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
    let config = if config_path.exists() && !request.force && request.plan.is_none() {
        ConfigOutcome {
            path: config_path,
            written: false,
            providers: None,
            search: None,
            previous: None,
        }
    } else {
        let semantic = crate::semantic::Models::new(models).is_installed();
        let editing = request
            .plan
            .as_ref()
            .is_some_and(|plan| plan.previous.is_some());
        let path = config_path.clone();
        let (providers, previous) =
            crate::blocking(move || write_configuration(&path, semantic, request.plan.as_ref()))
                .await?;
        if !semantic {
            next.push("For semantic search, run `mash models install`, then update your existing configuration as shown at https://mashapp.dev/indexing/#enable-semantic-search".to_owned());
        }
        ConfigOutcome {
            path: config_path,
            written: true,
            providers: Some(providers),
            search: (!editing).then_some(if semantic {
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
        register(
            &command,
            &request.marketplace,
            request.selected_agents.as_deref(),
        )
        .await
    } else {
        Vec::new()
    };
    if agents.is_empty() && request.agents {
        next.push("No Claude Code or Codex CLI was found on PATH. CLI search and the dashboard still work; to connect an agent, see https://mashapp.dev/agents/".to_owned());
    }
    next.extend(agent_next_steps(&agents));
    if !request.index {
        next.push("Index your history: mash index".to_owned());
    }
    Ok(InitResponse {
        config,
        agents,
        index: None,
        memory: None,
        next,
    })
}

fn write_configuration(
    path: &Path,
    semantic: bool,
    plan: Option<&InitPlan>,
) -> Result<(crate::provider::ProviderRoots, Option<PathBuf>), AppError> {
    let providers = match plan {
        Some(plan) => plan.choices.providers.clone(),
        None => crate::config::init::detect()?,
    };
    if providers.is_empty() {
        return Err(AppError::configuration(format!(
            "no supported agent history was found in its default location; write {} by hand (see the README's Configuration section)",
            path.display()
        )));
    }
    let text = if let Some(plan) = plan {
        let current = match std::fs::read_to_string(path) {
            Ok(text) => Some(text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(AppError::io(error)),
        };
        if current != plan.previous {
            return Err(AppError::configuration(
                "configuration changed during setup; run `mash init` again",
            ));
        }
        let base = match &plan.previous {
            Some(text) => text.clone(),
            None => crate::config::init::render(&providers, semantic)?,
        };
        crate::config::init::configure(&base, &plan.choices)?
    } else {
        crate::config::init::render(&providers, semantic)?
    };
    let previous = crate::config::init::write(path, &text)?;
    Ok((providers, previous))
}

/// What each agent still needs after registration, and whether to restart.
fn agent_next_steps(agents: &[AgentOutcome]) -> Vec<String> {
    let mut steps = Vec::new();
    for agent in agents {
        if let Step::Failed { command, .. } = &agent.mcp {
            steps.push(format!(
            "{}: the Mash MCP server was not registered, so this agent cannot search history yet; fix the error above, then rerun `mash init` or run `{command}`.",
            agent.agent
        ));
        } else if let Step::Failed { command, .. } = &agent.plugin {
            steps.push(format!(
            "{}: Mash MCP search works, but the mash@mash plugin (history skill and session hooks) was not installed; when online, rerun `mash init` or run `{command}`.",
            agent.agent
        ));
        }
    }
    if agents
        .iter()
        .any(|agent| !matches!(agent.mcp, Step::Failed { .. }))
    {
        steps.push("Restart your agents so they load Mash.".to_owned());
    }
    steps
}

/// The first index: `mash index` with the configured defaults.
pub(super) fn first_index() -> Operation {
    Operation::Index(IndexRequest {
        mode: IndexMode::from_rebuild_flag(false),
        full: false,
        provider: Vec::new(),
        since_days: None,
        all_history: false,
        reading: crate::core::Reading::Changed,
        defer_publication: false,
        backfill: None,
    })
}

/// Register the MCP server and plugin with each agent CLI found on PATH. A
/// failure is reported per step; it never stops the others.
async fn register(
    command: &[OsString],
    marketplace: &str,
    selected: Option<&[super::installation::Agent]>,
) -> Vec<AgentOutcome> {
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
        if selected.is_some_and(|agents| !agents.contains(&kind)) {
            continue;
        }
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

pub(crate) fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    crate::platform::find_program(program, &path, &std::env::current_dir().ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Provider, ProviderRoots};

    #[test]
    fn setup_wizard_refuses_changed_config_and_preserves_backup() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config.toml");
        let providers = ProviderRoots::from([(Provider::Codex, vec![root.path().to_owned()])]);
        let original = crate::config::init::render(&providers, false).unwrap();
        std::fs::write(&path, &original).unwrap();
        let plan = InitPlan {
            choices: crate::config::init::Choices {
                records: crate::records::capture::Policy::default(),
                providers,
                since_days: Some(14),
                shared: None,
            },
            previous: Some(original.clone()),
        };
        std::fs::write(&path, "# concurrent edit\n").unwrap();
        assert!(write_configuration(&path, false, Some(&plan)).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# concurrent edit\n"
        );
        assert!(!root.path().join("config.toml.previous").exists());
        std::fs::write(&path, &original).unwrap();
        write_configuration(&path, false, Some(&plan)).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.path().join("config.toml.previous")).unwrap(),
            original
        );
        assert_eq!(
            crate::config::load(Some(&path)).unwrap().since_days,
            Some(14)
        );
    }
}
