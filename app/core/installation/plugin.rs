use super::{
    Agent, Client, Component, Environment, Stamp, State, clients::Hooks, executable,
    expected_stamp, read,
};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(super) fn missing(client: &mut Client, path: &Path) {
    client.add(Component::Plugin, State::Missing, Some(path), "Mash history plugin registration or installed files are missing.", Some("Run `mash init` (or install and enable mash@mash from the kojah/mash marketplace), then restart the agent."));
}

pub(super) fn cached_roots(client: &mut Client, path: &Path) -> Vec<PathBuf> {
    match std::fs::read_dir(path) {
        Ok(entries) => {
            let mut roots = vec![];
            for entry in entries {
                match entry {
                    Ok(entry) if entry.path().is_dir() => roots.push(entry.path()),
                    Ok(_) => {}
                    Err(_) => client.add(
                        Component::Plugin,
                        State::Unverified,
                        Some(path),
                        "Cannot enumerate every cached plugin version.",
                        Some("Check plugin cache permissions."),
                    ),
                }
            }
            roots
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => vec![],
        Err(_) => {
            client.add(
                Component::Plugin,
                State::Unverified,
                Some(path),
                "Cannot read the plugin cache.",
                Some("Check plugin cache permissions."),
            );
            vec![]
        }
    }
}

#[derive(Deserialize)]
struct Installed {
    plugins: BTreeMap<String, Vec<Install>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Install {
    scope: String,
    install_path: PathBuf,
}

pub(super) fn claude_install(client: &mut Client, id: &str, environment: &Environment) {
    let path = environment.claude.join("plugins/installed_plugins.json");
    match read::<Installed>(&path) {
        Ok(Some(installed)) => {
            let installs: Vec<_> = installed
                .plugins
                .get(id)
                .into_iter()
                .flatten()
                .filter(|install| install.scope == "user")
                .collect();
            match installs.as_slice() {
                [install] => inspect(client, &install.install_path, environment, None),
                [] => missing(client, &path),
                _ => client.add(
                    Component::Plugin,
                    State::Unverified,
                    Some(&path),
                    "Multiple user installations are registered.",
                    Some("Inspect the active Mash copy in Claude Code's plugin settings."),
                ),
            }
        }
        Ok(None) => missing(client, &path),
        Err(message) => client.add(
            Component::Plugin,
            State::Invalid,
            Some(&path),
            message,
            Some("Repair or reinstall the Mash plugin with Claude Code."),
        ),
    }
}

#[derive(Deserialize)]
struct Manifest {
    name: String,
    version: String,
    hooks: Option<String>,
    skills: Option<String>,
}

#[derive(Deserialize)]
struct HookFile {
    hooks: BTreeMap<String, Vec<Group>>,
}

#[derive(Deserialize)]
struct Group {
    hooks: Vec<Hook>,
}

#[derive(Deserialize)]
struct Hook {
    #[serde(rename = "type")]
    kind: String,
    command: Option<String>,
}

pub(super) fn inspect(
    client: &mut Client,
    root: &Path,
    environment: &Environment,
    trust: Option<(&str, &Hooks)>,
) {
    let path = root.join(match client.agent {
        Agent::Codex => ".codex-plugin/plugin.json",
        Agent::ClaudeCode => ".claude-plugin/plugin.json",
    });
    let expected = expected_stamp();
    match read::<Manifest>(&path) {
        Ok(Some(manifest)) if manifest.name == "mash" => {
            if manifest
                .hooks
                .as_deref()
                .is_some_and(|path| path != "./hooks/hooks.json" && path != "hooks/hooks.json")
                || manifest
                    .skills
                    .as_deref()
                    .is_some_and(|path| path != "./skills/" && path != "skills/")
            {
                client.add(Component::Plugin, State::Unverified, Some(&path), "Manifest overrides the bundled hook or skill paths; effective resources are unverified.", Some("Restore the bundled Mash manifest or inspect the custom paths in the client."));
            }
            let state = if manifest.version == expected.plugin_version {
                State::Present
            } else {
                State::Outdated
            };
            client.add(Component::Plugin, state, Some(&path),
                if state == State::Present { "Plugin manifest matches the version bundled with this CLI." } else { "Plugin version differs from the version bundled with this CLI; versions may be out of sync." },
                (state != State::Present).then_some("Refresh the Mash binary and agent plugin together, then restart the client."));
        }
        Ok(None) => missing(client, &path),
        Ok(Some(_)) => client.add(
            Component::Plugin,
            State::Invalid,
            Some(&path),
            "Manifest does not identify the Mash plugin.",
            Some("Reinstall Mash from its marketplace."),
        ),
        Err(message) => client.add(
            Component::Plugin,
            State::Invalid,
            Some(&path),
            message,
            Some("Reinstall the Mash plugin."),
        ),
    }
    let skill = root.join("skills/history/SKILL.md");
    let readable = readable_file(&skill);
    client.add(
        Component::Skill,
        if readable {
            State::Present
        } else {
            State::Missing
        },
        Some(&skill),
        if readable {
            "History skill file is readable."
        } else {
            "History skill file is missing, empty or unreadable."
        },
        (!readable).then_some("Refresh or reinstall the Mash history plugin."),
    );
    let stamp = root.join("skills/history/contract.json");
    match read::<Stamp>(&stamp) {
        Ok(Some(stamp_value))
            if stamp_value.contract_fingerprint == expected.contract_fingerprint
                && stamp_value.plugin_version == expected.plugin_version => {}
        _ => client.add(
            Component::Skill,
            State::Outdated,
            Some(&stamp),
            "History skill contract stamp is missing, invalid or differs from this CLI.",
            Some("Refresh the plugin and binary together; reload the skill and reconnect MCP."),
        ),
    }
    hooks(client, root, environment, trust);
}

fn hooks(
    client: &mut Client,
    root: &Path,
    environment: &Environment,
    trust: Option<(&str, &Hooks)>,
) {
    let path = root.join("hooks/hooks.json");
    hook_dependencies(client, root, environment);
    let definition = match read::<HookFile>(&path) {
        Ok(Some(definition)) => definition,
        Ok(None) => {
            client.add(
                Component::Hooks,
                State::Missing,
                Some(&path),
                "Bundled hook definitions are missing.",
                Some("Reinstall the Mash plugin."),
            );
            return;
        }
        Err(message) => {
            client.add(
                Component::Hooks,
                State::Invalid,
                Some(&path),
                message,
                Some("Reinstall the Mash plugin."),
            );
            return;
        }
    };
    for (event, key, script) in [
        ("SessionStart", "session_start", "session_start.py"),
        ("UserPromptSubmit", "user_prompt_submit", "prompt_recall.py"),
    ] {
        let entries: Vec<_> = definition
            .hooks
            .get(event)
            .into_iter()
            .flatten()
            .enumerate()
            .flat_map(|(group, value)| {
                value
                    .hooks
                    .iter()
                    .enumerate()
                    .map(move |(index, hook)| (group, index, hook))
            })
            .collect();
        let expected_command = format!(
            "sh \"${{CLAUDE_PLUGIN_ROOT}}/hooks/python.sh\" \"${{CLAUDE_PLUGIN_ROOT}}/hooks/{script}\" 2>/dev/null || true"
        );
        let matched = entries.iter().find(|(_, _, hook)| {
            hook.kind == "command" && hook.command.as_deref() == Some(&expected_command)
        });
        let script_path = root.join("hooks").join(script);
        if matched.is_none() || !readable_file(&script_path) {
            client.add(Component::Hooks, State::Missing, Some(&path), &format!("{event} bundled command or script is missing or modified."), Some("Refresh the bundled Mash hooks; inspect intentional custom commands in the client."));
            continue;
        }
        client.add(
            Component::Hooks,
            State::Present,
            Some(&script_path),
            &format!("{event} command and script found; hook execution has not been tested."),
            None,
        );
        if let (Some((id, settings)), Some((group, index, _))) = (trust, matched) {
            let state_key = format!("{id}:hooks/hooks.json:{key}:{group}:{index}");
            let state = settings.state.get(&state_key);
            let (status, message) = if state.is_some_and(|state| state.enabled == Some(false)) {
                (
                    State::Disabled,
                    "Hook is explicitly disabled in Codex user settings.",
                )
            } else if state
                .is_none_or(|state| state.trusted_hash.as_deref().is_none_or(str::is_empty))
            {
                (
                    State::Missing,
                    "No user-level Codex trust record found for this hook.",
                )
            } else {
                (
                    State::Unverified,
                    "Codex hook trust record exists; only Codex can verify its hash against the effective hook definition.",
                )
            };
            client.add(Component::HookTrust, status, Some(&environment.codex.join("config.toml")), message, Some("Open /hooks in Codex, review and trust the current Mash hooks, then start a new session."));
        }
    }
}

fn hook_dependencies(client: &mut Client, root: &Path, environment: &Environment) {
    for helper in ["recall_common.py", "python.sh"] {
        let helper = root.join("hooks").join(helper);
        if !readable_file(&helper) {
            client.add(
                Component::Hooks,
                State::Missing,
                Some(&helper),
                "Required recall helper is missing or unreadable.",
                Some("Reinstall the Mash plugin."),
            );
        }
    }
    // hooks/python.sh runs the first of these that works.
    if ["python3", "python", "py"]
        .iter()
        .all(|name| executable(name, environment).is_none())
    {
        client.add(
            Component::Hooks,
            State::Missing,
            Some(&root.join("hooks/hooks.json")),
            "No Python 3 (python3, python or py) required by the bundled hooks is executable on this CLI's PATH.",
            Some("Install Python 3.9 or later and make it available to the agent's hook environment."),
        );
    }
}

fn readable_file(path: &Path) -> bool {
    std::fs::File::open(path).is_ok_and(|file| {
        file.metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
    })
}
