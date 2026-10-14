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
    std::fs::write(&config, format!("version=1\n[providers.claude-code]\nroots=[{}]\n[views.build]\nrepresentations='lexical-only'\n[views.query]\nintent='exact-mention'\n", toml::Value::String(root.to_str().unwrap().into()))).unwrap();
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
    let args = [base.as_slice(), &["search", "if", "--local"]].concat();

    let response = run_json(&args);
    let anchor = &response["results"][0]["matches"][0];
    let snippet = anchor["snippet"].as_str().unwrap();
    assert!(snippet.contains("(IF)"));
    assert!(!snippet.contains("gift"));
    let start = usize::try_from(anchor["projection_span"]["start_char"].as_u64().unwrap()).unwrap();
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
fn cli_search_pages_partition_one_ranked_list() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("claude");
    std::fs::create_dir(&root).unwrap();
    for thread in 0..5 {
        let lines = (0..3)
            .map(|message| {
                serde_json::to_string(&serde_json::json!({
                    "type":"user", "sessionId":format!("thread-{thread}"),
                    "uuid":format!("message-{thread}-{message}"),
                    "timestamp":format!("2026-01-0{}T00:00:0{message}Z", thread + 1),
                    "message":{"role":"user","content":format!(
                        "pagination sentinel {} in thread {thread} message {message}",
                        "word ".repeat(thread * 3 + message)
                    )}
                }))
                .unwrap()
            })
            .collect::<Vec<_>>();
        std::fs::write(
            root.join(format!("thread-{thread}.jsonl")),
            lines.join("\n") + "\n",
        )
        .unwrap();
    }
    let config = directory.path().join("config.toml");
    std::fs::write(&config, format!("version=1\n[providers.claude-code]\nroots=[{}]\n[views.build]\nrepresentations='lexical-only'\n[views.query]\nintent='exact-mention'\n", toml::Value::String(root.to_str().unwrap().into()))).unwrap();
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
    let key = |unit: &str, result: &Value| -> Vec<String> {
        let field = if unit == "chats" { "chat_id" } else { "id" };
        result["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row[field].as_str().unwrap().to_owned())
            .collect()
    };
    for (unit, total) in [("chats", 5), ("messages", 15)] {
        let search = |extra: &[&str]| {
            run_json(
                &[
                    base.as_slice(),
                    &["search", "sentinel", "--local", "--unit", unit],
                    extra,
                ]
                .concat(),
            )
        };
        let whole = search(&["--limit", "50"]);
        assert_eq!(whole["next_offset"], Value::Null);
        let expected = key(unit, &whole);
        assert_eq!(expected.len(), total, "{unit}");
        let mut paged = Vec::new();
        let mut recent = Vec::new();
        let mut offset = 0;
        loop {
            let offset_text = offset.to_string();
            let page = search(&["--limit", "2", "--offset", &offset_text]);
            let mut members = key(unit, &page);
            paged.extend(members.clone());
            // Recency orders only the page; its members are unchanged.
            let mut reordered = key(
                unit,
                &search(&["--limit", "2", "--offset", &offset_text, "--sort", "recent"]),
            );
            reordered.sort();
            members.sort();
            assert_eq!(reordered, members, "{unit} page {offset}");
            recent.extend(reordered);
            let Some(next) = page["next_offset"].as_u64() else {
                break;
            };
            let next = usize::try_from(next).unwrap();
            assert_eq!(next, offset + 2);
            offset = next;
        }
        assert_eq!(paged, expected, "{unit} pages must partition one ranking");
        assert_eq!(offset, (total - 1) / 2 * 2, "{unit} last page");
        recent.sort();
        let mut sorted = expected.clone();
        sorted.sort();
        assert_eq!(recent, sorted);
        // An offset past the results but inside the window is an empty last page.
        let empty = search(&["--offset", "40"]);
        assert_eq!(empty["results"], serde_json::json!([]));
        assert_eq!(empty["next_offset"], Value::Null);
    }
}

#[test]
fn cli_search_pages_outside_the_ranked_window_are_usage_errors() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    std::fs::write(&config, "version=1\nproviders={}\n").unwrap();
    let database = directory.path().join("missing.sqlite3");
    for page in [
        ["--limit", "0", "--unit", "chats"],
        ["--limit", "201", "--unit", "chats"],
        ["--offset", "200", "--unit", "chats"],
        ["--offset", "50", "--unit", "messages"],
    ] {
        let mut command = Command::cargo_bin("mash").unwrap();
        command
            .args(["--config", config.to_str().unwrap()])
            .args(["--db", database.to_str().unwrap()])
            .args(["search", "cbioportal", "--local"])
            .args(page);

        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{page:?}");
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["kind"], "usage");
        assert!(
            error["error"]["message"]
                .as_str()
                .unwrap()
                .contains("search pages require")
        );
    }
    assert!(!database.exists());
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
    let args = [
        base.as_slice(),
        &["search", "for", "--intent", "exact-mention", "--local"],
    ]
    .concat();

    let result = run_json(&args);
    assert_eq!(result["intent"], "exact-mention");
    assert_eq!(result["realized_mode"], "lexical");
    assert_eq!(result["results"], serde_json::json!([]));
    std::fs::write(
        &config,
        "version=1\nproviders={}\n[views.query]\nintent='exact-mention'\nunit='messages'\nlimit=3\n",
    )
    .unwrap();
    let result = run_json(&[base.as_slice(), &["search", "for", "--local"]].concat());
    assert_eq!(result["unit"], "messages");
    assert_eq!(result["intent"], "exact-mention");
    assert_eq!(std::fs::read(&database).unwrap(), before);
    assert!(!models.exists());
}

#[test]
fn cli_validates_search_before_accessing_models_or_index() {
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
        let mut command = Command::cargo_bin("mash").unwrap();
        command.args(base).args(&arguments);

        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(output.stdout, [] as [u8; 0]);
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["kind"], "usage");
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
