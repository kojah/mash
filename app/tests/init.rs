use super::*;

/// A home with Claude Code history and stand-in agent CLIs that log calls.
fn fixture(registered: bool) -> (tempfile::TempDir, Vec<(&'static str, String)>) {
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
        let get = if registered { 0 } else { 1 };
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\necho \"{agent} $*\" >> '{}'\n[ \"$1 $2\" = \"mcp get\" ] && exit {get}\nexit 0\n",
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
fn init_configures_registers_and_indexes_detected_history() {
    let (root, env) = fixture(false);
    let home = root.path();
    let value = init(&env, &[]);
    let config = home.join(".config/mash/config.toml");
    assert_eq!(value["config"]["written"], true, "{value}");
    assert_eq!(value["config"]["search"], "exact-mention");
    assert_eq!(
        value["config"]["providers"]["claude-code"][0],
        home.join(".claude/projects").display().to_string()
    );
    assert!(value["config"]["providers"].get("codex").is_none());
    assert_eq!(value["index"]["indexed_messages"], 1, "{value}");
    for agent in &value["agents"].as_array().unwrap()[..] {
        assert_eq!(agent["mcp"]["state"], "done", "{value}");
        assert_eq!(agent["plugin"]["state"], "done", "{value}");
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
