//! First-run configuration: find provider histories in their default locations
//! and write a strict node configuration naming them.
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use directories::BaseDirs;

use super::document::parse_document;
use crate::AppError;
use crate::provider::{Provider, ProviderRoots};

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
    let editor = |application: &str| {
        if cfg!(target_os = "macos") {
            home.join("Library/Application Support")
                .join(application)
                .join("User")
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
             # `mash models install`, run `mash init --force` for semantic search.\n\
             [views.build]\nrepresentations = \"lexical-only\"\n\n\
             [views.query]\nintent = \"exact-mention\"\n",
        );
    }
    parse_document(text.as_bytes())?;
    Ok(text)
}

/// Write `text` to `path`, keeping an existing file as `<name>.previous`.
pub(crate) fn write(path: &Path, text: &str) -> Result<(), AppError> {
    let failed = |operation: &str, error: &std::io::Error| {
        AppError::configuration(format!(
            "failed to {operation} configuration {}: {error}",
            path.display()
        ))
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| failed("create the directory of", &error))?;
    }
    if path.exists() {
        let mut previous = path.as_os_str().to_owned();
        previous.push(".previous");
        fs::rename(path, previous).map_err(|error| failed("keep the previous", &error))?;
    }
    let mut staged = path.as_os_str().to_owned();
    staged.push(".partial");
    fs::write(&staged, text).map_err(|error| failed("write", &error))?;
    fs::rename(&staged, path).map_err(|error| failed("install", &error))
}
