use super::*;

#[test]
fn exact_cli_thread_excerpts_show_the_literal_and_preserve_canonical_view() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("claude");
    std::fs::create_dir(&root).unwrap();
    let content = format!("gift {} (IF) {}", "🦀 ".repeat(1200), "界".repeat(1200));
    std::fs::write(
        root.join("thread.jsonl"),
        serde_json::to_vec(&serde_json::json!({
            "type":"user", "sessionId":"thread", "uuid":"message",
            "message":{"role":"user","content":content}
        }))
        .unwrap(),
    )
    .unwrap();
    let config = directory.path().join("config.toml");
    std::fs::write(&config, format!("version=1\n[providers.claude-code]\nroots=[{}]\n[gold.build]\nrepresentations='lexical-only'\n[gold.query]\nintent='exact-mention'\n", toml::Value::String(root.to_str().unwrap().into()))).unwrap();
    let database = directory.path().join("index.sqlite3");
    let models = directory.path().join("missing-models");
    let base = [
        "--config",
        config.to_str().unwrap(),
        "--db",
        database.to_str().unwrap(),
        "--models-dir",
        models.to_str().unwrap(),
    ];
    run_json(&[base.as_slice(), &["index"]].concat());
    std::fs::remove_dir_all(root).unwrap();
    let before = std::fs::read(&database).unwrap();
    for direct in [false, true] {
        let mut args = [base.as_slice(), &["search", "if", "--local"]].concat();
        if direct {
            args.push("--no-worker");
        }
        let response = run_json(&args);
        let anchor = &response["results"][0]["matches"][0];
        let snippet = anchor["snippet"].as_str().unwrap();
        assert!(snippet.contains("(IF)"));
        assert!(!snippet.contains("gift"));
        let start =
            usize::try_from(anchor["projection_span"]["start_char"].as_u64().unwrap()).unwrap();
        let end = usize::try_from(anchor["projection_span"]["end_char"].as_u64().unwrap()).unwrap();
        // This fixture's faithful user projection equals its canonical text.
        let excerpt: String = content.chars().skip(start).take(end - start).collect();
        assert_eq!(snippet, format!("…{excerpt}…"));
        let viewed = run_json(
            &[
                base.as_slice(),
                &[
                    "view",
                    anchor["message_id"].as_str().unwrap(),
                    "--context",
                    "0",
                ],
            ]
            .concat(),
        );
        assert_eq!(viewed["messages"][0]["content"], content);
    }
    let diagnosed = run_json(
        &[
            base.as_slice(),
            &[
                "search",
                "if",
                "--local",
                "--diagnostics",
                "--provider",
                "claude-code",
                "--role",
                "user",
                "--filter",
                r#"{"not":{"project":["private-filter-canary"]}}"#,
            ],
        ]
        .concat(),
    );
    assert_eq!(
        diagnosed["diagnostics"]["filter_fields"],
        serde_json::json!(["provider", "role", "project"])
    );
    let stage = &diagnosed["diagnostics"]["stages"][0];
    assert_eq!(stage["stage"], "local-exact");
    assert_eq!(stage["filters_before_limit"], true);
    assert_eq!(stage["completed_calls"], 1);
    assert!(
        !serde_json::to_string(&diagnosed["diagnostics"])
            .unwrap()
            .contains("private-filter-canary")
    );
    assert_eq!(std::fs::read(database).unwrap(), before);
    assert!(!models.exists());
}

#[test]
fn exact_cli_search_needs_no_models_and_query_defaults_do_not_modify_the_index() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    let database = directory.path().join("index.sqlite3");
    let models = directory.path().join("models");
    std::fs::write(&config, "version=1\nproviders={}\n").unwrap();
    create_valid_model_marker(&models);
    let base = [
        "--config",
        config.to_str().unwrap(),
        "--db",
        database.to_str().unwrap(),
        "--models-dir",
        models.to_str().unwrap(),
    ];
    run_json(&[base.as_slice(), &["index"]].concat());
    std::fs::remove_dir_all(&models).unwrap();
    let before = std::fs::read(&database).unwrap();
    for direct in [false, true] {
        let mut args = [
            base.as_slice(),
            &["search", "for", "--intent", "exact-mention", "--local"],
        ]
        .concat();
        if direct {
            args.push("--no-worker");
        }
        let result = run_json(&args);
        assert_eq!(result["intent"], "exact-mention");
        assert_eq!(result["realized_mode"], "lexical");
        assert_eq!(result["results"], serde_json::json!([]));
    }
    std::fs::write(
        &config,
        "version=1\nproviders={}\n[gold.query]\nintent='exact-mention'\nunit='messages'\nlimit=3\n",
    )
    .unwrap();
    let result = run_json(&[base.as_slice(), &["search", "for", "--local"]].concat());
    assert_eq!(result["unit"], "messages");
    assert_eq!(result["intent"], "exact-mention");
    assert_eq!(std::fs::read(&database).unwrap(), before);
    assert!(!models.exists());
}

#[test]
fn cached_and_direct_cli_validation_match_with_installed_models_and_no_index() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    std::fs::write(&config, "version = 1\nproviders = {}\n").unwrap();
    let database = directory.path().join("missing.sqlite3");
    let models = directory.path().join("models");
    create_valid_model_marker(&models);
    let base = [
        "--config",
        config.to_str().unwrap(),
        "--db",
        database.to_str().unwrap(),
        "--models-dir",
        models.to_str().unwrap(),
    ];
    for arguments in [
        vec![
            "search",
            "cbioportal",
            "--shared-vector-search",
            "exact",
            "--local",
        ],
        vec![
            "search",
            "cbioportal",
            "--shared-vector-search",
            "hnsw",
            "--shared-hnsw-ef",
            "0",
        ],
        vec![
            "search",
            "cbioportal",
            "--shared-vector-search",
            "exact",
            "--shared-hnsw-ef",
            "80",
        ],
        vec![
            "search",
            "cbioportal",
            "--shared-vector-search",
            "hnsw",
            "--intent",
            "exact-mention",
        ],
        vec!["search", "cbioportal", "--lexical-weight", "0", "--local"],
        vec![
            "search",
            "cbioportal",
            "--fusion-rank-constant",
            "1001",
            "--local",
        ],
        vec![
            "search",
            "cbioportal",
            "--semantic-weight",
            "4",
            "--intent",
            "exact-mention",
            "--local",
        ],
        vec!["search", "the and of", "--local"],
        vec![
            "search",
            "wie konfiguriere ich den gemeinsamen Suchindex",
            "--local",
        ],
        vec![
            "search",
            "cbioportal",
            "--filter",
            r#"{"project":[""]}"#,
            "--local",
        ],
    ] {
        let mut errors = Vec::new();
        for direct in [false, true] {
            let mut command = Command::cargo_bin("mash").unwrap();
            command.args(base).args(&arguments);
            if direct {
                command.arg("--no-worker");
            }
            let output = command.output().unwrap();
            assert_eq!(output.status.code(), Some(2));
            assert_eq!(output.stdout, [] as [u8; 0]);
            let error: Value = serde_json::from_slice(&output.stderr).unwrap();
            assert_eq!(error["error"]["kind"], "usage");
            errors.push(error);
        }
        assert_eq!(errors[0], errors[1]);
        assert!(!database.exists());
    }
}

#[test]
fn unsupported_language_is_explained_before_model_or_index_access() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    std::fs::write(&config, "version = 1\nproviders = {}\n").unwrap();
    let database = directory.path().join("missing.sqlite3");
    let models = directory.path().join("missing-models");
    let output = Command::cargo_bin("mash")
        .unwrap()
        .args([
            "--config",
            config.to_str().unwrap(),
            "--db",
            database.to_str().unwrap(),
            "--models-dir",
            models.to_str().unwrap(),
            "search",
            "wie konfiguriere ich den gemeinsamen Suchindex",
            "--local",
            "--no-worker",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output.stdout, [] as [u8; 0]);
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["kind"], "usage");
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("German")
    );
    assert!(!database.exists());
    assert!(!models.exists());
}

#[test]
fn search_without_models_fails_before_database_access() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let models = directory.path().join("models");
    let output = Command::cargo_bin("mash")
        .expect("mash binary")
        .args([
            "--db",
            directory.path().join("missing.sqlite3").to_str().unwrap(),
            "--models-dir",
            models.to_str().unwrap(),
            "search",
            "needle",
        ])
        .output()
        .expect("run search");

    assert!(!output.status.success());
    assert_eq!(output.stdout, Vec::<u8>::new());
    let error: Value = serde_json::from_slice(&output.stderr).expect("JSON error");
    assert_eq!(error["error"]["kind"], "model");
    assert_eq!(error["error"]["recommended_action"], "models install");
    assert!(!models.exists());
}

#[test]
fn search_with_missing_embeddings_recommends_index_without_loading_models() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let models = directory.path().join("models");
    let database = directory.path().join("mash.sqlite3");
    create_valid_model_marker(&models);
    seed_readiness_database(&database, None);

    let output = Command::cargo_bin("mash")
        .expect("mash binary")
        .args([
            "--db",
            database.to_str().unwrap(),
            "--models-dir",
            models.to_str().unwrap(),
            "search",
            "needle",
        ])
        .output()
        .expect("run search");

    assert!(!output.status.success());
    assert_eq!(output.stdout, Vec::<u8>::new());
    let error: Value = serde_json::from_slice(&output.stderr).expect("JSON error");
    assert_eq!(error["error"]["kind"], "search-not-ready");
    assert_eq!(error["error"]["recommended_action"], "index");
}

#[test]
fn merged_search_requires_explicit_indexing_before_inference_or_http() {
    let directory = tempfile::tempdir().unwrap();
    let models = directory.path().join("models");
    create_valid_model_marker(&models); // Marker is valid; inference assets are intentionally unusable.
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        r#"
version = 1
providers = {}
[shared]
group_id = "test"
node_id = "test"
api_url = "http://127.0.0.1:9"
token_env = "MASH_TEST_UNSET_INDEX_READINESS_TOKEN"
"#,
    )
    .unwrap();
    let database = directory.path().join("local.sqlite3");
    for exists in [false, true] {
        if exists {
            seed_readiness_database(&database, None);
        }
        let before = std::fs::read(&database).ok();
        let output = Command::cargo_bin("mash")
            .unwrap()
            .args([
                "--config",
                config.to_str().unwrap(),
                "--db",
                database.to_str().unwrap(),
                "--models-dir",
                models.to_str().unwrap(),
                "search",
                "--no-worker",
                "needle",
            ])
            .output()
            .unwrap();
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(output.status.code(), Some(if exists { 8 } else { 3 }));
        assert_eq!(error["error"]["recommended_action"], "index");
        assert_eq!(output.stdout, Vec::<u8>::new());
        assert_eq!(std::fs::read(&database).ok(), before);
    }
}

#[test]
fn invalid_installed_assets_fail_without_lexical_results() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let models = directory.path().join("models");
    std::fs::create_dir_all(&models).expect("model root");
    std::fs::write(models.join("installed.json"), "not-json").expect("broken marker");
    let output = Command::cargo_bin("mash")
        .expect("mash binary")
        .args([
            "--db",
            directory.path().join("mash.sqlite3").to_str().unwrap(),
            "--models-dir",
            models.to_str().unwrap(),
            "search",
            "needle",
        ])
        .output()
        .expect("run search");

    assert!(!output.status.success());
    assert_eq!(output.stdout, Vec::<u8>::new());
    let error: Value = serde_json::from_slice(&output.stderr).expect("JSON error");
    assert_eq!(error["error"]["kind"], "model");
    assert_eq!(error["error"]["recommended_action"], "models install");
}

#[test]
fn missing_listed_model_asset_is_a_typed_model_error() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let models = directory.path().join("models");
    create_valid_model_marker(&models);
    std::fs::remove_file(models.join("dummy-model")).expect("remove listed model asset");
    let output = Command::cargo_bin("mash")
        .expect("mash binary")
        .args([
            "--db",
            directory.path().join("mash.sqlite3").to_str().unwrap(),
            "--models-dir",
            models.to_str().unwrap(),
            "search",
            "needle",
        ])
        .output()
        .expect("run search");

    assert!(!output.status.success());
    let error: Value = serde_json::from_slice(&output.stderr).expect("JSON error");
    assert_eq!(error["error"]["kind"], "model");
    assert_eq!(error["error"]["recommended_action"], "models install");
}

#[test]
fn view_returns_context_only_canonical_content_without_models() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database = directory.path().join("mash.sqlite3");
    seed_readiness_database(&database, Some(""));
    let viewed = run_json(&[
        "--db",
        database.to_str().unwrap(),
        "view",
        "message",
        "--context",
        "0",
    ]);
    assert_eq!(viewed["messages"][0]["content"], "canonical tool output");
}
