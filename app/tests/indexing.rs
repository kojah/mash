use super::*;

mod incomplete;
mod rebuild;

#[test]
#[ignore = "requires disposable Qdrant"]
fn users_only_cli_index_keeps_assistant_lexical_search_without_model_assets() {
    let root = tempfile::tempdir().unwrap();
    let provider = root.path().join("provider");
    std::fs::create_dir(&provider).unwrap();
    std::fs::write(provider.join("session.jsonl"), concat!(
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"thread\"}}\n",
        "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"id\":\"reply\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"needle assistant evidence\"}]}}\n",
    )).unwrap();
    let config = root.path().join("node.toml");
    std::fs::write(&config, format!("version=1\n[providers]\ncodex={{roots=[{}]}}\n[views.build]\ndense_content='user-messages'\n[views.query]\nintent='exact-mention'\n", toml::Value::String(provider.to_str().unwrap().into()))).unwrap();
    let database = root.path().join("silver.sqlite3");
    configure_qdrant(&config);
    let models = root.path().join("absent-models");
    let base = [
        "--config",
        config.to_str().unwrap(),
        "--db",
        database.to_str().unwrap(),
        "--models-dir",
        models.to_str().unwrap(),
    ];
    let indexed = run_json(&[base.as_slice(), &["index"]].concat());
    assert_eq!(indexed["model_inferences"], 0);
    assert_eq!(indexed["embeddings"], 0);
    assert_eq!(indexed["searchable_messages"], 1);
    assert_eq!(indexed["views"]["build"]["dense_content"], "user-messages");
    std::fs::remove_dir_all(provider).unwrap();
    let before = std::fs::read(&database).unwrap();
    let result = run_json(
        &[
            base.as_slice(),
            &["search", "needle", "--local", "--unit", "messages"],
        ]
        .concat(),
    );
    assert_eq!(result["results"].as_array().unwrap().len(), 1);
    assert_eq!(result["results"][0]["author"]["id"], "assistant");
    assert_eq!(result["results"][0]["author"]["kind"], "bot");
    assert_eq!(std::fs::read(&database).unwrap(), before);
    assert!(!models.exists());
    cleanup_local_gold(&database, "local");
}

#[test]
#[ignore = "requires disposable Qdrant"]
fn lexical_only_cli_indexes_without_models_and_rejects_hybrid_overrides() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("node.toml");
    let database = root.path().join("index.sqlite3");
    let models = root.path().join("absent-models");
    std::fs::write(&config, "version=1\nproviders={}\n[views.build]\nrepresentations='lexical-only'\n[views.query]\nintent='exact-mention'\n").unwrap();
    configure_qdrant(&config);
    let base = [
        "--config",
        config.to_str().unwrap(),
        "--db",
        database.to_str().unwrap(),
        "--models-dir",
        models.to_str().unwrap(),
    ];
    let indexed = run_json(&[base.as_slice(), &["index"]].concat());
    assert_eq!(indexed["realized_mode"], "lexical");
    assert_eq!(indexed["model_inferences"], 0);
    assert_eq!(indexed["views"]["build"]["representations"], "lexical-only");
    let before = std::fs::read(&database).unwrap();
    let status = run_json(&[base.as_slice(), &["status"]].concat());
    assert_eq!(status["ready"], true);
    assert_eq!(
        status["local_artifacts"],
        serde_json::json!({"metadata":"ready","lexical":"ready","dense":"disabled"})
    );
    let result = run_json(&[base.as_slice(), &["search", "needle", "--local"]].concat());
    assert_eq!(result["realized_mode"], "lexical");
    let output = Command::cargo_bin("mash")
        .unwrap()
        .args(base)
        .args([
            "search",
            "needle",
            "--local",
            "--intent",
            "related-discussion",
        ])
        .output()
        .unwrap();
    assert_eq!(error_kind(&output), "usage");
    assert_eq!(std::fs::read(&database).unwrap(), before);
    assert!(!models.exists());
    cleanup_local_gold(&database, "local");
}

#[test]
fn index_without_models_fails_before_creating_the_database() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database = directory.path().join("mash.sqlite3");
    let models = directory.path().join("models");
    // An explicit configuration keeps this from reading the developer's own,
    // whose provider roots and embedding backend would start a real index.
    let config = directory.path().join("config.toml");
    let claude = directory.path().join("claude");
    let codex = directory.path().join("codex");
    std::fs::create_dir_all(&claude).expect("Claude root");
    std::fs::create_dir_all(&codex).expect("Codex root");
    write_config(&config, &claude, &codex);
    let output = Command::cargo_bin("mash")
        .expect("mash binary")
        .args([
            "--config",
            config.to_str().unwrap(),
            "--db",
            database.to_str().unwrap(),
            "--models-dir",
            models.to_str().unwrap(),
            "index",
        ])
        .output()
        .expect("run index");

    assert!(!output.status.success());
    assert_eq!(output.stdout, Vec::<u8>::new());
    let error: Value = serde_json::from_slice(&output.stderr).expect("JSON error");
    assert_eq!(error["error"]["kind"], "model");
    assert_eq!(error["error"]["recommended_action"], "models install");
    assert!(!database.exists());
    assert!(!models.exists());
}

#[test]
#[ignore = "requires disposable Qdrant"]
fn index_without_pending_embeddings_validates_but_does_not_load_models() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database = directory.path().join("mash.sqlite3");
    let models = directory.path().join("models");
    let config = directory.path().join("config.toml");
    let claude = directory.path().join("claude");
    let codex = directory.path().join("codex");
    std::fs::create_dir_all(&claude).expect("Claude root");
    std::fs::create_dir_all(&codex).expect("Codex root");
    seed_readiness_database(&database, Some(""));
    create_valid_model_marker(&models);
    write_config(&config, &claude, &codex);
    configure_qdrant(&config);

    let output = Command::cargo_bin("mash")
        .expect("mash binary")
        .args(["--config", config.to_str().unwrap()])
        .args(["--db", database.to_str().unwrap()])
        .args(["--models-dir", models.to_str().unwrap()])
        .arg("index")
        .output()
        .expect("run no-op index");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let indexed: Value = serde_json::from_slice(&output.stdout).expect("index JSON");
    assert_eq!(indexed["embeddings"], 0);
    assert_eq!(indexed["model_load_milliseconds"], 0);
    cleanup_local_gold(&database, "local");
}
