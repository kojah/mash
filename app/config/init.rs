//! First-run configuration: find provider histories in their default locations
//! and write a strict node configuration naming them.
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use directories::BaseDirs;

use super::document::parse_document;
use crate::AppError;
use crate::provider::{Provider, ProviderRoots};

#[derive(Debug, Clone)]
pub(crate) struct Choices {
    pub(crate) providers: ProviderRoots,
    pub(crate) since_days: Option<u32>,
    pub(crate) shared: Option<super::SharedConfig>,
    pub(crate) records: crate::records::capture::Policy,
}

pub(crate) fn configure(text: &str, choices: &Choices) -> Result<String, AppError> {
    if choices.providers.is_empty() || choices.providers.values().any(Vec::is_empty) {
        return Err(AppError::configuration(
            "select at least one history location",
        ));
    }
    super::document::configure(text, choices)
}

/// Default history locations that exist on this machine, per provider.
pub(crate) fn detect() -> Result<ProviderRoots, AppError> {
    let directories = BaseDirs::new()
        .ok_or_else(|| AppError::configuration("platform home directory is unavailable"))?;
    let home = directories.home_dir();
    if !home.is_absolute() {
        return Err(AppError::configuration(
            "platform home directory is not absolute",
        ));
    }
    let variable = |name: &str| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    let config_home = variable("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
    let data_home = variable("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share"));
    // VS Code-family editors keep per-user state here.
    let roaming = variable("APPDATA").unwrap_or_else(|| home.join("AppData/Roaming"));
    let editor = |application: &str| {
        if cfg!(target_os = "macos") {
            home.join("Library/Application Support")
                .join(application)
                .join("User")
        } else if cfg!(windows) {
            roaming.join(application).join("User")
        } else {
            config_home.join(application).join("User")
        }
    };
    let candidates = [
        (
            Provider::ClaudeCode,
            vec![
                variable("CLAUDE_CONFIG_DIR").map(|dir| dir.join("projects")),
                Some(home.join(".claude/projects")),
                Some(config_home.join("claude/projects")),
            ],
        ),
        (
            Provider::Codex,
            vec![
                variable("CODEX_HOME").map(|dir| dir.join("sessions")),
                variable("CODEX_HOME").map(|dir| dir.join("thread_history_1.sqlite")),
                Some(home.join(".codex/sessions")),
                Some(home.join(".codex/thread_history_1.sqlite")),
                Some(data_home.join("codex/sessions")),
                Some(data_home.join("codex/thread_history_1.sqlite")),
            ],
        ),
        (
            Provider::Opencode,
            vec![Some(data_home.join("opencode/opencode.db"))],
        ),
        (
            Provider::Copilot,
            vec![
                Some(home.join(".copilot/session-state")),
                Some(editor("Code").join("workspaceStorage")),
                Some(editor("Code").join("globalStorage")),
            ],
        ),
        (
            Provider::Cursor,
            vec![
                variable("CURSOR_CONFIG_DIR").map(|dir| dir.join("chats")),
                Some(config_home.join("cursor/chats")),
                Some(home.join(".cursor/chats")),
                Some(editor("Cursor").join("workspaceStorage")),
                Some(editor("Cursor").join("globalStorage")),
            ],
        ),
        (
            Provider::Antigravity,
            vec![Some(home.join(".gemini/antigravity-ide/conversations"))],
        ),
    ];
    let mut found = ProviderRoots::new();
    for (provider, paths) in candidates {
        let mut roots: Vec<PathBuf> = Vec::new();
        for path in paths.into_iter().flatten() {
            // Rebuilt from its parts so every separator is the platform's own:
            // the names above are written with "/" and joined to a Windows home
            // would otherwise read `C:\Users\me\.claude/projects`.
            let path: PathBuf = path.components().collect();
            if path.exists() && !roots.contains(&path) {
                roots.push(path);
            }
        }
        if !roots.is_empty() {
            found.insert(provider, roots);
        }
    }
    Ok(found)
}

/// A configuration for `providers`; without installed models, lexical-only
/// views make exact-mention search work immediately.
pub(crate) fn render(providers: &ProviderRoots, semantic: bool) -> Result<String, AppError> {
    let quote = |path: &Path| {
        path.to_str()
            .map(|text| toml::Value::String(text.to_owned()).to_string())
            .ok_or_else(|| {
                AppError::configuration(format!("history path is not UTF-8: {}", path.display()))
            })
    };
    let mut text = String::from(
        "# Written by `mash init` from the agent histories found on this machine.\n\
         # Add or remove providers and roots freely; see the Mash README.\n\
         version = 1\n",
    );
    for (provider, roots) in providers {
        let roots = roots
            .iter()
            .map(|root| quote(root))
            .collect::<Result<Vec<_>, _>>()?;
        let _ = write!(
            text,
            "\n[providers.{}]\nroots = [{}]\n",
            provider.as_str(),
            roots.join(", ")
        );
    }
    if !semantic {
        text.push_str(
            "\n# Models are not installed, so search matches words exactly. After\n\
             # `mash models install`, follow https://getmash.io/indexing/#enable-semantic-search.\n\
             [views.build]\nrepresentations = \"lexical-only\"\n\n\
             [views.query]\nintent = \"exact-mention\"\n",
        );
    }
    parse_document(text.as_bytes())?;
    Ok(text)
}

/// Write `text` to `path`, keeping an existing file as `<name>.previous` and
/// returning where it went.
pub(crate) fn write(path: &Path, text: &str) -> Result<Option<PathBuf>, AppError> {
    let failed = |operation: &str, error: &std::io::Error| {
        AppError::configuration(format!(
            "failed to {operation} configuration {}: {error}",
            path.display()
        ))
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| failed("create the directory of", &error))?;
    }
    let previous = if path.exists() {
        let mut previous = path.as_os_str().to_owned();
        previous.push(".previous");
        let previous = PathBuf::from(previous);
        fs::rename(path, &previous).map_err(|error| failed("keep the previous", &error))?;
        Some(previous)
    } else {
        None
    };
    let mut staged = path.as_os_str().to_owned();
    staged.push(".partial");
    fs::write(&staged, text).map_err(|error| failed("write", &error))?;
    fs::rename(&staged, path).map_err(|error| failed("install", &error))?;
    Ok(previous)
}

/// Record in the configuration at `path` that `mash init` connected
/// `agents`, keeping agents recorded earlier. Returns whether it changed.
pub(crate) fn record_connected_agents(
    path: &Path,
    agents: &[crate::core::installation::Agent],
) -> Result<bool, AppError> {
    let text = fs::read_to_string(path).map_err(|error| {
        AppError::configuration(format!(
            "failed to read configuration {}: {error}",
            path.display()
        ))
    })?;
    let Some(updated) = super::document::connect_agents(&text, agents)? else {
        return Ok(false);
    };
    let mut staged = path.as_os_str().to_owned();
    staged.push(".partial");
    let failed = |operation: &str, error: &std::io::Error| {
        AppError::configuration(format!(
            "failed to {operation} configuration {}: {error}",
            path.display()
        ))
    };
    fs::write(&staged, updated).map_err(|error| failed("write", &error))?;
    fs::rename(&staged, path).map_err(|error| failed("install", &error))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::installation::Agent;

    #[test]
    fn init_records_connected_agents_and_keeps_comments() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config.toml");
        let providers = ProviderRoots::from([(Provider::Codex, vec![root.path().join("history")])]);
        std::fs::write(&path, render(&providers, false).unwrap()).unwrap();
        assert_eq!(
            super::super::load(Some(&path)).unwrap().connected_agents,
            []
        );
        assert!(record_connected_agents(&path, &[Agent::Codex]).unwrap());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# Written by `mash init`"), "{text}");
        assert!(!record_connected_agents(&path, &[Agent::Codex]).unwrap());
        assert!(
            record_connected_agents(&path, &[Agent::ClaudeCode, Agent::Codex, Agent::ClaudeCode])
                .unwrap()
        );
        assert_eq!(
            super::super::load(Some(&path)).unwrap().connected_agents,
            [Agent::Codex, Agent::ClaudeCode]
        );
        assert!(!root.path().join("config.toml.previous").exists());
        std::fs::write(
            &path,
            "version = 1\nproviders = {}\n[agents]\nconnected = ['codex', 'codex']\n",
        )
        .unwrap();
        assert!(super::super::load(Some(&path)).is_err());
        std::fs::write(
            &path,
            "version = 1\nproviders = {}\n[agents]\nconnected = ['cursor']\n",
        )
        .unwrap();
        assert!(super::super::load(Some(&path)).is_err());
    }

    #[test]
    fn setup_wizard_preserves_custom_settings_and_applies_choices() {
        let root = tempfile::tempdir().unwrap();
        let providers = ProviderRoots::from([(Provider::Codex, vec![root.path().join("history")])]);
        let original = format!(
            "{}\n[qdrant]\nurl='http://localhost:7777'\ngrpc_url='http://localhost:7778'\n[redaction]\noutput=false\npublication=false\n",
            render(&providers, false).unwrap()
        );
        let mut choices = Choices {
            records: crate::records::capture::Policy::default(),
            providers: ProviderRoots::from([(
                Provider::ClaudeCode,
                vec![root.path().join("new-history")],
            )]),
            since_days: None,
            shared: Some(super::super::SharedConfig {
                group_id: "g".into(),
                node_id: "n".into(),
                api_url: "https://example.com".into(),
                token_env: "TOKEN".into(),
                publication_redaction: crate::redaction::Policy::default(),
            }),
        };
        let updated = configure(&original, &choices).unwrap();
        let parsed = parse_document(updated.as_bytes()).unwrap();
        assert_eq!(parsed.providers, choices.providers);
        choices.records.backfill_since = Some(1234);
        let with_memory = configure(&updated, &choices).unwrap();
        assert_eq!(
            parse_document(with_memory.as_bytes())
                .unwrap()
                .records
                .backfill_since,
            Some(1234)
        );
        assert_eq!(parsed.since_days, None);
        assert_eq!(parsed.destination, super::super::IndexDestination::Both);
        assert_eq!(parsed.qdrant.url, "http://localhost:7777");
        assert!(!parsed.redaction.output);
        assert!(!parsed.redaction.publication);
        let path = root.path().join("config.toml");
        std::fs::write(&path, &updated).unwrap();
        assert!(
            !super::super::load(Some(&path))
                .unwrap()
                .shared
                .unwrap()
                .publication_redaction
                .enabled()
        );
        choices.shared = None;
        choices.since_days = Some(12);
        let local = configure(&updated, &choices).unwrap();
        let local = parse_document(local.as_bytes()).unwrap();
        assert!(local.shared.is_none());
        assert_eq!(local.destination, super::super::IndexDestination::Local);
        assert_eq!(local.since_days, Some(12));
    }

    #[test]
    fn setup_wizard_rejects_invalid_choices_before_writing() {
        let root = tempfile::tempdir().unwrap();
        let providers = ProviderRoots::from([(Provider::Codex, vec![root.path().join("history")])]);
        let original = render(&providers, false).unwrap();
        let mut choices = Choices {
            records: crate::records::capture::Policy::default(),
            providers,
            since_days: Some(0),
            shared: None,
        };
        assert!(configure(&original, &choices).is_err());
        choices.since_days = Some(90);
        choices.providers.clear();
        assert!(configure(&original, &choices).is_err());
    }
}
