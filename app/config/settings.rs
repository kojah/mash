//! The settings a person changes from the dashboard: which agents are
//! indexed, how far back, where history goes, memory capture and credential
//! masking. Every other setting in the file is kept as written, and history
//! locations come from this machine's own detection, never from the request.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::IndexDestination;
use super::document::{Edit, edit, parse_document};
use crate::AppError;
use crate::provider::{Provider, ProviderRoots};

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub(crate) struct View {
    path: PathBuf,
    /// Names the file's contents. A change states the contents it was made
    /// against, so an edit made elsewhere in between is not overwritten.
    revision: String,
    agents: BTreeMap<Provider, Agent>,
    since_days: Option<u32>,
    destination: IndexDestination,
    /// Whether a shared server is configured, so history can go to it.
    shared: bool,
    memory_capture: bool,
    hide_credentials: crate::redaction::Settings,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[schemars(rename = "AgentSettings")]
struct Agent {
    indexed: bool,
    /// The configured locations, or the ones found on this machine.
    roots: Vec<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Change {
    revision: String,
    agents: BTreeMap<Provider, bool>,
    since_days: Option<u32>,
    destination: IndexDestination,
    memory_capture: bool,
    hide_credentials: crate::redaction::Settings,
}

fn read(path: &Path) -> Result<String, AppError> {
    std::fs::read_to_string(path).map_err(|_| {
        AppError::configuration(format!(
            "no readable configuration at {}; run `mash init` first",
            path.display()
        ))
    })
}

fn revision(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

pub(crate) fn view(path: &Path) -> Result<View, AppError> {
    describe(path, &read(path)?, &super::init::detect()?)
}

fn describe(path: &Path, text: &str, detected: &ProviderRoots) -> Result<View, AppError> {
    let document = parse_document(text.as_bytes())?;
    let agents = Provider::ALL
        .iter()
        .filter_map(|provider| {
            let configured = document.providers.get(provider);
            let roots = configured.or_else(|| detected.get(provider))?.clone();
            Some((
                *provider,
                Agent {
                    indexed: configured.is_some(),
                    roots,
                },
            ))
        })
        .collect();
    Ok(View {
        path: path.to_owned(),
        revision: revision(text),
        agents,
        since_days: document.since_days,
        destination: document.destination,
        shared: document.shared.is_some(),
        memory_capture: document.records.enabled,
        hide_credentials: document.redaction,
    })
}

/// Rewrite the file with `change`, keeping the previous file beside it, and
/// return the settings as they now stand.
pub(crate) fn apply(path: &Path, change: &Change) -> Result<View, AppError> {
    let detected = super::init::detect()?;
    let text = read(path)?;
    let updated = revise(&text, change, &detected)?;
    super::init::write(path, &updated)?;
    describe(path, &updated, &detected)
}

fn revise(text: &str, change: &Change, detected: &ProviderRoots) -> Result<String, AppError> {
    if revision(text) != change.revision {
        return Err(AppError::configuration(
            "the configuration file changed since these settings were loaded; reload them and try again",
        ));
    }
    let mut providers = parse_document(text.as_bytes())?.providers;
    for (provider, indexed) in &change.agents {
        if !indexed {
            providers.remove(provider);
        } else if !providers.contains_key(provider) {
            let roots = detected.get(provider).ok_or_else(|| {
                AppError::configuration(format!("no {provider} history was found on this machine"))
            })?;
            providers.insert(*provider, roots.clone());
        }
    }
    if providers.is_empty() {
        return Err(AppError::configuration(
            "keep at least one agent's history indexed",
        ));
    }
    edit(
        text,
        &Edit {
            providers: &providers,
            since_days: change.since_days,
            destination: change.destination,
            memory_capture: change.memory_capture,
            redaction: change.hide_credentials,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A history location that is absolute on the host running the test, as
    /// configured roots must be.
    fn home(name: &str) -> PathBuf {
        PathBuf::from(if cfg!(windows) { r"C:\h" } else { "/h" }).join(name)
    }

    fn file() -> String {
        format!(
            "version = 1\n\n[providers.codex]\nroots = ['{}']\n\n[index]\nall_history = true\n\n[views.query]\nlimit = 7\n",
            home("codex").display()
        )
    }

    fn detected() -> ProviderRoots {
        ProviderRoots::from([(Provider::ClaudeCode, vec![home("claude")])])
    }

    fn change(text: &str) -> Change {
        Change {
            revision: revision(text),
            agents: BTreeMap::new(),
            since_days: None,
            destination: IndexDestination::Local,
            memory_capture: true,
            hide_credentials: crate::redaction::Settings::default(),
        }
    }

    #[test]
    fn a_change_edits_only_what_it_names_and_keeps_the_rest_of_the_file() {
        let file = &file();
        let mut request = change(file);
        request.agents = BTreeMap::from([(Provider::ClaudeCode, true)]);
        request.since_days = Some(30);
        request.memory_capture = false;
        request.hide_credentials.publication = false;
        let updated = revise(file, &request, &detected()).unwrap();
        let view = describe(Path::new("/c.toml"), &updated, &detected()).unwrap();
        assert!(view.agents[&Provider::Codex].indexed);
        assert!(view.agents[&Provider::ClaudeCode].indexed);
        assert_eq!(view.agents[&Provider::ClaudeCode].roots, [home("claude")]);
        assert_eq!(view.since_days, Some(30));
        assert!(!view.memory_capture);
        assert!(view.hide_credentials.output && !view.hide_credentials.publication);
        // A setting the page does not offer survives the rewrite.
        assert!(updated.contains("limit = 7"));
    }

    #[test]
    fn an_agent_with_no_history_here_is_offered_nothing_and_cannot_be_turned_on() {
        let file = &file();
        let view = describe(Path::new("/c.toml"), file, &detected()).unwrap();
        assert!(!view.agents.contains_key(&Provider::Cursor));
        assert!(!view.agents[&Provider::ClaudeCode].indexed);
        let mut request = change(file);
        request.agents = BTreeMap::from([(Provider::Cursor, true)]);
        let error = revise(file, &request, &detected()).unwrap_err();
        assert!(error.error.message.contains("no cursor history"));
    }

    #[test]
    fn stale_or_unusable_changes_leave_the_file_alone() {
        let file = &file();
        let mut stale = change(file);
        stale.revision = revision("something else");
        assert!(
            revise(file, &stale, &detected())
                .unwrap_err()
                .error
                .message
                .contains("changed since")
        );
        let mut none = change(file);
        none.agents = BTreeMap::from([(Provider::Codex, false)]);
        assert!(
            revise(file, &none, &detected())
                .unwrap_err()
                .error
                .message
                .contains("at least one")
        );
        // History cannot be sent to a shared server that is not configured.
        let mut both = change(file);
        both.destination = IndexDestination::Both;
        assert!(revise(file, &both, &detected()).is_err());
        let mut days = change(file);
        days.since_days = Some(0);
        assert!(revise(file, &days, &detected()).is_err());
    }

    #[test]
    fn applying_keeps_the_previous_file_beside_the_new_one() {
        let file = &file();
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config.toml");
        std::fs::write(&path, file).unwrap();
        let mut request = change(file);
        request.since_days = Some(7);
        // Detection reads this machine; the change names no agent, so it is unused.
        let view = apply(&path, &request).unwrap();
        assert_eq!(view.since_days, Some(7));
        assert_ne!(view.revision, revision(file));
        assert_eq!(
            std::fs::read_to_string(root.path().join("config.toml.previous")).unwrap(),
            *file
        );
    }
}
