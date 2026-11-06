//! `mash init` against a redirected home. Unix only: Windows resolves Mash's
//! directories through the Known Folders API, not HOME/XDG variables, and its
//! `mash init` is covered by the Windows workflow on a real profile.
#![cfg(unix)]

use super::*;

/// A home with Claude Code history and stand-in agent CLIs that log calls.
fn fixture(registered: bool) -> (tempfile::TempDir, Vec<(&'static str, String)>) {
    fixture_with(registered, "exit 0")
}

/// `plugin` runs as the stand-in's last line for plugin commands.
fn fixture_with(
    registered: bool,
    plugin: &str,
) -> (tempfile::TempDir, Vec<(&'static str, String)>) {
    let root = tempfile::tempdir().unwrap();
    let home = root.path();
    let project = home.join(".claude/projects/p");
    std::fs::create_dir_all(&project).unwrap();
    let body = r#"{"type":"user","sessionId":"first","uuid":"m1","cwd":"/work","timestamp":"2026-10-28T09:00:00Z","message":{"role":"user","content":"initneedle schema change"}}"#;
    std::fs::write(project.join("first.jsonl"), format!("{body}\n")).unwrap();
    let bin = home.join("bin");
    std::fs::create_dir(&bin).unwrap();
    for agent in ["claude", "codex"] {
        let script = bin.join(agent);
        let get = i32::from(!registered);
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\necho \"{agent} $*\" >> '{}'\n[ \"$1 $2\" = \"mcp get\" ] && exit {get}\n[ \"$1\" = plugin ] && {{ {plugin}; }}\nexit 0\n",
                home.join("calls.log").display()
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let env = vec![
        ("HOME", home.display().to_string()),
        (
            "XDG_CONFIG_HOME",
            home.join(".config").display().to_string(),
        ),
        (
            "XDG_DATA_HOME",
            home.join(".local/share").display().to_string(),
        ),
        ("PATH", format!("{}:/usr/bin:/bin", bin.display())),
    ];
    (root, env)
}

fn init(env: &[(&str, String)], arguments: &[&str]) -> Value {
    let output = Command::cargo_bin("mash")
        .unwrap()
        .arg("init")
        .args(arguments)
        .env_clear()
        .envs(env.iter().map(|(key, value)| (*key, value.as_str())))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
#[ignore = "requires disposable Qdrant"]
fn init_configures_registers_and_indexes_detected_history() {
    let (root, env) = fixture(false);
    let home = root.path();
    let value = init(&env, &["--no-index"]);
    let config = home.join(".config/mash/config.toml");
    assert_eq!(value["config"]["written"], true, "{value}");
    assert_eq!(value["config"]["search"], "exact-mention");
    assert_eq!(
        value["config"]["providers"]["claude-code"][0],
        home.join(".claude/projects").display().to_string()
    );
    assert!(value["config"]["providers"].get("codex").is_none());
    configure_qdrant(&config);
    let indexed = init(&env, &["--no-agents"]);
    assert_eq!(indexed["index"]["indexed_messages"], 1, "{indexed}");
    for agent in &value["agents"].as_array().unwrap()[..] {
        assert_eq!(agent["mcp"]["state"], "done", "{value}");
        assert_eq!(agent["plugin"]["state"], "done", "{value}");
        // Each step names the command it ran, so users can see what changed.
        assert!(
            agent["mcp"]["command"]
                .as_str()
                .unwrap()
                .contains(" mcp add "),
            "{value}"
        );
    }
    let calls = std::fs::read_to_string(home.join("calls.log")).unwrap();
    assert!(
        calls.contains("claude mcp add --scope user mash -- "),
        "{calls}"
    );
    assert!(
        calls.contains("claude plugin marketplace add kojah/mash"),
        "{calls}"
    );
    assert!(calls.contains("claude plugin install mash@mash"), "{calls}");
    assert!(calls.contains("codex plugin add mash@mash"), "{calls}");
    assert!(
        calls
            .lines()
            .any(|line| line.starts_with("codex mcp add mash -- ") && line.ends_with(" mcp"))
    );

    let search = Command::cargo_bin("mash")
        .unwrap()
        .args(["search", "initneedle", "--local"])
        .env_clear()
        .envs(env.iter().map(|(key, value)| (*key, value.as_str())))
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&search.stdout).contains("\"chat_id\":\"first\""));

    // A second run keeps the configuration and existing registrations.
    std::fs::write(
        &config,
        std::fs::read_to_string(&config).unwrap() + "# edited\n",
    )
    .unwrap();
    let (_registered_home, registered) = fixture(true);
    let mut env = env;
    env.retain(|(key, _)| *key != "PATH");
    env.extend(registered.into_iter().filter(|(key, _)| *key == "PATH"));
    let again = init(&env, &["--no-index"]);
    assert_eq!(again["config"]["written"], false);
    assert!(again.get("index").is_none());
    assert_eq!(
        again["agents"][0]["mcp"]["state"], "already-present",
        "{again}"
    );
    assert!(
        std::fs::read_to_string(&config)
            .unwrap()
            .ends_with("# edited\n")
    );

    let forced = init(&env, &["--force", "--no-index", "--no-agents"]);
    assert_eq!(forced["config"]["written"], true);
    assert_eq!(forced["agents"], serde_json::json!([]));
    assert!(home.join(".config/mash/config.toml.previous").exists());
    assert_eq!(
        forced["config"]["previous"],
        home.join(".config/mash/config.toml.previous")
            .display()
            .to_string()
    );
    cleanup_local_gold(&home.join(".local/share/mash/mash.sqlite3"), "local");
}

#[test]
fn init_reports_why_a_plugin_could_not_be_installed() {
    // Offline, adding the marketplace fails and installing then cannot find
    // the plugin; the second error alone would not say why.
    let (_root, env) = fixture_with(
        false,
        r#"if [ "$2" = marketplace ]; then echo 'getaddrinfo ENOTFOUND github.com' >&2; else echo 'plugin not found' >&2; fi; exit 1"#,
    );
    let value = init(&env, &["--no-index"]);
    for agent in &value["agents"].as_array().unwrap()[..] {
        assert_eq!(agent["plugin"]["state"], "failed", "{value}");
        let message = agent["plugin"]["message"].as_str().unwrap();
        assert!(message.contains("plugin not found"), "{message}");
        assert!(message.contains("ENOTFOUND github.com"), "{message}");
    }
    // The next steps say what still works and how to finish, per agent.
    let next = value["next"].to_string();
    for agent in ["claude-code", "codex"] {
        assert!(
            next.contains(&format!("{agent}: Mash MCP search works")),
            "{next}"
        );
    }
    assert!(next.contains("Restart your agents"), "{next}");
}

#[test]
fn init_without_history_writes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let env = [
        ("HOME", root.path().display().to_string()),
        (
            "XDG_CONFIG_HOME",
            root.path().join(".config").display().to_string(),
        ),
        ("PATH", "/usr/bin:/bin".to_owned()),
    ];
    let output = Command::cargo_bin("mash")
        .unwrap()
        .arg("init")
        .env_clear()
        .envs(env.iter().map(|(key, value)| (*key, value.as_str())))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no supported agent history"));
    assert!(!root.path().join(".config/mash/config.toml").exists());
}

#[test]
fn init_yes_accepts_defaults_and_preserves_existing_configuration() {
    let (root, env) = fixture(false);
    let result = init(&env, &["--yes", "--no-index", "--no-agents"]);
    assert_eq!(result["config"]["written"], true);
    assert_eq!(result["agents"], serde_json::json!([]));
    assert!(!root.path().join("calls.log").exists());
    let path = root.path().join(".config/mash/config.toml");
    let original = std::fs::read_to_string(&path).unwrap() + "\n# keep me\n";
    std::fs::write(&path, &original).unwrap();
    let result = init(&env, &["--yes", "--no-index", "--no-agents"]);
    assert_eq!(result["config"]["written"], false);
    assert_eq!(std::fs::read_to_string(path).unwrap(), original);
}
