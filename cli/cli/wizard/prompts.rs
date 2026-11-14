use super::{AppError, Draft, InitRequest, config};
use crate::core::{Paths, installation::Agent, setup::InitPlan};
use crate::provider::{Provider, ProviderRoots};
use inquire::{Confirm, CustomType, InquireError, MultiSelect, Select, Text};
use std::fmt::Write as _;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Choice {
    Local,
    Shared,
    Later,
    Keep,
    Review,
    Cancel,
    Recent,
    All,
    Custom,
    Index,
    Configure,
    Save,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Local => "Just this computer (recommended)",
            Self::Shared => "Connect to an existing shared server",
            Self::Later => "Set up sharing later",
            Self::Keep => "Use existing settings",
            Self::Review => "Review and edit settings",
            Self::Cancel => "Cancel",
            Self::Recent => "Last 90 days",
            Self::All => "All history",
            Self::Custom => "Custom number of days",
            Self::Index => "Set up and index",
            Self::Configure => "Set up without indexing",
            Self::Save => "Save configuration only",
        })
    }
}

enum Failure {
    Cancelled,
    Error(AppError),
}

impl From<AppError> for Failure {
    fn from(error: AppError) -> Self {
        Self::Error(error)
    }
}

impl From<InquireError> for Failure {
    fn from(error: InquireError) -> Self {
        match error {
            InquireError::OperationCanceled | InquireError::OperationInterrupted => Self::Cancelled,
            other => Self::Error(AppError::usage(format!("setup prompt failed: {other}"))),
        }
    }
}

fn cancelled<T>(result: Result<T, Failure>) -> Result<Option<T>, AppError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(Failure::Cancelled) => {
            eprintln!("Setup cancelled. No configuration or agent settings were changed.");
            Ok(None)
        }
        Err(Failure::Error(error)) => Err(error),
    }
}

pub(super) fn collect(paths: &Paths, init: InitRequest) -> Result<Option<Draft>, AppError> {
    cancelled(collect_inner(paths, init))
}

fn collect_inner(paths: &Paths, mut init: InitRequest) -> Result<Draft, Failure> {
    eprintln!(
        "Mash setup — search your coding-agent histories. Esc or Ctrl-C cancels before changes."
    );
    let current = config::load(paths.config.as_deref())?;
    let previous = if current.loaded {
        Some(std::fs::read_to_string(&current.path).map_err(AppError::io)?)
    } else {
        None
    };
    let review = if current.loaded {
        eprintln!("Existing configuration: {}", current.path.display());
        if init.force {
            eprintln!("Reviewing existing settings; a backup will be kept when saved.");
            true
        } else {
            match Select::new(
                "Use your existing configuration?",
                vec![Choice::Keep, Choice::Review, Choice::Cancel],
            )
            .prompt()?
            {
                Choice::Keep => false,
                Choice::Review => true,
                _ => return Err(Failure::Cancelled),
            }
        }
    } else {
        true
    };
    let mut choices = config::init::Choices {
        records: current.records.clone(),
        providers: if current.loaded {
            current.providers.clone()
        } else {
            config::init::detect()?
        },
        since_days: current.since_days,
        shared: if current.destination == config::IndexDestination::Both {
            current.shared.clone()
        } else {
            None
        },
    };
    if review {
        choices.shared = sharing(choices.shared.as_ref())?;
        choices.providers = histories(choices.providers)?;
        choices.since_days = coverage(choices.since_days)?;
    }
    select_agents(&mut init)?;
    select_backfill(&mut init, &mut choices)?;
    if review || init.backfill_memory.is_some() {
        // Validate before readiness probes and final confirmation, without writing.
        let base = match &previous {
            Some(text) => text.clone(),
            None => config::init::render(&choices.providers, false)?,
        };
        config::init::configure(&base, &choices)?;
        init.plan = Some(InitPlan {
            choices: choices.clone(),
            previous,
        });
    }
    let summary = setup_summary(&current.path, &choices, &init);
    eprintln!("{summary}");
    Ok(Draft {
        init,
        qdrant: current.qdrant,
        shared: choices.shared,
        summary,
    })
}

fn setup_summary(
    path: &std::path::Path,
    choices: &config::init::Choices,
    init: &InitRequest,
) -> String {
    let mut summary = format!("Configuration: {}\n", path.display());
    if let Some(agent) = init.backfill_memory {
        let _ = writeln!(
            summary,
            "Memory: review indexed chats now with {agent:?}, after indexing. Uses agent tokens; older chats are never assigned to later conversations."
        );
    }
    summary.push_str(if choices.shared.is_some() {
        "Search: local and shared; indexed history will be uploaded to your server.\n"
    } else {
        "Search: this computer only.\n"
    });
    if let Some(shared) = &choices.shared {
        let _ = writeln!(
            summary,
            "Server: {}\nGroup: {}\nThis computer: {}\nToken variable: {}",
            shared.api_url, shared.group_id, shared.node_id, shared.token_env
        );
    }
    for (provider, roots) in &choices.providers {
        for root in roots {
            let _ = writeln!(summary, "  {provider}: {}", root.display());
        }
    }
    let _ = writeln!(
        summary,
        "History: {}",
        choices.since_days.map_or_else(
            || "all history".into(),
            |days| format!("sources modified in the last {days} days")
        )
    );
    let agents = init
        .selected_agents
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|agent| match agent {
            Agent::ClaudeCode => "Claude Code",
            Agent::Codex => "Codex",
        })
        .collect::<Vec<_>>()
        .join(", ");
    let _ = writeln!(
        summary,
        "Agent integrations: {}",
        if agents.is_empty() { "none" } else { &agents }
    );
    summary
}

fn select_backfill(
    init: &mut InitRequest,
    choices: &mut config::init::Choices,
) -> Result<(), Failure> {
    if init.backfill_memory.is_none() {
        let available: Vec<_> = [
            ("Codex", "codex", Agent::Codex),
            ("Claude Code", "claude", Agent::ClaudeCode),
        ]
        .into_iter()
        .filter(|(_, command, _)| crate::core::setup::which(command).is_some())
        .collect();
        if available.is_empty() {
            return Ok(());
        }
        let question = if choices.records.backfill_since.is_some() {
            "Resume the previous memory backfill now?"
        } else {
            "Backfill memory from chats active in the last 3 days now?"
        };
        if !Confirm::new(question).with_default(false)
            .with_help_message("Runs during this setup using a signed-in agent and its tokens. Sends selected chat text to that agent's service. You can skip or stop; completed reviews are retained.")
            .prompt()? {
            return Ok(());
        }
        let labels = available.iter().map(|(label, _, _)| *label).collect();
        let selected =
            Select::new("Which signed-in agent should review memory?", labels).prompt()?;
        init.backfill_memory = available
            .iter()
            .find(|(label, _, _)| *label == selected)
            .map(|(_, _, agent)| *agent);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| AppError::internal("system clock precedes Unix epoch"))?;
    let cutoff = now.as_millis().saturating_sub(3 * 86_400_000);
    choices.records.backfill_since.get_or_insert(
        i64::try_from(cutoff).map_err(|_| AppError::internal("clock out of range"))?,
    );
    Ok(())
}

fn select_agents(init: &mut InitRequest) -> Result<(), Failure> {
    if init.agents {
        let available: Vec<_> = [
            ("Claude Code", "claude", Agent::ClaudeCode),
            ("Codex", "codex", Agent::Codex),
        ]
        .into_iter()
        .filter(|(_, command, _)| crate::core::setup::which(command).is_some())
        .collect();
        let labels = available
            .iter()
            .map(|(label, _, _)| *label)
            .collect::<Vec<_>>();
        let defaults: Vec<_> = (0..labels.len()).collect();
        let selected = if labels.is_empty() {
            eprintln!("No Claude Code or Codex CLI found. You can connect an agent later.");
            Vec::new()
        } else {
            MultiSelect::new("Which agents should get access to Mash search?", labels)
                .with_default(&defaults)
                .with_help_message("These agents can search all histories selected above. Installs the Mash MCP connection and history plugin; select none for CLI-only use.")
                .prompt()?.into_iter().filter_map(|label| available.iter().find(|(name, _, _)| *name == label).map(|(_, _, kind)| *kind)).collect()
        };
        init.agents = !selected.is_empty();
        init.selected_agents = Some(selected);
    }
    Ok(())
}

fn sharing(
    existing: Option<&config::SharedConfig>,
) -> Result<Option<config::SharedConfig>, Failure> {
    let options = vec![Choice::Local, Choice::Shared, Choice::Later];
    let selection = Select::new("Where do you want to search your history?", options)
        .with_starting_cursor(usize::from(existing.is_some()))
        .prompt()?;
    if selection != Choice::Shared {
        if selection == Choice::Later {
            eprintln!(
                "Finish local setup now. Hosting guide: https://getmash.io/shared-index/\nLater, rerun `mash init` and choose Review and edit settings."
            );
        }
        return Ok(None);
    }
    eprintln!(
        "Each computer keeps a local index and publishes history to the shared server.\nAsk its administrator for these settings and a token for this computer. Hosting guide: https://getmash.io/shared-index/"
    );
    let api_url = text(
        "Shared server URL",
        existing.as_ref().map_or("https://", |s| s.api_url.as_str()),
    )?;
    let group_id = text(
        "Group ID",
        existing
            .as_ref()
            .map_or("personal", |s| s.group_id.as_str()),
    )?;
    let node_id = text(
        "Unique ID for this computer",
        existing.as_ref().map_or("", |s| s.node_id.as_str()),
    )?;
    eprintln!(
        "Keep the token in an environment variable available to Mash and your agent clients. Enter its NAME below, not the token itself; setup never stores tokens in TOML."
    );
    let token_env = text(
        "Token environment variable name",
        existing
            .as_ref()
            .map_or("MASH_SHARED_TOKEN", |s| s.token_env.as_str()),
    )?;
    Ok(Some(config::SharedConfig {
        api_url,
        group_id,
        node_id,
        token_env,
        publication_redaction: crate::redaction::Policy::default(),
    }))
}

fn text(message: &str, default: &str) -> Result<String, Failure> {
    Ok(Text::new(message).with_initial_value(default).prompt()?)
}

fn histories(mut roots: ProviderRoots) -> Result<ProviderRoots, Failure> {
    loop {
        let providers: Vec<_> = roots.keys().copied().collect();
        if !providers.is_empty() {
            for (provider, paths) in &roots {
                for path in paths {
                    eprintln!("{provider}: {}", path.display());
                }
            }
            let defaults: Vec<_> = (0..providers.len()).collect();
            let selected =
                MultiSelect::new("Which conversation histories should Mash index?", providers)
                    .with_default(&defaults)
                    .prompt()?;
            roots.retain(|provider, _| selected.contains(provider));
        }
        if roots.is_empty()
            || Confirm::new("Add or replace a history location?")
                .with_default(false)
                .prompt()?
        {
            let provider = Select::new("Provider", Provider::ALL.to_vec()).prompt()?;
            let path = Text::new("Absolute path to the history folder or database")
                .with_validator(|input: &str| {
                    let path = std::path::Path::new(input);
                    Ok(if path.is_absolute() && path.exists() {
                        inquire::validator::Validation::Valid
                    } else {
                        inquire::validator::Validation::Invalid(
                            "Enter an existing absolute path.".into(),
                        )
                    })
                })
                .prompt()?;
            roots.insert(provider, vec![PathBuf::from(path)]);
        } else {
            return Ok(roots);
        }
    }
}

fn coverage(current: Option<u32>) -> Result<Option<u32>, Failure> {
    let cursor = match current {
        Some(90) => 0,
        None => 1,
        Some(_) => 2,
    };
    match Select::new(
        "How much history should Mash index?",
        vec![Choice::Recent, Choice::All, Choice::Custom],
    )
    .with_starting_cursor(cursor)
    .prompt()?
    {
        Choice::Recent => Ok(Some(90)),
        Choice::All => Ok(None),
        _ => Ok(Some(
            CustomType::<u32>::new("Number of days")
                .with_default(current.unwrap_or(90))
                .with_validator(|days: &u32| {
                    Ok(if (1..=36_500).contains(days) {
                        inquire::validator::Validation::Valid
                    } else {
                        inquire::validator::Validation::Invalid("Choose 1 to 36500 days.".into())
                    })
                })
                .prompt()?,
        )),
    }
}

pub(super) fn confirm(draft: &mut Draft, ready: bool) -> Result<bool, AppError> {
    let result = (|| -> Result<bool, Failure> {
        eprintln!("Ready to apply:\n{}", draft.summary);
        let options = if !draft.init.index {
            vec![Choice::Configure, Choice::Save, Choice::Cancel]
        } else if ready {
            vec![Choice::Index, Choice::Save, Choice::Cancel]
        } else {
            vec![Choice::Save, Choice::Cancel]
        };
        match Select::new("Apply these choices?", options).prompt()? {
            Choice::Index | Choice::Configure => Ok(true),
            Choice::Save => {
                draft.init.index = false;
                draft.init.agents = false;
                draft.init.backfill_memory = None;
                eprintln!(
                    "Saving configuration only. Run `mash init` again to connect agents and index."
                );
                Ok(true)
            }
            _ => Err(Failure::Cancelled),
        }
    })();
    Ok(cancelled(result)?.unwrap_or(false))
}
