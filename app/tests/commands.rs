use super::*;

#[test]
fn standalone_server_has_separate_arguments_and_side_effect_free_information() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing.toml");
    for flag in ["--help", "-h", "--version", "-V"] {
        let output = Command::cargo_bin("mash-server")
            .unwrap()
            .args(["--server-config", missing.to_str().unwrap(), flag])
            .env_remove("MASH_TRACE")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stderr, [] as [u8; 0]);
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        if flag == "--help" || flag == "-h" {
            let help = response["help"].as_str().unwrap();
            assert!(help.contains("mash-server"));
            assert!(help.contains("--server-config"));
            assert!(!help.contains("--db"));
            assert!(!help.contains("--config "));
        } else {
            assert_eq!(response["version"], env!("CARGO_PKG_VERSION"));
            assert!(response["silver_schema"].as_i64().unwrap() > 0);
            let minimum = response["silver_schema_min"].as_i64().unwrap();
            assert!(minimum > 0 && minimum <= response["silver_schema"].as_i64().unwrap());
            assert_eq!(
                response["silver_schema_min"],
                run_json(&["--version"])["silver_schema_min"]
            );
        }
    }
    for args in [
        vec![],
        vec!["serve"],
        vec!["mcp"],
        vec!["dash"],
        vec!["--db", "somewhere"],
    ] {
        let output = Command::cargo_bin("mash-server")
            .unwrap()
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(super::error_kind(&output), "usage");
    }
    let output = Command::cargo_bin("mash")
        .unwrap()
        .args(["serve", "--help"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(super::error_kind(&output), "usage");
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn retired_worker_entry_points_are_usage_errors() {
    for args in [
        vec!["search", "needle", "--no-worker"],
        vec!["mcp", "--worker"],
        vec!["--internal-search-worker", "socket"],
    ] {
        let output = Command::cargo_bin("mash")
            .unwrap()
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(output.stdout, [] as [u8; 0]);
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["kind"], "usage");
    }
}

#[test]
fn built_in_views_flags_replace_gold_flags_without_aliases() {
    for (binary, prefix, current, retired) in [
        (
            "mash",
            vec!["index"],
            "--rebuild-local-views",
            "--rebuild-local-gold",
        ),
        ("mash-server", vec![], "--replay-views", "--replay-gold"),
        ("mash-server", vec![], "--activate-views", "--activate-gold"),
    ] {
        let output = Command::cargo_bin(binary)
            .unwrap()
            .args(&prefix)
            .arg("--help")
            .output()
            .unwrap();
        assert!(output.status.success());
        let help: Value = serde_json::from_slice(&output.stdout).unwrap();
        let help = help["help"].as_str().unwrap();
        assert!(help.contains(current), "{help}");
        assert!(!help.contains(retired), "{help}");
        let output = Command::cargo_bin(binary)
            .unwrap()
            .args(&prefix)
            .arg(retired)
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["kind"], "usage");
    }
}

#[test]
fn opt_in_tracing_preserves_cli_json_and_excludes_configured_paths() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    let database = directory.path().join("private-database-canary");
    std::fs::write(&config, "version = 1\nproviders = {}\n").unwrap();
    for enabled in [false, true] {
        let mut command = Command::cargo_bin("mash").unwrap();
        command.args([
            "--config",
            config.to_str().unwrap(),
            "--db",
            database.to_str().unwrap(),
            "status",
        ]);
        if enabled {
            command.env("MASH_TRACE", "1");
        } else {
            command.env_remove("MASH_TRACE");
        }
        let output = command.output().unwrap();
        assert!(output.status.success());
        let status: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(status["ready"], false);
        let diagnostics = String::from_utf8(output.stderr).unwrap();
        assert!(!diagnostics.contains("private-database-canary"));
        if enabled {
            assert!(diagnostics.contains("request_id"));
            for line in diagnostics.lines() {
                let _: Value = serde_json::from_str(line).unwrap();
            }
        } else {
            assert_eq!(diagnostics, "");
        }
    }
}

#[test]
fn bare_invocation_prints_help_without_starting_an_interface() {
    Command::cargo_bin("mash")
        .expect("mash binary")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage:"));
}

#[test]
fn informational_flags_succeed_without_initializing_application_resources() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("unavailable");
    for arguments in [
        vec!["--help"],
        vec!["-h"],
        vec!["search", "--help"],
        vec!["index", "--help"],
        vec!["mcp", "--help"],
        vec!["models", "install", "--help"],
        vec!["--version"],
        vec!["-V"],
    ] {
        let output = Command::cargo_bin("mash")
            .unwrap()
            .args(["--config", missing.to_str().unwrap()])
            .args(["--db", missing.to_str().unwrap()])
            .args(["--models-dir", missing.to_str().unwrap()])
            .args(&arguments)
            .env_remove("MASH_TRACE")
            .env("HOME", directory.path())
            .env("XDG_CONFIG_HOME", directory.path())
            .env("XDG_DATA_HOME", directory.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "{arguments:?}: {output:?}");
        assert!(output.stderr.is_empty(), "{arguments:?}");
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        if arguments.contains(&"--version") || arguments.contains(&"-V") {
            assert_eq!(response["version"], env!("CARGO_PKG_VERSION"));
            assert!(response["silver_schema"].as_i64().is_some_and(|v| v > 0));
            let minimum = response["silver_schema_min"].as_i64().unwrap();
            assert!(minimum > 0 && minimum <= response["silver_schema"].as_i64().unwrap());
            assert_eq!(response.as_object().unwrap().len(), 3);
        } else {
            assert_eq!(response.as_object().unwrap().len(), 1);
            let help = response["help"].as_str().unwrap();
            assert!(help.contains("Usage:"), "{arguments:?}");
            assert!(help.contains("--help"), "{arguments:?}");
        }
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }
}

#[test]
fn missing_and_invalid_cli_arguments_remain_json_usage_errors() {
    for arguments in [
        vec![],
        vec!["--unknown"],
        vec!["search"],
        vec!["serve"],
        vec!["index", "--connector-batch", "batch.json"],
        vec!["index", "--shared"],
    ] {
        let output = Command::cargo_bin("mash")
            .unwrap()
            .args(&arguments)
            .env_remove("MASH_TRACE")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(output.stdout.is_empty(), "{arguments:?}");
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["kind"], "usage", "{arguments:?}");
        assert_eq!(error["error"]["retryable"], false);
    }
}

#[test]
fn removed_commands_are_rejected() {
    for command in ["export", "doctor", "list", "sources"] {
        Command::cargo_bin("mash")
            .expect("mash binary")
            .arg(command)
            .assert()
            .failure();
    }
    for option in ["--stage-views", "--discard-staged-views"] {
        let output = Command::cargo_bin("mash-server")
            .expect("mash-server binary")
            .args(["--server-config", "nonexistent-server.toml", option])
            .output()
            .expect("mash-server");
        assert!(!output.status.success());
        assert_eq!(super::error_kind(&output), "usage");
    }
}

#[test]
fn cargo_and_ci_make_semantic_support_unconditional() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("Cargo manifest");
    let ci = std::fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("CI workflow");
    let release = std::fs::read_to_string(root.join(".github/workflows/release.yml"))
        .expect("release workflow");

    assert!(manifest.contains("fastembed = { version = \"6.0.1\", default-features = false"));
    let configuration: toml::Table = toml::from_str(&manifest).unwrap();
    let dependencies = configuration["dependencies"].as_table().unwrap();
    assert_ne!(
        dependencies["fastembed"]
            .get("optional")
            .and_then(toml::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        dependencies["console-subscriber"]
            .get("optional")
            .and_then(toml::Value::as_bool),
        Some(true)
    );
    assert!(!manifest.contains("semantic = ["));
    assert!(ci.contains("cargo nextest run --profile ci"));
    assert!(!ci.contains("--no-default-features"));
    assert!(release.contains("cargo +stable build --release --locked"));
    assert!(!release.contains("--no-default-features"));
    assert!(!root.join("app/semantic_disabled.rs").exists());
}

#[test]
fn maintained_repository_is_independent_and_minimal() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for removed in ["src", "tests", "benches", "fuzz", "scripts"] {
        assert!(!root.join(removed).exists(), "stale surface: {removed}");
    }
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("Cargo manifest");
    for removed in ["franken", "asupersync", "toon", "msgpack"] {
        assert!(!manifest.to_ascii_lowercase().contains(removed));
    }
}
