use super::*;

fn isolated(root: &Path) -> Command {
    let mut command = Command::cargo_bin("mash").unwrap();
    command
        .current_dir(root)
        .env("HOME", root)
        .env("CODEX_HOME", root.join(".codex"))
        .env("CLAUDE_CONFIG_DIR", root.join(".claude"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env_remove("MASH_TRACE");
    command
}

fn json(command: &mut Command) -> Value {
    let output = command.output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn installation_help_warns_only_when_integration_is_incomplete() {
    let root = tempfile::tempdir().unwrap();
    let clean = json(isolated(root.path()).arg("--help"));
    assert!(
        !clean["help"]
            .as_str()
            .unwrap()
            .contains("Installation warning")
    );
    std::fs::create_dir(root.path().join(".codex")).unwrap();
    let unconfigured = json(isolated(root.path()).arg("--help"));
    assert!(
        unconfigured["help"]
            .as_str()
            .unwrap()
            .contains("Installation notice")
    );
    let report = json(isolated(root.path()).args(["status", "--installation"]));
    assert_eq!(report["summary"]["unconfigured"], 1);
    assert_eq!(report["summary"]["attention"], 0);
    std::fs::write(
        root.path().join(".codex/config.toml"),
        "[mcp_servers.mash]\ncommand = '/does/not/exist'\n",
    )
    .unwrap();
    for args in [vec!["--help"], vec!["help"], vec!["status", "--help"]] {
        let help = json(isolated(root.path()).args(args));
        assert!(
            help["help"]
                .as_str()
                .unwrap()
                .contains("Installation warning")
        );
        assert!(
            help["help"]
                .as_str()
                .unwrap()
                .contains("mash status --installation")
        );
    }
}

#[test]
fn installation_inspection_bypasses_broken_mash_resources_and_is_read_only() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("broken.toml");
    std::fs::write(&config, "not a valid configuration").unwrap();
    let db = root.path().join("missing-db");
    let models = root.path().join("missing-models");
    let report = json(
        isolated(root.path())
            .arg("--config")
            .arg(&config)
            .arg("--db")
            .arg(&db)
            .arg("--models-dir")
            .arg(&models)
            .args(["status", "--installation", "--agent", "codex"]),
    );
    assert_eq!(report["clients"].as_array().unwrap().len(), 1);
    assert_eq!(report["summary"]["attention"], 3);
    assert!(report["resources"]["findings"].as_array().unwrap().iter().any(|finding| finding["component"] == "configuration" && finding["state"] == "invalid"));
    assert!(!db.exists());
    assert!(!models.exists());
    assert_eq!(
        std::fs::read_to_string(config).unwrap(),
        "not a valid configuration"
    );
}

#[test]
fn installation_reports_schema_mismatch_and_help_uses_selected_database() {
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("old.sqlite3");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection.execute_batch("PRAGMA user_version=45; CREATE TABLE retained(value TEXT); INSERT INTO retained VALUES('keep');").unwrap();
    drop(connection);
    let before = std::fs::read(&database).unwrap();
    let report = json(
        isolated(root.path())
            .arg("--db")
            .arg(&database)
            .args(["status", "--installation"]),
    );
    assert_eq!(report["resources"]["findings"][0]["state"], "outdated");
    assert!(
        report["resources"]["findings"][0]["message"]
            .as_str()
            .unwrap()
            .contains("45")
    );
    assert_eq!(report["summary"]["attention"], 1);
    assert!(report["resources"]["connectors"].is_null());
    for args in [vec!["--help"], vec!["help"], vec!["status", "--help"]] {
        let help = json(isolated(root.path()).arg("--db").arg(&database).args(args));
        assert!(
            help["help"]
                .as_str()
                .unwrap()
                .contains("local index is incompatible or unreadable"),
            "{help}"
        );
    }
    let help = json(
        isolated(root.path())
            .arg("--help")
            .arg("--db")
            .arg(&database),
    );
    assert!(
        help["help"]
            .as_str()
            .unwrap()
            .contains("local index is incompatible or unreadable")
    );
    assert_eq!(std::fs::read(&database).unwrap(), before);
    assert!(!root.path().join("old.views.sqlite3").exists());
}

#[test]
fn installation_detects_interrupted_upgrade_even_without_active_database() {
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("absent.sqlite3");
    let marker = root.path().join("absent.sqlite3.upgrade-in-progress");
    std::fs::create_dir(&marker).unwrap();
    std::fs::write(marker.join("backup"), "/retained/private-backup").unwrap();
    let report = json(
        isolated(root.path())
            .arg("--db")
            .arg(&database)
            .args(["status", "--installation"]),
    );
    assert_eq!(report["resources"]["findings"][0]["state"], "failed");
    assert_eq!(report["summary"]["attention"], 1);
    let help = json(
        isolated(root.path())
            .arg("--db")
            .arg(&database)
            .arg("--help"),
    );
    assert!(
        help["help"]
            .as_str()
            .unwrap()
            .contains("upgrade is unfinished")
    );
    assert!(!database.exists());
    assert!(marker.is_dir());
}

#[test]
fn installation_lists_provider_setup_without_claiming_missing_sources_are_connected() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("node.toml");
    let available = root.path().join("history");
    std::fs::create_dir(&available).unwrap();
    let missing = root.path().join("absent");
    std::fs::write(
        &config,
        format!(
            "version = 1\n[providers.codex]\nroots = [{}, {}]\n",
            toml::Value::String(available.to_string_lossy().into_owned()),
            toml::Value::String(missing.to_string_lossy().into_owned())
        ),
    )
    .unwrap();
    let report = json(
        isolated(root.path())
            .arg("--config")
            .arg(&config)
            .args(["status", "--installation"]),
    );
    let providers = report["resources"]["providers"].as_array().unwrap();
    assert_eq!(providers.len(), 6);
    let codex = providers
        .iter()
        .find(|provider| provider["provider"] == "codex")
        .unwrap();
    assert_eq!(codex["configured"], true);
    assert_eq!(codex["roots"][0]["state"], "present");
    assert_eq!(codex["roots"][1]["state"], "missing");
    assert_eq!(report["summary"]["attention"], 1);
    let help = json(
        isolated(root.path())
            .arg("--config")
            .arg(&config)
            .arg("--help"),
    );
    assert!(
        help["help"]
            .as_str()
            .unwrap()
            .contains("configured provider roots need attention")
    );
    assert!(codex["scan"].is_null());
    assert!(
        providers
            .iter()
            .filter(|provider| provider["provider"] != "codex")
            .all(|provider| provider["configured"] == false)
    );
    assert_eq!(
        report["resources"]["findings"][0]["state"],
        "not_configured"
    );
    assert!(!root.path().join("data").exists());
}

#[test]
fn installation_status_includes_summary_without_changing_search_readiness() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config.toml");
    std::fs::write(&config, "version = 1\nproviders = {}\n").unwrap();
    let status = json(
        isolated(root.path())
            .arg("--config")
            .arg(&config)
            .arg("--db")
            .arg(root.path().join("db"))
            .arg("--models-dir")
            .arg(root.path().join("models"))
            .arg("status"),
    );
    assert_eq!(status["ready"], false);
    assert_eq!(status["installation"]["attention"], 0);
    assert_eq!(
        status["installation"]["next_action"],
        "mash status --installation"
    );
}

#[test]
fn installation_probe_verifies_real_mash_without_index_or_models() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".codex")).unwrap();
    let executable = assert_cmd::cargo::cargo_bin!("mash").to_str().unwrap();
    let config = format!(
        "[mcp_servers.mash]\ncommand = {}\nargs = ['mcp']\n",
        toml::Value::String(executable.into())
    );
    std::fs::write(root.path().join(".codex/config.toml"), config).unwrap();
    let report = json(isolated(root.path()).args([
        "status",
        "--installation",
        "--agent",
        "codex",
        "--probe",
    ]));
    assert!(
        report["clients"][0]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["component"] == "probe" && finding["state"] == "passed"),
        "{report}"
    );
    // MCP may leave an empty runtime-registry directory. The probe must never
    // create an index, model files, or a live record after its child has exited.
    let data = root.path().join("data");
    if data.exists() {
        for entry in walkdir::WalkDir::new(data) {
            let entry = entry.unwrap();
            assert!(
                entry.file_type().is_dir(),
                "unexpected probe file: {:?}",
                entry.path()
            );
            assert_ne!(entry.file_name(), "models");
        }
    }
}

#[test]
fn installation_flags_reject_ambiguous_modes() {
    for args in [
        vec!["status", "--probe"],
        vec!["status", "--agent", "codex"],
        vec!["status", "--installation", "--shared"],
        vec!["status", "--installation", "--coverage"],
    ] {
        let root = tempfile::tempdir().unwrap();
        let output = isolated(root.path()).args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(error_kind(&output), "usage");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn installation_probe_timeout_terminates_child_and_keeps_help_passive() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".codex")).unwrap();
    std::fs::write(root.path().join(".codex/config.toml"), "[mcp_servers.mash]\ncommand = '/bin/sh'\nargs = ['-c', 'echo $$ > probe.pid; exec sleep 60']\n").unwrap();
    json(isolated(root.path()).arg("--help"));
    json(isolated(root.path()).args(["status", "--installation"]));
    assert!(!root.path().join("probe.pid").exists());
    let started = std::time::Instant::now();
    let report = json(isolated(root.path()).args([
        "status",
        "--installation",
        "--agent",
        "codex",
        "--probe",
    ]));
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
    assert!(
        report["clients"][0]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["component"] == "probe" && finding["state"] == "failed")
    );
    let pid = std::fs::read_to_string(root.path().join("probe.pid")).unwrap();
    let process = std::path::PathBuf::from(format!("/proc/{}", pid.trim()));
    for _ in 0..50 {
        if !process.exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("timed-out probe child survived CLI exit: {}", pid.trim());
}
