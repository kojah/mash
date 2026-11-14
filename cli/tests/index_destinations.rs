use super::*;
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
        Self::with_server(true)
    }

    fn with_server(start_server: bool) -> Self {
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
        let backend = if start_server {
            std::env::var("MASH_TEST_QDRANT_URL").ok()
        } else {
            None
        };
        let server = backend.map(|_| {
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
            index = {destination = (destination)}
            shared = {group_id = (self.group.as_str()),node_id = (node),api_url = (self.server.as_ref().map_or("http://127.0.0.1:9",|s|s.url.as_str())),token_env = "MASH_TEST_ROUTING_TOKEN"}
        }).unwrap()).unwrap();
        if self.server.is_some() {
            configure_qdrant(&self.config);
        }
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
        let body: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{arguments:?}: {error}; status={}; stderr={}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )
        });
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
    fn cleanup(&self) {
        shared_server::cleanup_group(&self.group);
        let config: toml::Value =
            toml::from_str(&std::fs::read_to_string(&self.config).unwrap()).unwrap();
        cleanup_local_gold(
            &self.database,
            config["shared"]["node_id"].as_str().unwrap(),
        );
    }
}

#[test]
#[ignore = "requires disposable Qdrant and explicitly installed MASH_TEST_MODELS_DIR"]
fn claude_copy_reconciliation_qdrant_snapshot() {
    claude_copy_reconciliation();
}

fn assert_ready_node(node: &Value, messages: u64, interpretation_version: &Value) {
    assert_eq!(node["messages"], messages);
    assert_eq!(node["artifacts"]["metadata"], "ready");
    assert_eq!(node["artifacts"]["lexical"], "ready");
    assert_eq!(node["artifacts"]["dense"], "ready");
    assert_eq!(node["source_silver"]["completeness"], "complete");
    // Shared silver carries the interpretation the local index certified.
    assert_eq!(
        &node["source_silver"]["interpretation_version"],
        interpretation_version
    );
    assert_eq!(node["source_silver"]["rejected_sources"], 0);
}

fn claude_copy_reconciliation() {
    let mut fixture = Fixture::new();
    fixture.models = std::env::var("MASH_TEST_MODELS_DIR").unwrap().into();
    let root = fixture.directory.path().join("claude");
    let a = root.join("session.jsonl");
    let b = root.join("copy.jsonl");
    std::fs::write(root.join("journal.jsonl"), "{\"type\":\"started\",\"agentId\":\"alpha\"}\n{\"type\":\"result\",\"agentId\":\"beta\"}\n").unwrap();
    let write = |path: &Path, branch: Option<&str>| {
        let mut rows = vec![
            serde_json::json!({"type":"user","sessionId":"claude-session","uuid":"common","parentUuid":null,"message":{"role":"user","content":"common sentinel"}}),
        ];
        if let Some(branch) = branch {
            rows.push(serde_json::json!({"type":"user","sessionId":"claude-session","uuid":branch,"parentUuid":"common","message":{"role":"user","content":format!("{branch} sentinel")}}));
            rows.extend([
                serde_json::json!({"type":"user","sessionId":"claude-session","uuid":"common","parentUuid":null,"message":{"role":"user","content":"common sentinel"}}),
                serde_json::json!({"type":"user","sessionId":"claude-session","uuid":branch,"parentUuid":null,"message":{"role":"user","content":format!("{branch} sentinel")}}),
                serde_json::json!({"type":"system","subtype":"stop_hook_summary","uuid":"hook","parentUuid":branch}),
                serde_json::json!({"type":"system","subtype":"compact_boundary","uuid":"boundary","parentUuid":null,"logicalParentUuid":"hook","compactMetadata":{"preservedSegment":{"tailUuid":"hook"}}}),
            ]);
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
    let index_config = toml::toml! {destination = "both"};
    std::fs::write(&fixture.config, toml::to_string(&toml::toml! {
                version = 1
                providers = {claude-code = {roots = [(root.to_str().unwrap())]}}
                index = (index_config)
                shared = {group_id = (fixture.group.as_str()),node_id = "split",api_url = (fixture.server.as_ref().unwrap().url.as_str()),token_env = "MASH_TEST_ROUTING_TOKEN"}
            }).unwrap()).unwrap();
    configure_qdrant(&fixture.config);
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
        let local = fixture.json(&["status"], false);
        assert_ready_node(node, messages, &local["silver"]["interpretation_version"]);
        let connection = super::support::open_silver(&fixture.database);
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
#[ignore = "requires disposable Qdrant"]
fn local_destination_never_publishes() {
    let fixture = Fixture::new();
    fixture.configure("local", "local");
    let response = fixture.json(&["index"], false);
    assert_eq!(response["indexed_chats"], 2);
    assert!(response.get("shared").is_none());
    assert!(fixture.json(&["status"], false).get("shared").is_none());
    fixture.cleanup();
}

#[test]
fn shared_only_configuration_is_rejected_before_opening_sqlite() {
    // Configuration validation must not need a running backend, even when the
    // wider test job supplies backend settings for unrelated integration tests.
    let fixture = Fixture::with_server(false);
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
    // Only the Claude thread remains; the forgotten Codex thread never arrives.
    assert_eq!(
        fixture.remote_node("first-delivery")["chats"],
        1,
        "forget before first upload is durable"
    );
    fixture.cleanup();
}
