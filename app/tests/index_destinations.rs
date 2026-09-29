use super::*;

struct Fixture {
    directory: tempfile::TempDir,
    config: std::path::PathBuf,
    database: std::path::PathBuf,
    models: std::path::PathBuf,
    codex: std::path::PathBuf,
    group: String,
    server: Option<shared_server::Server>,
}

impl Fixture {
    fn new() -> Self {
        Self::with_backend(false)
    }
    fn with_backend(redis: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let config = root.join("config.toml");
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
        let server = if redis {
            Some(shared_server::Server::redis(
                root,
                &group,
                &["remote", "split", "first-delivery", "local"],
            ))
        } else {
            std::env::var("MASH_TEST_PG_URL").ok().map(|_| {
                shared_server::Server::new(
                    root,
                    &group,
                    &["remote", "split", "first-delivery", "local"],
                )
            })
        };
        Self {
            directory,
            config,
            database,
            models,
            codex,
            group,
            server,
        }
    }

    fn write_codex(root: &Path) {
        std::fs::write(root.join("session.jsonl"), r#"{"type":"session_meta","payload":{"id":"codex-session"}}
{"type":"response_item","payload":{"type":"function_call_output","call_id":"codex-tool","output":"shared Codex output"}}
"#).unwrap();
    }

    fn configure(&self, destination: &str, node: &str) {
        std::fs::write(&self.config, toml::to_string(&toml::toml! {
            version = 1
            providers = {
            claude-code = {roots = [(self.directory.path().join("claude").to_str().unwrap().to_owned())]},
            codex = {roots = [(self.codex.to_str().unwrap())]}
            }
            index = {destination = (destination),local = {providers = (if destination == "local" {vec!["claude-code"]} else {vec!["claude-code", "codex"]})},shared = {providers = ["codex"]}}
            shared = {group_id = (self.group.as_str()),node_id = (node),api_url = (self.server.as_ref().map_or("http://127.0.0.1:9",|s|s.url.as_str())),token_env = "MASH_TEST_ROUTING_TOKEN"}
        }).unwrap()).unwrap();
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
                "MASH_TEST_ROUTING_TOKEN",
                shared_server::token(
                    toml::from_str::<toml::Value>(&std::fs::read_to_string(&self.config).unwrap())
                        .unwrap()["shared"]["node_id"]
                        .as_str()
                        .unwrap(),
                ),
            );
        } else {
            command.env_remove("MASH_TEST_ROUTING_TOKEN");
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
fn shared_only_configuration_is_rejected_before_opening_sqlite() {
    let fixture = Fixture::new();
    fixture.configure("shared", "remote");
    for command in [
        vec!["index"],
        vec!["status"],
        vec!["view", "id"],
        vec!["forget", "id"],
    ] {
        assert_eq!(error_kind(&fixture.run(&command, false)), "configuration");
        assert!(!fixture.database.parent().unwrap().exists());
    }
    std::fs::create_dir(fixture.database.parent().unwrap()).unwrap();
    std::fs::write(&fixture.database, b"invalid SQLite sentinel").unwrap();
    assert_eq!(
        error_kind(&fixture.run(&["status"], false)),
        "configuration"
    );
    assert_eq!(
        std::fs::read(&fixture.database).unwrap(),
        b"invalid SQLite sentinel"
    );
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
        .prepare("SELECT provider FROM conversations ORDER BY provider")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(providers, ["claude-code", "codex"]);
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

#[test]
#[ignore = "requires MASH_TEST_REDIS_URL pointing to disposable Redis Search"]
fn redis_publication_local_required_and_filtered_offline_forgets_through_cli() {
    let fixture = Fixture::with_backend(true);
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
        fixture.json(&["forget", "codex-session"], false)["shared"]["state"],
        "pending"
    );
    assert_eq!(
        fixture.json(&["index"], true)["shared"]["state"],
        "published"
    );
    assert_eq!(
        fixture.json(&["index", "--full"], true)["shared"]["state"],
        "published"
    );
    let status = fixture.json(&["status", "--shared"], true);
    assert_eq!(status["conversations"], 1);
    assert_eq!(
        status["shared_index"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["node_id"] == "split")
            .unwrap()["messages"],
        0
    );
    let client = redis::Client::open(std::env::var("MASH_TEST_REDIS_URL").unwrap()).unwrap();
    let mut connection = client.get_connection().unwrap();
    let prefix = format!(
        "mash:r:{}:",
        blake3::hash(fixture.group.as_bytes()).to_hex()
    );
    redis::cmd("FT.DROPINDEX")
        .arg(format!("{prefix}index"))
        .arg("DD")
        .query::<()>(&mut connection)
        .unwrap();
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg(format!("{prefix}*"))
        .query(&mut connection)
        .unwrap();
    if !keys.is_empty() {
        redis::cmd("DEL")
            .arg(keys)
            .query::<usize>(&mut connection)
            .unwrap();
    }
}
