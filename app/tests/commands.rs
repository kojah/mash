use super::*;

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
        vec!["serve", "--help"],
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
            assert_eq!(response.as_object().unwrap().len(), 2);
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
    for arguments in [vec![], vec!["--unknown"], vec!["search"], vec!["serve"]] {
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
    for option in ["--stage-gold", "--discard-staged-gold"] {
        let output = Command::cargo_bin("mash")
            .expect("mash binary")
            .args([
                "serve",
                "--server-config",
                "nonexistent-server.toml",
                option,
            ])
            .output()
            .expect("mash serve");
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
    assert!(!manifest.contains("optional = true"));
    assert!(!manifest.contains("semantic = ["));
    assert!(ci.contains("cargo nextest run --profile ci"));
    assert!(!ci.contains("--no-default-features"));
    assert!(release.contains("cargo build --release --locked"));
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
