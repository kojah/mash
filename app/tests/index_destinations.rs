use super::*;

struct Fixture {
    directory: tempfile::TempDir,
    config: std::path::PathBuf,
    database: std::path::PathBuf,
    models: std::path::PathBuf,
    codex: std::path::PathBuf,
    group: String,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let config = root.join("config.json");
        let database = root.join("local/mash.sqlite3");
        let models = root.join("models");
        create_valid_model_marker(&models);
        let claude = root.join("claude");
        let codex = root.join("codex");
        std::fs::create_dir(&claude).unwrap();
        std::fs::create_dir(&codex).unwrap();
        std::fs::write(claude.join("session.jsonl"), r#"{"type":"user","sessionId":"claude-session","uuid":"claude-tool","message":{"role":"user","content":[{"type":"tool_result","content":"local Claude output"}]}}
"#).unwrap();
        Self::write_codex(&codex);
        let group = format!("routing-{}", root.file_name().unwrap().to_string_lossy());
        Self {
            directory,
            config,
            database,
            models,
            codex,
            group,
        }
    }

    fn write_codex(root: &Path) {
        std::fs::write(root.join("session.jsonl"), r#"{"type":"session_meta","payload":{"id":"codex-session"}}
{"type":"response_item","payload":{"type":"function_call_output","call_id":"codex-tool","output":"shared Codex output"}}
"#).unwrap();
    }

    fn configure(&self, destination: &str, node: &str) {
        std::fs::write(&self.config, serde_json::to_vec(&serde_json::json!({
            "version":1,
            "providers":{
                "claude-code":{"roots":[self.directory.path().join("claude")]},
                "codex":{"roots":[self.codex]}
            },
            "index":{"destination":destination,"local":{"providers":["claude-code"]},"shared":{"providers":["codex"]}},
            "shared":{"group_id":self.group,"node_id":node,"database_url_env":"MASH_TEST_ROUTING_URL"}
        })).unwrap()).unwrap();
    }

    fn run(&self, arguments: &[&str], online: bool) -> std::process::Output {
        let mut command = Command::cargo_bin("mash").unwrap();
        command.args([
            "--config",
            self.config.to_str().unwrap(),
            "--db",
            self.database.to_str().unwrap(),
            "--models-dir",
            self.models.to_str().unwrap(),
        ]);
        if online {
            command.env(
                "MASH_TEST_ROUTING_URL",
                std::env::var("MASH_TEST_PG_URL").unwrap(),
            );
        } else {
            command.env_remove("MASH_TEST_ROUTING_URL");
        }
        command.args(arguments).output().unwrap()
    }

    fn json(&self, arguments: &[&str], online: bool) -> Value {
        let output = self.run(arguments, online);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn client(&self) -> postgres::Client {
        let mut client =
            postgres::Client::connect(&std::env::var("MASH_TEST_PG_URL").unwrap(), postgres::NoTls)
                .unwrap();
        client
            .batch_execute(&format!(
                "SET search_path=mash_g_{}",
                &blake3::hash(self.group.as_bytes()).to_hex()[..32]
            ))
            .unwrap();
        client
    }

    fn cleanup(&self) {
        self.client()
            .batch_execute(&format!(
                "DROP SCHEMA mash_g_{} CASCADE",
                &blake3::hash(self.group.as_bytes()).to_hex()[..32]
            ))
            .unwrap();
    }
}

#[test]
fn local_destination_filters_before_scanning_and_never_publishes() {
    let fixture = Fixture::new();
    fixture.configure("local", "local");
    std::fs::remove_file(fixture.codex.join("session.jsonl")).unwrap();
    std::fs::remove_dir(&fixture.codex).unwrap();
    let response = fixture.json(&["index"], false);
    assert_eq!(response["indexed_conversations"], 1);
    assert!(response.get("shared").is_none());
    let connection = Connection::open(&fixture.database).unwrap();
    let provider: String = connection
        .query_row("SELECT provider FROM conversations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(provider, "claude-code");
    assert!(fixture.json(&["status"], false).get("shared").is_none());
}

#[test]
fn offline_shared_only_never_opens_or_creates_sqlite() {
    let fixture = Fixture::new();
    fixture.configure("shared", "remote");
    for command in [
        vec!["index"],
        vec!["status"],
        vec!["view", "id"],
        vec!["forget", "id"],
    ] {
        assert_eq!(error_kind(&fixture.run(&command, false)), "shared-index");
        assert!(!fixture.database.parent().unwrap().exists());
    }
    std::fs::create_dir(fixture.database.parent().unwrap()).unwrap();
    std::fs::write(&fixture.database, b"invalid SQLite sentinel").unwrap();
    assert_eq!(error_kind(&fixture.run(&["status"], false)), "shared-index");
    assert_eq!(
        std::fs::read(&fixture.database).unwrap(),
        b"invalid SQLite sentinel"
    );
}

#[test]
#[ignore = "requires MASH_TEST_PG_URL pointing to an expendable PostgreSQL database"]
fn postgres_publication_shared_only_uses_remote_checkpoints_and_tombstones_without_sqlite() {
    let fixture = Fixture::new();
    fixture.configure("shared", "remote");
    let first = fixture.json(&["index"], true);
    assert_eq!(first["indexed_conversations"], 1);
    assert_eq!(first["scanned_files"], 1);
    assert_eq!(
        fixture.json(&["index", "--provider", "claude-code"], true)["scanned_files"],
        0,
        "command filters cannot bypass the shared allowlist"
    );
    assert_eq!(
        fixture.json(&["index"], true)["checkpoint_skipped_sources"],
        1
    );
    let status = fixture.json(&["status"], true);
    assert_eq!(status["database_path"], Value::Null);
    assert_eq!(status["messages"], 1);
    let freshness = status["shared_index"].clone();
    assert_eq!(freshness["nodes"][0]["node_id"], "remote");
    assert!(freshness["nodes"][0]["last_updated_at"].is_string());
    let id: String = fixture
        .client()
        .query_one("SELECT id FROM messages", &[])
        .unwrap()
        .get(0);
    assert_eq!(
        fixture.json(&["view", &id], true)["messages"][0]["content"],
        "shared Codex output"
    );
    let mut content = std::fs::read_to_string(fixture.codex.join("session.jsonl")).unwrap();
    content.push_str("invalid JSON\n");
    std::fs::write(fixture.codex.join("session.jsonl"), content).unwrap();
    assert_eq!(error_kind(&fixture.run(&["index"], true)), "shared-index");
    assert_eq!(
        fixture.json(&["status"], true)["shared_index"],
        freshness,
        "rolled-back ingestion does not change freshness"
    );
    assert_eq!(
        fixture.json(&["status"], true)["messages"],
        1,
        "malformed ingestion rolled back"
    );
    Fixture::write_codex(&fixture.codex);
    assert_eq!(
        fixture.json(&["forget", "codex-session"], true)["forgotten"],
        true
    );
    assert_eq!(
        fixture.json(&["index", "--full"], true)["tombstoned_sources"],
        1
    );
    assert_eq!(fixture.json(&["status"], true)["messages"], 0);
    let source = fixture.codex.join("session.jsonl");
    let next = std::fs::read_to_string(&source)
        .unwrap()
        .replace("codex-session", "another-session");
    std::fs::write(&source, next).unwrap();
    assert_eq!(fixture.json(&["index"], true)["indexed_conversations"], 1);
    std::fs::remove_file(source).unwrap();
    assert_eq!(fixture.json(&["index"], true)["purged_conversations"], 1);
    assert!(!fixture.database.parent().unwrap().exists());
    fixture.cleanup();
}

#[test]
#[ignore = "requires MASH_TEST_PG_URL pointing to an expendable PostgreSQL database"]
fn postgres_publication_routes_providers_independently_and_retries_shared_forgets() {
    let fixture = Fixture::new();
    fixture.configure("both", "split");
    assert_eq!(
        fixture.json(&["index"], false)["shared"]["state"],
        "pending"
    );
    assert_eq!(
        fixture.json(&["index"], true)["shared"]["state"],
        "published"
    );
    assert_eq!(
        fixture.json(&["status"], true)["shared"]["state"],
        "published"
    );
    let connection = Connection::open(&fixture.database).unwrap();
    let providers: Vec<String> = connection
        .prepare("SELECT provider FROM conversations")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(providers, ["claude-code"]);
    let remote: Vec<String> = fixture
        .client()
        .query("SELECT provider FROM conversations", &[])
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    assert_eq!(remote, ["codex"]);
    let id: String = fixture
        .client()
        .query_one("SELECT id FROM messages", &[])
        .unwrap()
        .get(0);
    assert_eq!(
        fixture.json(&["forget", "codex-session"], false)["shared"]["state"],
        "pending"
    );
    assert_eq!(
        fixture.json(&["view", &id, "--shared", "split"], true)["messages"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        fixture.json(&["index"], true)["shared"]["state"],
        "published"
    );
    assert_eq!(
        fixture
            .client()
            .query_one("SELECT count(*) FROM conversations", &[])
            .unwrap()
            .get::<_, i64>(0),
        0
    );
    assert_eq!(
        fixture.json(&["index", "--full"], true)["shared"]["state"],
        "published"
    );
    assert_eq!(
        fixture
            .client()
            .query_one("SELECT count(*) FROM conversations", &[])
            .unwrap()
            .get::<_, i64>(0),
        0,
        "persistent tombstone prevents reupload"
    );
    assert_eq!(
        fixture.json(&["status"], false)["conversations"],
        1,
        "local provider remains"
    );
    fixture.cleanup();
}

#[test]
#[ignore = "requires MASH_TEST_PG_URL pointing to an expendable PostgreSQL database"]
fn postgres_publication_forget_before_initial_delivery_is_durable() {
    let fixture = Fixture::new();
    fixture.configure("both", "first-delivery");
    assert_eq!(
        fixture.json(&["index"], false)["shared"]["state"],
        "pending"
    );
    assert_eq!(
        fixture.json(&["forget", "codex-session"], false)["shared"]["state"],
        "pending"
    );
    assert_eq!(
        fixture.json(&["index"], true)["shared"]["state"],
        "published"
    );
    assert_eq!(
        fixture
            .client()
            .query_one("SELECT count(*) FROM conversations", &[])
            .unwrap()
            .get::<_, i64>(0),
        0,
        "forget before first upload is durable"
    );
    fixture.cleanup();
}
