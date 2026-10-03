use super::*;
mod rebuild;
mod recipe;

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
        let server = std::env::var("MASH_TEST_QDRANT_URL").ok().map(|_| {
            shared_server::Server::new(
                root,
                &group,
                &["remote", "split", "first-delivery", "local"],
            )
        });
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
        let body: Value = serde_json::from_slice(&output.stdout).unwrap();
        let pending_index =
            arguments.first() == Some(&"index") && body["shared"]["state"] == "pending";
        assert!(
            output.status.code() == Some(if pending_index { 9 } else { 0 }),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        body
    }

    fn remote_node(&self, node: &str) -> Value {
        self.json(&["status", "--shared"], true)["shared_index"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|value| value["node_id"] == node)
            .unwrap()
            .clone()
    }
    fn assert_claude_scan(&self, node: &str) {
        let state: Value = tokio::runtime::Runtime::new().unwrap().block_on(async {
            reqwest::Client::new()
                .get(format!("{}/v1/state", self.server.as_ref().unwrap().url))
                .bearer_auth(shared_server::token(node))
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .json()
                .await
                .unwrap()
        });
        assert_eq!(state["silver_scans"].as_array().unwrap().len(), 1);
        assert_eq!(state["silver_scans"][0]["provider"], "claude-code");
        assert_eq!(state["silver_scans"][0]["authoritative"], true);
        assert!(state["silver_scans"][0]["cutoff"].is_null());
    }

    fn cleanup(&self) {
        shared_server::cleanup_group(&self.group);
    }
}

#[test]
#[ignore = "requires disposable Qdrant and explicitly installed MASH_TEST_MODELS_DIR"]
fn claude_copy_reconciliation_qdrant_snapshot() {
    claude_copy_reconciliation(false);
}

#[test]
#[ignore = "requires disposable Qdrant and explicitly installed MASH_TEST_MODELS_DIR"]
fn claude_copy_reconciliation_qdrant_routed() {
    claude_copy_reconciliation(true);
}

fn claude_copy_reconciliation(routed: bool) {
    let mut fixture = Fixture::new();
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
        if routed {
            fixture.assert_claude_scan("split");
        }
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
#[ignore = "requires MASH_TEST_QDRANT_URL pointing to disposable Qdrant"]
fn qdrant_publication_routes_providers_independently_and_retries_shared_forgets() {
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
    assert_eq!(fixture.remote_node("split")["conversations"], 1);
    assert_eq!(fixture.remote_node("split")["messages"], 1);
    let id: String = connection
        .query_row(
            "SELECT id FROM messages WHERE conversation_id='codex-session'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let remote = fixture.json(&["view", &id, "--shared", "split", "--context", "0"], true);
    assert_eq!(remote["messages"][0]["content"], "shared Codex output");
    let threads = fixture.json(&["list-threads", "--shared"], true);
    assert_eq!(threads["threads"].as_array().unwrap().len(), 1);
    assert_eq!(threads["threads"][0]["provider"], "codex");
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
    assert_eq!(fixture.remote_node("split")["conversations"], 0);
    assert_eq!(
        fixture.json(&["index", "--full"], true)["shared"]["state"],
        "published"
    );
    assert_eq!(
        fixture.remote_node("split")["conversations"],
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
#[ignore = "requires MASH_TEST_QDRANT_URL pointing to disposable Qdrant"]
fn qdrant_publication_forget_before_initial_delivery_is_durable() {
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
        fixture.remote_node("first-delivery")["conversations"],
        0,
        "forget before first upload is durable"
    );
    fixture.cleanup();
}
