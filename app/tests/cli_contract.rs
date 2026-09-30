use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use rusqlite::Connection;
use serde_json::Value;

mod index_destinations;
mod shared_server;
mod watch_flow;

fn run_json(arguments: &[&str]) -> Value {
    let output = Command::cargo_bin("mash")
        .expect("mash binary")
        .args(arguments)
        .output()
        .expect("run mash");
    assert!(
        output.status.success(),
        "mash failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("JSON response")
}

fn error_kind(output: &std::process::Output) -> Value {
    serde_json::from_slice::<Value>(&output.stderr).unwrap_or_else(|error| {
        panic!(
            "typed error: {error}; status={:?}; stdout={}; stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })["error"]["kind"]
        .clone()
}

fn create_valid_model_marker(root: &Path) {
    std::fs::create_dir_all(root).expect("model root");
    std::fs::write(root.join("dummy-model"), [0_u8]).expect("dummy model asset");
    std::fs::write(
        root.join("installed.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema": 1,
            "embedding_model": "AllMiniLML6V2Q",
            "reranker_model": "jinaai/jina-reranker-v1-turbo-en",
            "embedding_dimensions": 384,
            "coreml_embedding": cfg!(all(target_os = "macos", target_arch = "aarch64")),
            "files": [{"path": "dummy-model", "size": 1}]
        }))
        .expect("model marker JSON"),
    )
    .expect("model marker");
}

fn seed_readiness_database(path: &Path, search_projection: Option<&str>) {
    let connection = Connection::open(path).expect("readiness database");
    connection
        .execute_batch(
            "CREATE TABLE conversations (
                id TEXT PRIMARY KEY, provider TEXT NOT NULL, source_path TEXT NOT NULL UNIQUE,
                title TEXT, created_at INTEGER, updated_at INTEGER, project TEXT, repository TEXT,
                source_fingerprint TEXT NOT NULL DEFAULT ''
             );
             CREATE TABLE messages (
                id TEXT PRIMARY KEY, conversation_id TEXT NOT NULL,
                ordinal INTEGER NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL,
                search_projection TEXT, created_at INTEGER,
                fingerprint TEXT NOT NULL DEFAULT '', UNIQUE(conversation_id, ordinal)
             );
             CREATE TABLE message_embeddings (
                message_id TEXT PRIMARY KEY, generation TEXT NOT NULL,
                dimensions INTEGER NOT NULL, norm REAL NOT NULL, vector BLOB NOT NULL
             );
             CREATE TABLE pending_embeddings (
                message_id TEXT PRIMARY KEY
             ) WITHOUT ROWID;
             CREATE VIRTUAL TABLE message_fts USING fts5(
                content, message_id UNINDEXED, conversation_id UNINDEXED,
                tokenize = 'unicode61'
             );
             CREATE TABLE tombstones (
                provider TEXT NOT NULL, conversation_id TEXT NOT NULL,
                forgotten_at INTEGER NOT NULL, PRIMARY KEY(provider, conversation_id)
             );
             CREATE TABLE source_checkpoints (
                provider TEXT NOT NULL, source_path TEXT NOT NULL,
                size_bytes INTEGER NOT NULL, modified_ns INTEGER NOT NULL,
                PRIMARY KEY(provider, source_path)
             );
             CREATE TABLE semantic_chunks (
                chunk_id INTEGER PRIMARY KEY, generation TEXT NOT NULL,
                dimensions INTEGER NOT NULL, vector_count INTEGER NOT NULL,
                message_rowids BLOB NOT NULL, norms BLOB NOT NULL,
                providers BLOB NOT NULL, created_ats BLOB NOT NULL, vectors BLOB NOT NULL
             );
             CREATE TABLE derived_state (
                singleton INTEGER PRIMARY KEY, search_dirty INTEGER NOT NULL,
                semantic_ready_generation TEXT
             );
             INSERT INTO derived_state VALUES (1, 0, NULL);
             INSERT INTO conversations(id, provider, source_path)
                VALUES ('session', 'codex', '/tmp/session.jsonl');
             PRAGMA user_version = 15;",
        )
        .expect("readiness schema");
    connection
        .execute(
            "INSERT INTO messages(
                id, conversation_id, ordinal, role, content, search_projection
             ) VALUES ('message', 'session', 0, 'tool', 'canonical tool output', ?1)",
            [search_projection],
        )
        .expect("readiness message");
}

fn current_embedding_generation() -> String {
    let specification = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        concat!(
            "fastembed=6.0.1;model=AllMiniLML6V2Q;",
            "backend=coreml-fp32;batch=32;sequence=512;",
            "vector=i8-per-vector-symmetric;",
            "cosine=quantized-flat-exact;schema=4"
        )
    } else {
        concat!(
            "fastembed=6.0.1;model=AllMiniLML6V2Q;",
            "batch=8;workers=8;threads=2;",
            "vector=i8-per-vector-symmetric;",
            "cosine=quantized-flat-exact;schema=3"
        )
    };
    blake3::hash(specification.as_bytes()).to_hex().to_string()
}

fn write_config(path: &Path, claude_root: &Path, codex_root: &Path) {
    std::fs::write(
        path,
        toml::to_string(&toml::toml! {
            version = 1
            providers = {
            claude-code = {roots = [(claude_root.to_str().unwrap())]},
            codex = {roots = [(codex_root.to_str().unwrap())]}
            }
            index = {since_days = 30}
        })
        .expect("configuration TOML"),
    )
    .expect("configuration file");
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
            "providers": {
                "claude-code": {"roots": [claude]},
                "codex": {"roots": [codex]}
            },
            "index": {"since_days": 30, "destination": "local"}
        })
    );
}

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
        vec!["--provider", "opencode"],
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
fn bare_invocation_prints_help_without_starting_an_interface() {
    Command::cargo_bin("mash")
        .expect("mash binary")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage:"));
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
}

#[test]
fn index_without_models_fails_before_creating_the_database() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database = directory.path().join("mash.sqlite3");
    let models = directory.path().join("models");
    let output = Command::cargo_bin("mash")
        .expect("mash binary")
        .args([
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
fn status_uses_model_then_database_then_embedding_readiness() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let models = directory.path().join("models");
    create_valid_model_marker(&models);

    let missing = run_json(&[
        "--db",
        directory.path().join("missing.sqlite3").to_str().unwrap(),
        "--models-dir",
        models.to_str().unwrap(),
        "status",
    ]);
    assert_eq!(missing["ready"], false);
    assert_eq!(missing["recommended_action"], "index");

    let incomplete_database = directory.path().join("incomplete.sqlite3");
    seed_readiness_database(&incomplete_database, None);
    let incomplete = run_json(&[
        "--db",
        incomplete_database.to_str().unwrap(),
        "--models-dir",
        models.to_str().unwrap(),
        "status",
    ]);
    assert_eq!(incomplete["searchable_messages"], 1);
    assert_eq!(incomplete["embeddings"], 0);
    assert_eq!(incomplete["ready"], false);
    assert_eq!(incomplete["recommended_action"], "index");

    let ready_database = directory.path().join("ready.sqlite3");
    seed_readiness_database(&ready_database, None);
    let ready_connection = Connection::open(&ready_database).expect("ready database");
    let generation = current_embedding_generation();
    ready_connection
        .execute(
            "INSERT INTO message_embeddings(message_id, generation, dimensions, norm, vector)
             VALUES ('message', ?1, 1, 127.0, X'7F')",
            [&generation],
        )
        .expect("current embedding");
    ready_connection
        .execute(
            "INSERT INTO semantic_chunks(
                chunk_id, generation, dimensions, vector_count, message_rowids,
                norms, providers, created_ats, vectors
             ) VALUES (0, ?1, 1, 1, ?2, ?3, X'02', ?4, X'7F')",
            rusqlite::params![
                generation,
                1_i64.to_le_bytes().to_vec(),
                127.0_f32.to_le_bytes().to_vec(),
                i64::MIN.to_le_bytes().to_vec()
            ],
        )
        .expect("packed semantic chunk");
    ready_connection
        .execute(
            "UPDATE derived_state SET semantic_ready_generation = ?1 WHERE singleton = 1",
            [&generation],
        )
        .expect("semantic readiness");
    let ready = run_json(&[
        "--db",
        ready_database.to_str().unwrap(),
        "--models-dir",
        models.to_str().unwrap(),
        "status",
    ]);
    assert_eq!(ready["ready"], true);
    assert_eq!(ready["realized_mode"], "hybrid");
    assert!(ready["recommended_action"].is_null());

    let context_only_database = directory.path().join("context-only.sqlite3");
    seed_readiness_database(&context_only_database, Some(""));
    Connection::open(&context_only_database)
        .unwrap()
        .execute(
            "UPDATE derived_state SET semantic_ready_generation = ?1 WHERE singleton = 1",
            [&generation],
        )
        .expect("explicitly completed context-only index");
    let context_only = run_json(&[
        "--db",
        context_only_database.to_str().unwrap(),
        "--models-dir",
        models.to_str().unwrap(),
        "status",
    ]);
    assert_eq!(context_only["messages"], 1);
    assert_eq!(context_only["searchable_messages"], 0);
    assert_eq!(context_only["embeddings"], 0);
    assert_eq!(context_only["ready"], true);
    assert_eq!(context_only["realized_mode"], "hybrid");
    assert!(context_only["recommended_action"].is_null());
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
    let readme = std::fs::read_to_string(root.join("README.md")).expect("README");
    for removed in ["OpenCode", "Copilot", "Hermes", "Pi histories"] {
        assert!(!readme.contains(removed), "stale provider docs: {removed}");
    }
}

#[test]
#[ignore = "requires MASH_TEST_MODELS_DIR containing an explicit model installation"]
fn real_models_index_and_run_hybrid_search() {
    let models = std::env::var_os("MASH_TEST_MODELS_DIR")
        .map(std::path::PathBuf::from)
        .expect("explicit model directory");
    let directory = tempfile::tempdir().expect("temporary directory");
    let claude = directory.path().join("claude");
    let codex = directory.path().join("codex");
    std::fs::create_dir_all(&claude).expect("Claude root");
    std::fs::create_dir_all(&codex).expect("Codex root");
    std::fs::write(
        claude.join("session.jsonl"),
        concat!(
            "{\"type\":\"user\",\"sessionId\":\"hybrid\",\"uuid\":\"m1\",",
            "\"message\":{\"role\":\"user\",\"content\":\"repair authentication credentials\"}}\n",
            "{\"type\":\"assistant\",\"sessionId\":\"hybrid\",\"uuid\":\"m2\",",
            "\"message\":{\"role\":\"assistant\",\"content\":[",
            "{\"type\":\"text\",\"text\":\"refresh the login token\"},",
            "{\"type\":\"tool_result\",\"content\":\"private-output-marker\"}]}}\n"
        ),
    )
    .expect("Claude fixture");
    let database = directory.path().join("mash.sqlite3");
    let config = directory.path().join("config.toml");
    write_config(&config, &claude, &codex);
    let model_arg = models.to_str().unwrap();
    let output = Command::cargo_bin("mash")
        .expect("mash binary")
        .args([
            "--config",
            config.to_str().unwrap(),
            "--db",
            database.to_str().unwrap(),
            "--models-dir",
            model_arg,
            "index",
            "--full",
        ])
        .output()
        .expect("index");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let indexed: Value = serde_json::from_slice(&output.stdout).expect("index JSON");
    assert_eq!(indexed["realized_mode"], "hybrid");
    assert_eq!(indexed["indexed_messages"], 2);
    assert_eq!(indexed["searchable_messages"], 2);
    let progress = output
        .stderr
        .split(|&byte| byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<Value>(line).expect("index progress JSON"))
        .collect::<Vec<_>>();
    assert_eq!(progress.last().unwrap()["phase"], "complete");
    assert!(
        progress
            .iter()
            .any(|event| event["phase"] == "semantic-embeddings"
                && event["eta_seconds"] == 0
                && event["stored_vectors"] == 2)
    );
    for timing in [
        "model_load_milliseconds",
        "storage_setup_milliseconds",
        "ingestion_milliseconds",
        "search_refresh_milliseconds",
        "embedding_milliseconds",
        "total_milliseconds",
    ] {
        assert!(indexed[timing].is_u64(), "missing index timing: {timing}");
    }

    let found = run_json(&[
        "--db",
        database.to_str().unwrap(),
        "--models-dir",
        model_arg,
        "search",
        "authentication",
    ]);
    assert_eq!(found["realized_mode"], "hybrid");
    assert!(found.get("fallback_mode").is_none());
    assert!(found.get("fallback_reason").is_none());
    assert!(found["results"][0]["lexical_score"].is_number());
    assert!(found["results"][0]["semantic_score"].is_number());
    assert!(found["results"][0]["rerank_score"].is_number());
    assert!(found["results"].as_array().is_some_and(|results| {
        results.iter().any(|result| {
            result["content"]
                .as_str()
                .is_some_and(|content| content.contains("authentication credentials"))
        })
    }));
}

#[test]
#[ignore = "requires explicit MASH_BENCHMARK_DB, MASH_BENCHMARK_CONFIG, and MASH_TEST_MODELS_DIR"]
fn benchmark_real_corpus_semantic_generation() {
    let source = std::env::var_os("MASH_BENCHMARK_DB")
        .map(std::path::PathBuf::from)
        .expect("explicit benchmark source database");
    let config = std::env::var_os("MASH_BENCHMARK_CONFIG")
        .map(std::path::PathBuf::from)
        .expect("explicit benchmark configuration");
    let models = std::env::var_os("MASH_TEST_MODELS_DIR")
        .map(std::path::PathBuf::from)
        .expect("explicit model directory");
    let directory = tempfile::tempdir().expect("temporary benchmark directory");
    let database = directory.path().join("benchmark.sqlite3");
    let source_connection =
        Connection::open_with_flags(&source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("open benchmark source");
    source_connection
        .execute("VACUUM INTO ?1", [database.to_string_lossy().as_ref()])
        .expect("snapshot benchmark database");
    drop(source_connection);
    let connection = Connection::open(&database).expect("open benchmark snapshot");
    connection
        .execute("DELETE FROM message_embeddings", [])
        .expect("clear derived embeddings");
    drop(connection);

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
        .expect("benchmark semantic index");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let indexed: Value = serde_json::from_slice(&output.stdout).expect("benchmark index JSON");
    let stored = indexed["embeddings"].as_u64().expect("stored embeddings");
    let inferred = indexed["model_inferences"]
        .as_u64()
        .expect("model inference count");
    let elapsed = indexed["embedding_milliseconds"]
        .as_u64()
        .expect("embedding elapsed time");
    assert!(inferred <= stored);
    #[allow(clippy::cast_precision_loss)]
    let rate = stored as f64 / (elapsed.max(1) as f64 / 1_000.0);
    eprintln!(
        "semantic benchmark stored={stored} inferred={inferred} elapsed_ms={elapsed} rate={rate:.1}"
    );
    assert!(rate >= 220.0, "semantic generation rate was {rate:.1}/s");
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

#[test]
#[ignore = "requires MASH_TEST_PG_URL pointing to an expendable PostgreSQL database"]
#[allow(clippy::too_many_lines)]
fn postgres_publication_retries_no_change_index_and_offline_forget_through_cli() {
    let dir = tempfile::tempdir().expect("temp");
    let database = dir.path().join("local.sqlite3");
    seed_readiness_database(&database, Some(""));
    Connection::open(&database)
        .expect("seed connection")
        .pragma_update(None, "user_version", 12)
        .expect("exercise migration");
    let models = dir.path().join("models");
    create_valid_model_marker(&models);
    let config = dir.path().join("config.toml");
    let group = format!(
        "cli-{}",
        dir.path().file_name().expect("name").to_string_lossy()
    );
    let server = shared_server::Server::new(dir.path(), &group, &["cli-node"]);
    std::fs::write(&config, toml::to_string(&toml::toml! {
        version = 1
        providers = {}
        shared = {
        group_id = (group.as_str()),node_id = "cli-node",api_url = (server.url.as_str()),token_env = "MASH_TEST_SHARED_CLIENT_TOKEN"
        }
    }).expect("TOML")).expect("config");
    let url = std::env::var("MASH_TEST_PG_URL").expect("test URL");
    let invoke = |args: &[&str], online: bool| {
        let mut command = Command::cargo_bin("mash").expect("binary");
        command.args([
            "--config",
            config.to_str().unwrap(),
            "--db",
            database.to_str().unwrap(),
            "--models-dir",
            models.to_str().unwrap(),
        ]);
        if online {
            command.env(
                "MASH_TEST_SHARED_CLIENT_TOKEN",
                shared_server::token("cli-node"),
            );
        } else {
            command.env_remove("MASH_TEST_SHARED_CLIENT_TOKEN");
        }
        let output = command.args(args).output().expect("run");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).expect("JSON response")
    };
    let first = invoke(&["index"], false);
    assert_eq!(first["shared"]["state"], "pending");
    let retry = invoke(&["index"], true);
    assert_eq!(retry["changed_messages"], 0);
    assert_eq!(retry["shared"]["state"], "published");
    let first_freshness = invoke(&["status", "--shared"], true)["shared_index"].clone();
    assert_eq!(first_freshness["group_id"], group);
    assert_eq!(first_freshness["nodes"][0]["node_id"], "cli-node");
    assert!(
        first_freshness["nodes"][0]["last_synced_at"]
            .as_str()
            .unwrap()
            .ends_with('Z')
    );
    invoke(&["index"], true);
    let unchanged = invoke(&["status", "--shared"], true)["shared_index"].clone();
    assert_eq!(
        unchanged["nodes"][0]["last_updated_at"],
        first_freshness["nodes"][0]["last_updated_at"]
    );
    assert!(
        unchanged["nodes"][0]["last_synced_at"].as_str()
            > first_freshness["nodes"][0]["last_synced_at"].as_str()
    );
    assert_eq!(
        invoke(&["view", "message", "--shared", "cli-node"], true)["messages"][0]["content"],
        "canonical tool output"
    );
    let forgotten = invoke(&["forget", "session"], false);
    assert_eq!(forgotten["forgotten"], true);
    assert_eq!(forgotten["shared"]["state"], "pending");
    assert_eq!(
        invoke(&["status", "--shared"], true)["shared_index"],
        unchanged,
        "failed upload does not advance remote freshness"
    );
    assert_eq!(
        invoke(&["view", "message", "--shared", "cli-node"], true)["messages"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "shared deletion is pending"
    );
    assert_eq!(invoke(&["index"], true)["shared"]["state"], "published");
    assert_eq!(
        invoke(&["view", "message", "--shared", "cli-node"], true)["messages"]
            .as_array()
            .unwrap()
            .as_slice(),
        &[] as &[serde_json::Value]
    );
    let schema = format!("mash_g_{}", &blake3::hash(group.as_bytes()).to_hex()[..32]);
    postgres::Client::connect(&url, postgres::NoTls)
        .expect("cleanup client")
        .batch_execute(&format!("DROP SCHEMA {schema} CASCADE"))
        .expect("cleanup");
}
