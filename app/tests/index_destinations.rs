use super::*;

struct Fixture {
    directory: tempfile::TempDir,
    config: std::path::PathBuf,
    database: std::path::PathBuf,
    models: std::path::PathBuf,
    codex: std::path::PathBuf,
    group: String,
    server: Option<shared_server::Server>,
    redis: bool,
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
            redis,
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
        if self.redis {
            let client =
                redis::Client::open(std::env::var("MASH_TEST_REDIS_URL").unwrap()).unwrap();
            let mut connection = client.get_connection().unwrap();
            let prefix = format!("mash:r:{}:", blake3::hash(self.group.as_bytes()).to_hex());
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
            return;
        }
        self.client()
            .batch_execute(&format!(
                "DROP SCHEMA mash_g_{} CASCADE",
                &blake3::hash(self.group.as_bytes()).to_hex()[..32]
            ))
            .unwrap();
    }
}

#[test]
#[ignore = "requires disposable PostgreSQL and explicitly installed MASH_TEST_MODELS_DIR"]
fn claude_copy_reconciliation_postgres_snapshot() {
    claude_copy_reconciliation(false, false);
}

#[test]
#[ignore = "requires disposable PostgreSQL and explicitly installed MASH_TEST_MODELS_DIR"]
fn claude_copy_reconciliation_postgres_routed() {
    claude_copy_reconciliation(false, true);
}

#[test]
#[ignore = "requires disposable Redis Search and explicitly installed MASH_TEST_MODELS_DIR"]
fn claude_copy_reconciliation_redis_snapshot() {
    claude_copy_reconciliation(true, false);
}

#[test]
#[ignore = "requires disposable Redis Search and explicitly installed MASH_TEST_MODELS_DIR"]
fn claude_copy_reconciliation_redis_routed() {
    claude_copy_reconciliation(true, true);
}

fn claude_copy_reconciliation(redis: bool, routed: bool) {
    let mut fixture = Fixture::with_backend(redis);
    fixture.models = std::env::var("MASH_TEST_MODELS_DIR").unwrap().into();
    let root = fixture.directory.path().join("claude");
    let a = root.join("session.jsonl");
    let b = root.join("copy.jsonl");
    let write = |path: &Path, branch: Option<&str>| {
        let mut rows = vec![
            serde_json::json!({"type":"user","sessionId":"claude-session","uuid":"common","parentUuid":null,"message":{"role":"user","content":"common sentinel"}}),
        ];
        if let Some(branch) = branch {
            rows.push(serde_json::json!({"type":"user","sessionId":"claude-session","uuid":branch,"parentUuid":"common","message":{"role":"user","content":format!("{branch} sentinel")}}));
        }
        std::fs::write(
            path,
            rows.into_iter()
                .map(|row| row.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
    };
    write(&a, None);
    write(&b, None);
    let index_config = if routed {
        toml::toml! {destination = "both" shared = {providers = ["claude-code"]}}
    } else {
        toml::toml! {destination = "both"}
    };
    std::fs::write(&fixture.config, toml::to_string(&toml::toml! {
                version = 1
                providers = {claude-code = {roots = [(root.to_str().unwrap())]}}
                index = (index_config)
                shared = {group_id = (fixture.group.as_str()),node_id = "split",api_url = (fixture.server.as_ref().unwrap().url.as_str()),token_env = "MASH_TEST_ROUTING_TOKEN"}
            }).unwrap()).unwrap();
    let check = |messages: u64| {
        let indexed = fixture.json(&["index", "--all-history"], true);
        assert_eq!(indexed["shared"]["state"], "published", "{indexed}");
        assert_eq!(indexed["indexed_messages"], messages);
        let status = fixture.json(&["status", "--shared"], true);
        let node = status["shared_index"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["node_id"] == "split")
            .unwrap();
        assert_eq!(node["messages"], messages);
        let connection = rusqlite::Connection::open(&fixture.database).unwrap();
        let rows = connection
            .prepare("SELECT id,content FROM messages ORDER BY ordinal")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        for (id, content) in rows {
            let remote = fixture.json(&["view", &id, "--shared", "split", "--context", "0"], true);
            assert_eq!(remote["messages"][0]["content"], content);
        }
    };
    check(1);
    check(1);
    write(&b, Some("right"));
    check(2);
    write(&a, Some("left"));
    check(3);
    write(&b, None);
    check(2);
    write(&b, Some("right"));
    check(3);
    std::fs::remove_file(&a).unwrap();
    check(2);
    check(2);
    write(&a, None);
    check(2);
    std::fs::remove_file(&b).unwrap();
    check(1);
    check(1);
    std::fs::write(&a, "").unwrap();
    check(0);
    write(&a, None);
    check(1);
    assert!(
        fixture.json(&["forget", "claude-session"], true)["forgotten"]
            .as_bool()
            .unwrap()
    );
    check(0);
    fixture.cleanup();
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
