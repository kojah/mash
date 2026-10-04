use super::*;

#[test]
#[ignore = "requires disposable Qdrant and real models"]
fn qdrant_shared_gold_cli_rebuild_uses_retained_silver_without_local_or_provider_writes() {
    let mut fixture = Fixture::new();
    fixture.models = std::env::var_os("MASH_TEST_MODELS_DIR").unwrap().into();
    std::fs::write(fixture.codex.join("session.jsonl"), concat!(
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"codex-session\",\"cwd\":\"/work/rebuild\"}}\n",
        "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"retain canonical silver and rebuild vector search\"}]}}\n"
    )).unwrap();
    fixture.configure("both", "split");
    let initial = fixture.json(&["index", "--all-history"], true);
    assert_eq!(initial["shared"]["state"], "published");
    let local = fixture.json(&["status"], true);
    let connection = Connection::open(&fixture.database).unwrap();
    snapshot_local(&connection);
    let shared = fixture.remote_node("split");
    std::fs::remove_dir_all(&fixture.codex).unwrap();
    std::fs::remove_dir_all(fixture.directory.path().join("claude")).unwrap();
    let output = fixture.run(&["index", "--rebuild-shared-gold"], true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rebuilt: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(rebuilt["rebuild_shared_gold"], true);
    assert_eq!(rebuilt["embeddings"], 1);
    assert_eq!(rebuilt["model_inferences"], 1);
    assert_eq!(rebuilt["silver"], shared["silver_revision"]);
    assert_eq!(fixture.json(&["status"], true), local);
    assert_eq!(fixture.remote_node("split"), shared);
    assert_local_unchanged(&connection);
    assert_mcp_rebuild(&fixture);
    assert_eq!(fixture.remote_node("split"), shared);
    assert_local_unchanged(&connection);
    assert!(!fixture.codex.exists());
    assert_shared_recipe_switch(&mut fixture, &connection, &shared);
    let phases: Vec<Value> = output
        .stderr
        .split(|&b| b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    assert!(phases.iter().any(|p| p["phase"] == "shared-silver-read"));
    assert!(phases.iter().any(|p| p["phase"] == "shared-embeddings"
        && p["stored_vectors"] == 1
        && p["eta_seconds"] == 0));
    assert_eq!(phases.last().unwrap()["phase"], "complete");
    for field in [
        "download_milliseconds",
        "embedding_milliseconds",
        "publication_milliseconds",
        "total_milliseconds",
    ] {
        assert!(rebuilt[field].is_u64(), "{field}");
    }
    eprintln!("shared source-free fixture rebuild timings: {rebuilt}");
    fixture.models = fixture.directory.path().join("missing-models");
    let failed = fixture.run(&["index", "--rebuild-shared-gold"], true);
    assert!(!failed.status.success());
    assert_eq!(fixture.remote_node("split"), shared);
    assert_local_unchanged(&connection);
    let original_database = fixture.database.clone();
    fixture.database = fixture.directory.path().join("missing-local.sqlite3");
    let failed = fixture.run(&["index", "--rebuild-shared-gold"], true);
    assert!(!failed.status.success());
    assert!(!fixture.database.exists());
    fixture.database = fixture.directory.path().join("empty-local.sqlite3");
    let empty = Connection::open(&fixture.database).unwrap();
    let failed = fixture.run(&["index", "--rebuild-shared-gold"], true);
    assert!(!failed.status.success());
    let version: i64 = empty
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 0, "shared rebuild migrated local storage");
    let tables: i64 = empty
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tables, 0, "shared rebuild initialized local storage");
    fixture.database = original_database;
}

fn assert_shared_recipe_switch(fixture: &mut Fixture, connection: &Connection, original: &Value) {
    let baseline_score = remote_score(fixture);
    let canonical = remote_silver(fixture);
    for recipe in ["title-and-dialogue", "dialogue"] {
        drop(fixture.server.take());
        fixture.server = Some(shared_server::Server::with_recipe(
            fixture.directory.path(),
            &fixture.group,
            &["remote", "split", "first-delivery", "local"],
            recipe,
        ));
        fixture.configure("both", "split");
        assert_eq!(fixture.remote_node("split")["gold_current"], false);
        let stale = remote_search(fixture);
        assert!(!stale.status().is_success());
        let rebuilt = fixture.json(&["index", "--rebuild-shared-gold"], true);
        eprintln!("shared recipe {recipe} fixture rebuild: {rebuilt}");
        assert_eq!(rebuilt["build"]["embedding_text"], recipe);
        let current = fixture.remote_node("split");
        assert_eq!(current["gold_current"], true);
        assert_eq!(current["gold_manifest"]["build"]["embedding_text"], recipe);
        for field in [
            "silver_revision",
            "revision",
            "last_updated_at",
            "last_synced_at",
            "silver_coverage",
            "messages",
            "conversations",
        ] {
            assert_eq!(
                current[field], original[field],
                "changed canonical evidence: {field}"
            );
        }
        assert_eq!(remote_silver(fixture), canonical);
        assert_local_unchanged(connection);
        let models = fixture.models.clone();
        fixture.models = fixture.directory.path().join("missing-models");
        assert!(
            !fixture
                .run(&["index", "--rebuild-shared-gold"], true)
                .status
                .success()
        );
        fixture.models = models;
        assert_eq!(fixture.remote_node("split"), current);
        assert_local_unchanged(connection);
        let score = remote_score(fixture);
        eprintln!(
            "shared recipe {recipe} vector score={score}; retained history bytes={}",
            serde_json::to_vec(&canonical).unwrap().len()
        );
        if recipe == "title-and-dialogue" {
            assert!(
                (score - baseline_score).abs() > 0.000_001,
                "recipe did not change real embeddings"
            );
        } else {
            assert!(
                (score - baseline_score).abs() < 0.000_001,
                "default rebuild did not restore embeddings"
            );
        }
        // Query overrides execute existing gold, despite absent provider files.
        fixture.json(
            &[
                "search",
                "retain canonical",
                "--no-worker",
                "--unit",
                "messages",
                "--limit",
                "1",
            ],
            true,
        );
        assert_eq!(fixture.remote_node("split"), current);
        assert_local_unchanged(connection);
    }
    assert_eq!(fixture.remote_node("split"), *original);
}

fn remote_search(fixture: &Fixture) -> reqwest::Response {
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(send_search(fixture))
}

async fn send_search(fixture: &Fixture) -> reqwest::Response {
    let mut vector = vec![0.0; 384];
    vector[0] = 127.0;
    reqwest::Client::new().post(format!("{}/v1/search", fixture.server.as_ref().unwrap().url))
            .bearer_auth(shared_server::token("split"))
            .json(&serde_json::json!({"query":"canonical", "vector":vector, "provider":null, "days":null}))
            .send().await.unwrap()
}

fn remote_score(fixture: &Fixture) -> f64 {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        send_search(fixture)
            .await
            .error_for_status()
            .unwrap()
            .json::<Value>()
            .await
            .unwrap()["semantic"][0]["semantic_score"]
            .as_f64()
            .unwrap()
    })
}

pub(super) fn remote_silver(fixture: &Fixture) -> Value {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let retained: Value = reqwest::Client::new()
            .get(format!(
                "{}/v1/silver",
                fixture.server.as_ref().unwrap().url
            ))
            .bearer_auth(shared_server::token("split"))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        retained["history"].clone()
    })
}

const TABLES: &[&str] = &[
    "conversations",
    "messages",
    "conversation_enrichment",
    "source_checkpoints",
    "tombstones",
    "silver_state",
    "silver_scans",
    "silver_rejections",
    "message_embeddings",
    "pending_embeddings",
    "gold_manifest",
    "derived_state",
    "shared_publication",
];

pub(super) fn snapshot_local(connection: &Connection) {
    for table in TABLES {
        connection
            .execute_batch(&format!(
                "CREATE TEMP TABLE expected_{table} AS SELECT * FROM {table}"
            ))
            .unwrap();
    }
}

pub(super) fn assert_local_unchanged(connection: &Connection) {
    for table in TABLES {
        for query in [
            format!("SELECT * FROM {table} EXCEPT SELECT * FROM expected_{table}"),
            format!("SELECT * FROM expected_{table} EXCEPT SELECT * FROM {table}"),
        ] {
            let count: i64 = connection
                .query_row(&format!("SELECT count(*) FROM ({query})"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0, "local {table} changed during shared gold rebuild");
        }
    }
}

struct Mcp(std::process::Child);
impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn assert_mcp_status(
    input: &mut std::process::ChildStdin,
    receiver: &std::sync::mpsc::Receiver<Value>,
    fixture: &Fixture,
) {
    use std::io::Write;
    writeln!(
        input,
        "{}",
        serde_json::json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{
        "name":"status","arguments":{"shared":true}}})
    )
    .unwrap();
    input.flush().unwrap();
    let status = receiver
        .recv_timeout(std::time::Duration::from_secs(60))
        .unwrap();
    assert_eq!(status["id"], 5);
    assert_ne!(status["result"]["isError"], true, "{status}");
    let nodes = status["result"]["structuredContent"]["shared_index"]["nodes"]
        .as_array()
        .unwrap();
    let node = nodes
        .iter()
        .find(|node| node["node_id"] == "split")
        .unwrap();
    assert_eq!(node["artifacts"]["metadata"], "ready");
    assert_eq!(node["artifacts"]["lexical"], "ready");
    assert_eq!(node["artifacts"]["dense"], "ready");
    let cli = fixture.json(&["status", "--shared"], true);
    let expected = cli["shared_index"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["node_id"] == "split")
        .unwrap();
    assert_eq!(node["source_silver"], expected["source_silver"]);
    assert_eq!(node["silver_coverage"], expected["silver_coverage"]);
    if node.get("source_silver").is_none() {
        let scopes = node["silver_coverage"].as_array().unwrap();
        assert_ne!(scopes.as_slice(), &[] as &[Value]);
        assert!(
            scopes
                .iter()
                .all(|scope| scope["completeness"] == "complete")
        );
    }
}

pub(super) fn assert_mcp_rebuild(fixture: &Fixture) {
    use std::io::{BufRead, Write};
    use std::process::Stdio;
    let mut child = Mcp(
        std::process::Command::new(assert_cmd::cargo::cargo_bin!("mash"))
            .args([
                "--config",
                fixture.config.to_str().unwrap(),
                "--db",
                fixture.database.to_str().unwrap(),
                "--models-dir",
                fixture.models.to_str().unwrap(),
                "mcp",
            ])
            .env("MASH_TEST_ROUTING_TOKEN", shared_server::token("split"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut input = child.0.stdin.take().unwrap();
    let output = child.0.stdout.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in std::io::BufReader::new(output).lines() {
            let event: Value = serde_json::from_str(&line.unwrap()).unwrap();
            if sender.send(event).is_err() {
                break;
            }
        }
    });
    let next = || {
        receiver
            .recv_timeout(std::time::Duration::from_secs(60))
            .unwrap()
    };
    writeln!(input, "{}", serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"shared-rebuild-contract","version":"1"}}})).unwrap();
    input.flush().unwrap();
    assert_eq!(next()["id"], 1);
    writeln!(
        input,
        "{}",
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"})
    )
    .unwrap();
    writeln!(input, "{}", serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
        "name":"index","arguments":{"rebuild_shared_gold":true},"_meta":{"progressToken":"shared-gold"}}})).unwrap();
    input.flush().unwrap();
    let mut phases = Vec::new();
    loop {
        let event = next();
        if event["id"] == 2 {
            assert_ne!(event["result"]["isError"], true, "{event}");
            let result = &event["result"]["structuredContent"];
            assert_eq!(result["rebuild_shared_gold"], true);
            assert_eq!(result["model_inferences"], 1);
            break;
        }
        assert_eq!(event["params"]["progressToken"], "shared-gold");
        phases.push(event["params"]["_meta"]["dev.jamesqo.mash/index-progress"]["phase"].clone());
    }
    assert!(phases.contains(&serde_json::json!("shared-silver-read")));
    assert!(phases.contains(&serde_json::json!("shared-embeddings")));
    assert!(phases.contains(&serde_json::json!("complete")));
    assert_mcp_status(&mut input, &receiver, fixture);
    for (id, arguments) in [
        (
            3,
            serde_json::json!({"rebuild_shared_gold":true,"provider":["codex"]}),
        ),
        (
            4,
            serde_json::json!({"rebuild_shared_gold":true,"rebuild_local_gold":true}),
        ),
    ] {
        writeln!(
            input,
            "{}",
            serde_json::json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{
            "name":"index","arguments":arguments}})
        )
        .unwrap();
        input.flush().unwrap();
        let error = next();
        assert_eq!(error["id"], id);
        assert_eq!(error["result"]["isError"], true);
        assert_eq!(
            error["result"]["structuredContent"]["error"]["kind"],
            "usage"
        );
    }
    drop(input);
    drop(child);
    reader.join().unwrap();
}
