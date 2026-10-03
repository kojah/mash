use super::*;

#[test]
fn legacy_provider_root_environment_variables_are_ignored() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let home = directory.path().join("home");
    let config_home = directory.path().join("config-home");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&config_home).expect("config home");
    let injected = directory.path().join("injected");
    let output = Command::cargo_bin("mash")
        .expect("mash binary")
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &config_home)
        .env("MASH_CLAUDE_ROOTS", &injected)
        .env("MASH_CODEX_ROOTS", &injected)
        .env("MASH_OPENCODE_ROOTS", &injected)
        .env("MASH_COPILOT_ROOTS", &injected)
        .env("MASH_HERMES_ROOTS", &injected)
        .env("MASH_PI_ROOTS", &injected)
        .args([
            "--db",
            directory.path().join("missing.sqlite3").to_str().unwrap(),
            "status",
        ])
        .output()
        .expect("status");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body: Value = serde_json::from_slice(&output.stdout).expect("status JSON");
    assert_eq!(
        body["configuration"],
        serde_json::json!({
            "path": config_home.join("mash/config.toml"),
            "loaded": false,
            "shared": null,
            "gold": {"query": {"unit": "threads", "limit": 20}},
            "providers": {
                "claude-code": {"roots": [
                    home.join(".claude/projects"),
                    home.join(".config/claude/projects")
                ]},
                "codex": {"roots": [
                    home.join(".codex/sessions"),
                    home.join(".local/share/codex/sessions")
                ]}
            },
            "index": {"since_days": 90, "destination": "local"}
        })
    );
}

#[test]
fn every_public_command_rejects_malformed_configuration_before_effects() {
    let command_arguments: &[&[&str]] = &[
        &["index"],
        &["search", "needle"],
        &["view", "message"],
        &["status"],
        &["forget", "conversation"],
        &["models", "install"],
    ];

    for (arguments, explicit) in command_arguments
        .iter()
        .flat_map(|arguments| [(arguments, true), (arguments, false)])
    {
        let directory = tempfile::tempdir().expect("temporary directory");
        let xdg = directory.path().join("xdg");
        let config = if explicit {
            directory.path().join("config.toml")
        } else {
            xdg.join("mash/config.toml")
        };
        let database = directory.path().join("mash.sqlite3");
        let models = directory.path().join("models");
        std::fs::create_dir_all(config.parent().expect("config parent")).expect("config parent");
        std::fs::write(&config, "{").expect("malformed configuration");
        let mut command = Command::cargo_bin("mash").expect("mash binary");
        if explicit {
            command.args(["--config", config.to_str().unwrap()]);
        } else {
            command.env("XDG_CONFIG_HOME", &xdg);
        }
        let output = command
            .args(["--db", database.to_str().unwrap()])
            .args(["--models-dir", models.to_str().unwrap()])
            .args(*arguments)
            .output()
            .expect("run public command");

        assert!(
            !output.status.success(),
            "command unexpectedly passed: {arguments:?}"
        );
        assert_eq!(output.stdout, Vec::<u8>::new());
        let error: Value = serde_json::from_slice(&output.stderr).expect("one JSON error");
        assert_eq!(error["error"]["kind"], "configuration");
        assert_eq!(error["error"]["retryable"], false);
        assert!(error["error"].get("recommended_action").is_none());
        assert_eq!(output.status.code(), Some(9));
        assert!(!database.exists());
        assert!(!models.exists());
    }
}

#[test]
fn server_rejects_json_duplicate_keys_and_unknown_fields_before_connecting() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("server.toml");
    for contents in [
        r#"{"version":1,"database_url_env":"MISSING","nodes":[]}"#,
        "version = 1\nversion = 1\nnodes = []\n",
        "version = 1\nnodes = []\nunknown = true\n",
    ] {
        std::fs::write(&config, contents).unwrap();
        let output = Command::cargo_bin("mash")
            .unwrap()
            .args(["serve", "--server-config", config.to_str().unwrap()])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(9));
        assert_eq!(output.stdout, [] as [u8; 0]);
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["kind"], "configuration");
        assert_eq!(error["error"]["message"], "invalid server configuration");
    }
}

#[test]
fn public_index_flags_validate_before_models_database_or_scanning() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let config = directory.path().join("config.toml");
    let codex_root = directory.path().join("codex");
    std::fs::write(
        &config,
        toml::to_string(&toml::toml! {
            version = 1
            providers = {codex = {roots = [(codex_root.to_str().unwrap())]}}
            index = {since_days = 90}
        })
        .expect("config TOML"),
    )
    .expect("config");
    let database = directory.path().join("mash.sqlite3");
    let models = directory.path().join("models");
    for arguments in [
        vec!["--provider", "unsupported-provider"],
        vec!["--provider", "claude-code"],
        vec!["--since-days", "0"],
        vec!["--since-days", "30", "--all-history"],
    ] {
        let output = Command::cargo_bin("mash")
            .expect("mash binary")
            .args(["--config", config.to_str().unwrap()])
            .args(["--db", database.to_str().unwrap()])
            .args(["--models-dir", models.to_str().unwrap()])
            .arg("index")
            .args(arguments)
            .output()
            .expect("invalid index invocation");
        let error: Value = serde_json::from_slice(&output.stderr).expect("usage JSON");
        assert_eq!(error["error"]["kind"], "usage");
        assert!(!database.exists());
        assert!(!models.exists());
    }

    let accepted = Command::cargo_bin("mash")
        .expect("mash binary")
        .args(["--config", config.to_str().unwrap()])
        .args(["--db", database.to_str().unwrap()])
        .args(["--models-dir", models.to_str().unwrap()])
        .args([
            "index",
            "--provider",
            "codex",
            "--provider",
            "codex",
            "--since-days",
            "30",
        ])
        .output()
        .expect("accepted index invocation");
    let error: Value = serde_json::from_slice(&accepted.stderr).expect("model JSON");
    assert_eq!(error["error"]["kind"], "model");
    assert!(!database.exists());
}

#[test]
fn ssh_options_are_rejected_and_shared_access_requires_configuration() {
    for args in [
        vec!["view", "id", "--node", "remote"],
        vec!["search", "--federation-request"],
        vec!["view", "--federation-request"],
        vec!["--local-node", "node", "status"],
    ] {
        let output = Command::cargo_bin("mash")
            .expect("binary")
            .args(args)
            .output()
            .expect("run");
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(error_kind(&output), "usage");
    }
    let dir = tempfile::tempdir().expect("temp");
    let config = dir.path().join("config.toml");
    std::fs::write(
        &config,
        r#""version" = 1
"providers" = {}
"#,
    )
    .expect("config");
    for args in [
        vec!["search", "q", "--shared"],
        vec!["view", "id", "--shared", "node"],
    ] {
        let output = Command::cargo_bin("mash")
            .expect("binary")
            .args(["--config", config.to_str().unwrap()])
            .args(args)
            .output()
            .expect("run");
        assert_eq!(error_kind(&output), "configuration");
    }
}
