use super::*;

#[test]
fn concurrent_models_install_returns_a_retryable_error_before_changing_assets() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    std::fs::write(&config, "version = 1\nproviders = {}\n").unwrap();
    let models = directory.path().join("models");
    create_valid_model_marker(&models);
    let marker = std::fs::read(models.join("installed.json")).unwrap();
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(models.join(".mash-install.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    let output = Command::cargo_bin("mash")
        .unwrap()
        .args([
            "--config",
            config.to_str().unwrap(),
            "--models-dir",
            models.to_str().unwrap(),
            "models",
            "install",
        ])
        .timeout(std::time::Duration::from_secs(5))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(6));
    assert_eq!(output.stdout, [] as [u8; 0]);
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["kind"], "model-busy");
    assert_eq!(error["error"]["retryable"], true);
    assert_eq!(error["error"]["recommended_action"], "models install");
    assert_eq!(
        std::fs::read(models.join("installed.json")).unwrap(),
        marker
    );
    assert_eq!(std::fs::read_dir(&models).unwrap().count(), 3); // marker, fixture asset, lock
}

#[test]
fn status_reports_the_exact_resolved_local_configuration() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let config = directory.path().join("config.toml");
    let claude = directory.path().join("remote-claude");
    let codex = directory.path().join("remote-codex");
    write_config(&config, &claude, &codex);

    let body = run_json(&[
        "--config",
        config.to_str().unwrap(),
        "--db",
        directory.path().join("missing.sqlite3").to_str().unwrap(),
        "--models-dir",
        directory.path().join("missing-models").to_str().unwrap(),
        "status",
    ]);

    assert_eq!(
        body["configuration"],
        serde_json::json!({
            "path": std::fs::canonicalize(&config).expect("canonical config"),
            "loaded": true,
            "shared": null,
            "gold": {"build": {"embedding_text": "dialogue"}, "query": {
                "intent": "related-discussion", "unit": "threads", "limit": 20,
                "fusion": {"rank_constant": 60, "lexical_weight": 1, "semantic_weight": 1},
                "ranking": "contextual-rerank"
            }},
            "providers": {
                "claude-code": {"roots": [claude]},
                "codex": {"roots": [codex]}
            },
            "index": {"since_days": 30, "destination": "local"}
        })
    );
}

#[test]
fn status_without_models_recommends_install_without_downloading() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let models = directory.path().join("models");
    let body = run_json(&[
        "--db",
        directory.path().join("missing.sqlite3").to_str().unwrap(),
        "--models-dir",
        models.to_str().unwrap(),
        "status",
    ]);
    assert_eq!(body["ready"], false);
    assert_eq!(body["recommended_action"], "models install");
    assert!(!models.exists());
}

#[test]
fn oversized_model_metadata_fails_before_indexing_or_loading_and_status_explains_recovery() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config.toml");
    std::fs::write(&config, "version = 1\nproviders = {}\n").unwrap();
    let models = root.path().join("models");
    create_valid_model_marker(&models);
    let marker = std::fs::File::create(models.join("installed.json")).unwrap();
    marker.set_len(1024 * 1024 + 1).unwrap();
    let database = root.path().join("index.sqlite3");
    let base = [
        "--config",
        config.to_str().unwrap(),
        "--models-dir",
        models.to_str().unwrap(),
        "--db",
        database.to_str().unwrap(),
    ];
    let status = run_json(&[base.as_slice(), &["status"]].concat());
    assert_eq!(status["ready"], false);
    assert_eq!(status["recommended_action"], "models install");
    for args in [
        vec!["index"],
        vec!["search", "cbioportal", "--local", "--no-worker"],
    ] {
        let output = Command::cargo_bin("mash")
            .unwrap()
            .args(base)
            .args(args)
            .timeout(std::time::Duration::from_secs(5))
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(4));
        assert_eq!(output.stdout, [] as [u8; 0]);
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["kind"], "model");
        assert_eq!(error["error"]["recommended_action"], "models install");
        assert!(!database.exists());
    }
    assert_eq!(std::fs::read_dir(&models).unwrap().count(), 2);
    assert_eq!(marker.metadata().unwrap().len(), 1024 * 1024 + 1);
}

#[test]
fn status_with_models_but_no_database_recommends_index() {
    let fixture = NodeFixture::new();
    let status = fixture.status();
    assert_eq!(status["ready"], false);
    assert_eq!(status["recommended_action"], "index");
    assert!(!fixture.database.exists());
}

#[test]
fn status_with_missing_embeddings_recommends_index() {
    let fixture = NodeFixture::new();
    fixture.seed(None);
    let status = fixture.status();
    assert_eq!(status["searchable_messages"], 1);
    assert_eq!(status["embeddings"], 0);
    assert_eq!(status["ready"], false);
    assert_eq!(status["recommended_action"], "index");
}

#[test]
fn status_with_completed_embeddings_reports_hybrid_readiness() {
    let fixture = NodeFixture::new();
    fixture.seed(None);
    fixture.insert_embedding();
    fixture.complete_index();
    let status = fixture.status();
    assert_eq!(status["ready"], true);
    assert_eq!(status["realized_mode"], "hybrid");
    assert!(status["recommended_action"].is_null());
}

#[test]
fn status_with_completed_context_only_index_needs_no_embeddings() {
    let fixture = NodeFixture::new();
    fixture.seed(Some(""));
    fixture.complete_index();
    let status = fixture.status();
    assert_eq!(status["messages"], 1);
    assert_eq!(status["searchable_messages"], 0);
    assert_eq!(status["embeddings"], 0);
    assert_eq!(status["ready"], true);
    assert_eq!(status["realized_mode"], "hybrid");
    assert!(status["recommended_action"].is_null());
}

#[test]
fn explicit_empty_index_reports_ready_without_loading_models() {
    let directory = tempfile::tempdir().unwrap();
    let models = directory.path().join("models");
    create_valid_model_marker(&models);
    let config = directory.path().join("config.toml");
    std::fs::write(&config, "version = 1\nproviders = {}\n").unwrap();
    let database = directory.path().join("empty.sqlite3");
    let mut args = vec![
        "--config",
        config.to_str().unwrap(),
        "--db",
        database.to_str().unwrap(),
        "--models-dir",
        models.to_str().unwrap(),
        "index",
    ];
    let indexed = run_json(&args);
    assert_eq!(indexed["indexed_messages"], 0);
    assert_eq!(indexed["model_load_milliseconds"], 0);
    *args.last_mut().unwrap() = "status";
    let status = run_json(&args);
    assert_eq!(status["messages"], 0);
    assert_eq!(status["ready"], true);
    assert!(status["recommended_action"].is_null());
}

#[test]
fn status_reads_an_older_database_without_migrating_it() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let models = directory.path().join("models");
    create_valid_model_marker(&models);
    let database = directory.path().join("old.sqlite3");
    Connection::open(&database)
        .expect("old database")
        .execute_batch(
            "CREATE TABLE conversations (id TEXT PRIMARY KEY);
             CREATE TABLE messages (id TEXT PRIMARY KEY);
             CREATE TABLE message_embeddings (message_id TEXT PRIMARY KEY);
             INSERT INTO conversations VALUES ('conversation');
             INSERT INTO messages VALUES ('message');
             PRAGMA user_version = 7;",
        )
        .expect("seed old database");

    let body = run_json(&[
        "--db",
        database.to_str().unwrap(),
        "--models-dir",
        models.to_str().unwrap(),
        "status",
    ]);
    assert_eq!(body["conversations"], 1);
    assert_eq!(body["messages"], 1);
    assert_eq!(body["ready"], false);
    assert_eq!(body["recommended_action"], "index");

    let connection = Connection::open(&database).expect("reopen old database");
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("schema version");
    let projection_exists: bool = connection
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM pragma_table_info('messages') WHERE name = 'search_projection'
             )",
            [],
            |row| row.get(0),
        )
        .expect("projection check");
    assert_eq!(version, 7);
    assert!(!projection_exists);
}
