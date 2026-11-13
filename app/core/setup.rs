//! `mash init`: write a configuration for the histories on this machine,
//! register Mash with installed agents through their own CLIs, then index.
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use super::installation::{Agent, Registration};
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
    /// Why the first index failed. The configuration and agent steps above
    /// were kept; the same error goes to stderr and sets the exit status.
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "error_body")]
    pub(in crate::core) index_error: Option<AppError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::core) memory: Option<backfill::Outcome>,
    next: Vec<String>,
}

#[expect(clippy::ref_option, reason = "serde's serialize_with signature")]
fn error_body<S: serde::Serializer>(
    error: &Option<AppError>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    error
        .as_ref()
        .map(|error| &error.error)
        .serialize(serializer)
}

impl InitResponse {
    /// An agent's registration names a missing or different executable and
    /// could not be replaced, so `init` exits 1.
    pub(in crate::core) fn left_stale_registration(&self) -> bool {
        self.agents
            .iter()
            .any(|agent| matches!(agent.mcp, Step::Stale { .. }))
    }

    /// Keep the setup report when the first index fails, and say what is
    /// left: setup is saved, so `mash index` finishes it.
    pub(in crate::core) fn index_failed(&mut self, error: AppError) {
        let step = if error.error.kind == crate::ErrorKind::QdrantUnavailable {
            "Setup was saved, but the first index stopped because Qdrant did not answer (see index_error for how to start it). Start Qdrant, then run `mash index` to finish."
        } else {
            "Setup was saved, but the first index failed (see index_error). Fix that, then run `mash index` to finish."
        };
        self.next.insert(0, step.to_owned());
        self.index_error = Some(error);
    }
}

/// Indexing needs Qdrant, so check it before `init` changes anything: a
/// refused check leaves configuration and agent settings as they were.
pub(super) async fn preflight(paths: &Paths, request: &InitRequest) -> Result<(), AppError> {
    if !request.index {
        return Ok(());
    }
    let config_path = match &paths.config {
        Some(path) => path.clone(),
        None => crate::config::default_path()?,
    };
    // A kept or reviewed configuration keeps its Qdrant settings; a newly
    // written one uses the defaults.
    let kept = config_path.exists()
        && request
            .plan
            .as_ref()
            .map_or(!request.force, |plan| plan.previous.is_some());
    let qdrant = if kept {
        crate::blocking(move || crate::config::load(Some(&config_path)))
            .await?
            .qdrant
    } else {
        crate::config::QdrantConfig::default()
    };
    crate::views::qdrant::check(&qdrant).await.map_err(|mut error| {
        let rerun = "rerun `mash init`";
        let unavailable = (error.error.kind == crate::ErrorKind::QdrantUnavailable)
            .then(|| {
                crate::views::qdrant::unavailable_message(&qdrant.url, Some(BROWSE_NOW), rerun)
            })
            .flatten();
        error.error.message = match unavailable {
            Some(message) => {
                error.error.recommended_action = Some(crate::Action::StartQdrant);
                format!("`mash init` changed nothing. {message}")
            }
            None => format!(
                "`mash init` changed nothing: indexing needs the local Qdrant service. {} Then {rerun}.",
                error.error.message
            ),
        };
        error
    })
}

/// How to read history before Qdrant runs: indexing saves it first.
const BROWSE_NOW: &str = "To read your chats before Qdrant runs, run `mash init --no-index`, then `mash index`: indexing saves your history before it needs Qdrant, and `mash list-chats` and `mash view <message id>` read it without Qdrant.";

/// The same route after `--no-index` has saved the configuration.
const INDEX_SAVES_HISTORY: &str = "To read your chats now, run `mash index`: it saves your history before it needs Qdrant, and `mash list-chats` and `mash view <message id>` read it without Qdrant.";

/// The `--no-index` step: index now, or start Qdrant first.
fn index_later(observed: Option<crate::views::qdrant::ServiceStatus>) -> String {
    use crate::views::qdrant::Reachability;
    match observed {
        Some(observed) if observed.state == Reachability::Reachable => {
            "Index your history: mash index".to_owned()
        }
        Some(observed) => {
            let unreachable = (observed.state == Reachability::Unreachable)
                .then(|| {
                    crate::views::qdrant::unavailable_message(
                        &observed.url,
                        Some(INDEX_SAVES_HISTORY),
                        "run `mash index` to make search current",
                    )
                })
                .flatten();
            unreachable.unwrap_or_else(|| {
                format!(
                    "Index your history with `mash index` once Qdrant answers at {}; indexing and every search need it. {}",
                    observed.url,
                    observed.message.unwrap_or_default()
                )
            })
        }
        None => "Index your history: mash index".to_owned(),
    }
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
    /// Agents newly recorded under `[agents] connected`, which
    /// `mash status --installation` then expects to stay connected.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    recorded_agents: Vec<Agent>,
}

#[derive(Debug, Serialize)]
struct AgentOutcome {
    agent: Agent,
    mcp: Step,
    plugin: Step,
    /// The settings files and plugin directories the agent's own CLI edits.
    files: Vec<PathBuf>,
    /// The agent CLI commands that remove what `init` added.
    undo: [&'static str; 2],
}

#[derive(Debug, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
enum Step {
    Done {
        command: String,
    },
    AlreadyPresent,
    /// The registration named `stale`, a missing or different executable,
    /// and was removed and added again with this binary.
    Replaced {
        stale: String,
        command: String,
    },
    /// The registration names `stale` and could not be replaced; `command`
    /// replaces it.
    Stale {
        stale: String,
        command: String,
        message: String,
    },
    Failed {
        command: String,
        message: String,
    },
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
    let mut config = if config_path.exists() && !request.force && request.plan.is_none() {
        ConfigOutcome {
            path: config_path,
            written: false,
            providers: None,
            search: None,
            previous: None,
            recorded_agents: Vec::new(),
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
            next.push("For semantic search, run `mash models install`, then update your existing configuration as shown at https://getmash.io/indexing/#enable-semantic-search".to_owned());
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
            recorded_agents: Vec::new(),
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
        next.push("No Claude Code or Codex CLI was found on PATH. CLI search and the dashboard still work; to connect an agent, see https://getmash.io/agents/".to_owned());
    }
    record_agents(&mut config, &agents, &paths, &mut next).await;
    if !request.index {
        // Indexing reports an unreadable configuration itself.
        let path = config.path.clone();
        let observed = match crate::blocking(move || crate::config::load(Some(&path))).await {
            Ok(loaded) => Some(crate::views::qdrant::reachability(&loaded.qdrant).await),
            Err(_) => None,
        };
        next.push(index_later(observed));
    }
    Ok(InitResponse {
        config,
        agents,
        index: None,
        index_error: None,
        memory: None,
        next,
    })
}

/// Record the agents whose MCP registration is in place, then add each
/// agent's next steps and how to undo setup.
async fn record_agents(
    config: &mut ConfigOutcome,
    agents: &[AgentOutcome],
    paths: &Paths,
    next: &mut Vec<String>,
) {
    let connected: Vec<Agent> = agents
        .iter()
        .filter(|agent| {
            matches!(
                agent.mcp,
                Step::Done { .. } | Step::AlreadyPresent | Step::Replaced { .. }
            )
        })
        .map(|agent| agent.agent)
        .collect();
    if !connected.is_empty() {
        let path = config.path.clone();
        let recorded = connected.clone();
        match crate::blocking(move || {
            crate::config::init::record_connected_agents(&path, &recorded)
        })
        .await
        {
            Ok(true) => config.recorded_agents = connected,
            Ok(false) => {}
            Err(error) => next.push(format!(
                "Could not record the connected agents in {}, so `mash status --installation` will not report them as broken if their registration disappears: {}",
                config.path.display(),
                error.error.message
            )),
        }
    }
    next.extend(agent_next_steps(agents));
    if !agents.is_empty() {
        let data = paths
            .db
            .clone()
            .unwrap_or_else(super::default_database_path);
        next.push(format!(
            "To undo agent setup, run each agent's `undo` commands; to remove Mash's own files, also delete {} and {}.",
            config.path.display(),
            data.parent().unwrap_or(&data).display()
        ));
    }
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
        let name = agent.agent.as_str();
        if let Step::Failed { command, .. } = &agent.mcp {
            steps.push(format!(
                "{name}: the Mash MCP server was not registered, so this agent cannot search history yet; fix the error above, then rerun `mash init` or run `{command}`."
            ));
        } else if let Step::Stale { stale, command, .. } = &agent.mcp {
            steps.push(format!(
                "{name}: the Mash MCP registration runs {stale}, which is missing or is not this Mash binary, and could not be replaced (see the error above). Run `{command}`, then restart the agent."
            ));
        } else if let Step::Failed { command, .. } = &agent.plugin {
            steps.push(format!(
                "{name}: the Mash MCP server is registered, but the mash@mash plugin (history skill and session hooks) was not installed; when online, rerun `mash init` or run `{command}`."
            ));
        }
    }
    if agents
        .iter()
        .any(|agent| !matches!(agent.mcp, Step::Failed { .. } | Step::Stale { .. }))
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
    selected: Option<&[Agent]>,
) -> Vec<AgentOutcome> {
    let mut outcomes = Vec::new();
    for (kind, install) in [(Agent::ClaudeCode, "install"), (Agent::Codex, "add")] {
        if selected.is_some_and(|agents| !agents.contains(&kind)) {
            continue;
        }
        let Some(program) = which(kind.program()) else {
            continue;
        };
        let program = program.as_path();
        let mut add: Vec<OsString> = ["mcp", "add"].map(OsString::from).to_vec();
        if kind == Agent::ClaudeCode {
            add.extend(["--scope", "user"].map(OsString::from));
        }
        add.extend(["mash", "--"].map(OsString::from));
        add.extend(command.iter().cloned());
        let mcp = if run(program, &["mcp", "get", "mash"]).await.is_ok() {
            let registration = crate::blocking(move || Ok(super::installation::registration(kind)))
                .await
                .unwrap_or(Registration::Unknown);
            match registration {
                Registration::Stale { command: stale } => replace(program, kind, stale, &add).await,
                Registration::Current | Registration::Unknown => Step::AlreadyPresent,
            }
        } else {
            step(program, &add).await
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
        let files = crate::blocking(move || Ok(super::installation::edited_files(kind)))
            .await
            .unwrap_or_default();
        outcomes.push(AgentOutcome {
            agent: kind,
            mcp,
            plugin,
            files,
            undo: [kind.remove_mcp(), kind.remove_plugin()],
        });
    }
    outcomes
}

/// Remove a registration whose executable is missing or another binary,
/// then add this one through the agent's own CLI.
async fn replace(program: &Path, agent: Agent, stale: String, add: &[OsString]) -> Step {
    let mut remove: Vec<OsString> = ["mcp", "remove"].map(OsString::from).to_vec();
    if agent == Agent::ClaudeCode {
        remove.extend(["--scope", "user"].map(OsString::from));
    }
    remove.push("mash".into());
    let both = format!("{} && {}", display(program, &remove), display(program, add));
    if let Err(message) = run(program, &remove).await {
        return Step::Stale {
            stale,
            command: both,
            message,
        };
    }
    match step(program, add).await {
        Step::Done { command } => Step::Replaced { stale, command },
        failed => failed,
    }
}

fn display(program: &Path, arguments: &[OsString]) -> String {
    std::iter::once(program.display().to_string())
        .chain(arguments.iter().map(|a| a.to_string_lossy().into_owned()))
        .collect::<Vec<_>>()
        .join(" ")
}

async fn step(program: &Path, arguments: &[OsString]) -> Step {
    let command = display(program, arguments);
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
