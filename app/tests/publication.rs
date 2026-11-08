use super::*;

#[test]
#[ignore = "requires MASH_TEST_QDRANT_URL pointing to an expendable Qdrant instance"]
#[allow(clippy::too_many_lines)]
fn qdrant_publication_retries_no_change_index_and_offline_forget_through_cli() {
    let dir = tempfile::tempdir().expect("temp");
    let database = dir.path().join("local.sqlite3");
    seed_readiness_database(&database, Some(""));
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
    configure_qdrant(&config);
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
        let response = serde_json::from_slice::<Value>(&output.stdout).expect("JSON response");
        let incomplete_index = args.first() == Some(&"index")
            && (response["silver"]["completeness"] != "complete"
                || response["shared"]["state"] == "pending");
        assert!(
            output.status.code() == Some(if incomplete_index { 9 } else { 0 }),
            "response={response}; {}",
            String::from_utf8_lossy(&output.stderr)
        );
        response
    };
    let first = invoke(&["index"], false);
    assert_eq!(first["shared"]["state"], "pending");
    let retry = invoke(&["index"], true);
    assert_eq!(retry["changed_messages"], 0);
    assert_eq!(retry["shared"]["state"], "published");
    // Publication succeeds independently of legacy silver certification.
    assert_eq!(retry["silver"]["completeness"], "unverified");
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
    shared_server::cleanup_group(&group);
    cleanup_local_gold(&database, "cli-node");
}

#[test]
#[ignore = "requires MASH_TEST_QDRANT_URL pointing to an expendable Qdrant instance"]
fn qdrant_backfill_command_publishes_a_baseline_through_the_cli() {
    assert_cli_baseline_acknowledged(false);
}

#[test]
#[ignore = "requires MASH_TEST_QDRANT_URL pointing to an expendable Qdrant instance"]
fn qdrant_backfill_command_acknowledges_a_changed_destination() {
    assert_cli_baseline_acknowledged(true);
}

#[allow(
    clippy::too_many_lines,
    reason = "verify baseline receipts through CLI and shared status"
)]
fn assert_cli_baseline_acknowledged(previous_destination: bool) {
    let dir = tempfile::tempdir().expect("temp");
    let database = dir.path().join("local.sqlite3");
    seed_readiness_database(&database, Some(""));
    // Local journal revisions can be much larger than the server's newly
    // assigned baseline revisions. Never report the former as a receipt.
    open_silver(&database)
        .execute("UPDATE shared_publication SET revision=5000000", [])
        .expect("advance local journal");
    if previous_destination {
        open_silver(&database)
            .execute(
                "UPDATE shared_publication SET target='previous-destination', acknowledged=revision",
                [],
            )
            .expect("seed previous destination receipt");
    }
    let models = dir.path().join("models");
    create_valid_model_marker(&models);
    let config = dir.path().join("config.toml");
    let group = format!(
        "backfill-{}",
        dir.path().file_name().expect("name").to_string_lossy()
    );
    let server = shared_server::Server::new(dir.path(), &group, &["cli-node"]);
    std::fs::write(
        &config,
        toml::to_string(&toml::toml! {
            version = 1
            providers = {}
            shared = {
            group_id = (group.as_str()),node_id = "cli-node",api_url = (server.url.as_str()),token_env = "MASH_TEST_SHARED_CLIENT_TOKEN"
            }
        })
        .expect("TOML"),
    )
    .expect("config");
    configure_qdrant(&config);

    let output = Command::cargo_bin("mash")
        .expect("binary")
        .args([
            "--config",
            config.to_str().unwrap(),
            "--db",
            database.to_str().unwrap(),
            "--models-dir",
            models.to_str().unwrap(),
            "index",
            "--backfill",
            "cli-baseline",
        ])
        .env(
            "MASH_TEST_SHARED_CLIENT_TOKEN",
            shared_server::token("cli-node"),
        )
        .output()
        .expect("run");
    let response: Value = serde_json::from_slice(&output.stdout).expect("JSON response");
    //  reports exit 9 when local silver is unverified, like a normal
    // index pass; the shared publication itself must still have succeeded.
    assert!(
        matches!(output.status.code(), Some(0 | 9)),
        "status={:?} response={response} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(response["shared"]["state"], "published", "{response}");
    assert_eq!(response["backfill"]["id"], "cli-baseline");
    let revision = response["backfill"]["revision"].as_i64().expect("revision");
    assert!(revision >= 1, "{response}");

    // The server recorded a completed baseline covering every member. Stored
    // identities are qualified by destination: `[group][node]id`.
    let catalog = Connection::open(dir.path().join("silver/sync/catalog.sqlite3"))
        .expect("open transfer catalog");
    let (state, total): (String, i64) = catalog
        .query_row(
            "SELECT state, total_chats FROM backfills WHERE backfill_id LIKE '%]cli-baseline'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("baseline row");
    assert_eq!(state, "complete");
    assert!(total >= 1);
    let covered: i64 = catalog
        .query_row(
            "SELECT COUNT(*) FROM backfill_members WHERE backfill_id LIKE '%]cli-baseline' AND covered = 1",
            [],
            |row| row.get(0),
        )
        .expect("coverage");
    assert_eq!(covered, total);

    // The published baseline is visible through the shared status.
    let status = Command::cargo_bin("mash")
        .expect("binary")
        .args([
            "--config",
            config.to_str().unwrap(),
            "--db",
            database.to_str().unwrap(),
            "--models-dir",
            models.to_str().unwrap(),
            "status",
            "--shared",
        ])
        .env(
            "MASH_TEST_SHARED_CLIENT_TOKEN",
            shared_server::token("cli-node"),
        )
        .output()
        .expect("run status");
    assert!(status.status.success());
    let status: Value = serde_json::from_slice(&status.stdout).expect("status JSON");
    assert_eq!(status["shared"]["state"], "published", "{status}");
    assert_eq!(
        status["shared_index"]["nodes"][0]["revision"].as_i64(),
        Some(revision)
    );
    let mapping: Value = serde_json::from_slice(
        &std::fs::read(database.with_extension("sqlite3.publication-base.json"))
            .expect("publication mapping"),
    )
    .expect("mapping JSON");
    assert_eq!(mapping["server"].as_i64(), Some(revision));
    assert!(mapping["local"].as_i64().unwrap() >= 5_000_000);
    if previous_destination {
        assert_eq!(mapping["local"].as_i64(), Some(5_000_001));
    }
    assert!(
        revision < 5_000_000,
        "server assigns its own baseline revisions"
    );
    shared_server::cleanup_group(&group);
    cleanup_local_gold(&database, "cli-node");
}

#[test]
#[ignore = "requires MASH_TEST_QDRANT_URL pointing to an expendable Qdrant instance"]
fn released_server_no_longer_exposes_legacy_mutation_routes() {
    let dir = tempfile::tempdir().expect("temp");
    let group = format!(
        "cutover-{}",
        dir.path().file_name().expect("name").to_string_lossy()
    );
    let server = shared_server::Server::new(dir.path(), &group, &["cli-node"]);
    let token = shared_server::token("cli-node");
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let client = reqwest::Client::new();
        for route in ["index", "attachment-upload", "connector-batches"] {
            let response = client
                .post(format!("{}/v1/{route}", server.url))
                .bearer_auth(&token)
                .header("content-type", "application/x-ndjson")
                .body("{}")
                .send()
                .await
                .expect("request");
            assert_eq!(
                response.status(),
                reqwest::StatusCode::NOT_FOUND,
                "{route} must be gone from the released router"
            );
        }
        // The synchronization endpoint is still mounted; a plain GET is a rejected
        // upgrade, not an unknown route.
        let response = client
            .get(format!("{}/v1/sync", server.url))
            .bearer_auth(&token)
            .send()
            .await
            .expect("request");
        assert_ne!(response.status(), reqwest::StatusCode::NOT_FOUND);
    });
    drop(server);
    shared_server::cleanup_group(&group);
}

/// Start `mash-server` for one node, optionally aborting at a crash point.
fn spawn_crash_server(
    root: &Path,
    group: &str,
    crash: Option<&str>,
) -> (std::process::Child, String) {
    let path = root.join("server.toml");
    if !path.exists() {
        let silver = root.join("silver").to_str().unwrap().to_owned();
        let config = toml::toml! {
            version = 1
            listen = "127.0.0.1:0"
            qdrant_url_env = "MASH_TEST_QDRANT_URL"
            qdrant_grpc_url_env = "MASH_TEST_QDRANT_GRPC_URL"
            silver_dir = silver
            nodes = [{ group_id = (group), node_id = "cli-node", token_env = "MASH_TEST_TOKEN_0" }]
            views = { build = { embedding_text = "dialogue" } }
        };
        std::fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
    }
    let mut command = std::process::Command::new(assert_cmd::cargo::cargo_bin!("mash-server"));
    command
        .args(["--server-config", path.to_str().unwrap()])
        .stdin(std::process::Stdio::piped())
        .env("MASH_TEST_EXIT_WITH_STDIN", "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .env("MASH_TEST_TOKEN_0", shared_server::token("cli-node"));
    if let Some(point) = crash {
        command.env("MASH_TEST_CRASH_AT", point);
    }
    let mut child = command.spawn().expect("spawn server");
    let stderr = child.stderr.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    let log = root.join("server.log");
    std::thread::spawn(move || {
        use std::io::{BufRead as _, Write as _};
        let mut reader = std::io::BufReader::new(stderr);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let _ = sender.send(line);
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
        {
            for line in reader.lines().map_while(Result::ok) {
                let _ = writeln!(file, "{line}");
            }
        }
    });
    let line = receiver
        .recv_timeout(std::time::Duration::from_secs(45))
        .expect("server startup");
    let response: Value = serde_json::from_str(&line).expect("startup JSON");
    let url = format!(
        "http://{}",
        response["listening"].as_str().expect("listening address")
    );
    (child, url)
}

fn write_client_config(path: &Path, group: &str, api_url: &str) {
    std::fs::write(
        path,
        toml::to_string(&toml::toml! {
            version = 1
            providers = {}
            shared = {
                group_id = (group), node_id = "cli-node",
                api_url = (api_url), token_env = "MASH_TEST_SHARED_CLIENT_TOKEN"
            }
        })
        .unwrap(),
    )
    .unwrap();
    configure_qdrant(path);
}

#[test]
#[ignore = "requires MASH_TEST_QDRANT_URL pointing to an expendable Qdrant instance"]
fn a_server_crash_after_silver_commit_converges_after_restart() {
    let dir = tempfile::tempdir().expect("temp");
    let database = dir.path().join("local.sqlite3");
    seed_readiness_database(&database, Some(""));
    let models = dir.path().join("models");
    create_valid_model_marker(&models);
    let config = dir.path().join("config.toml");
    let group = format!(
        "crash-{}",
        dir.path().file_name().expect("name").to_string_lossy()
    );

    // First process exits abruptly after the durable silver commit, before the
    // receipt and certification.
    let (mut first, url) = spawn_crash_server(dir.path(), &group, Some("after_silver_commit"));
    write_client_config(&config, &group, &url);
    let failed = Command::cargo_bin("mash")
        .expect("binary")
        .args([
            "--config",
            config.to_str().unwrap(),
            "--db",
            database.to_str().unwrap(),
            "--models-dir",
            models.to_str().unwrap(),
            "index",
        ])
        .env(
            "MASH_TEST_SHARED_CLIENT_TOKEN",
            shared_server::token("cli-node"),
        )
        .output()
        .expect("run");
    assert!(
        !failed.status.success(),
        "the crashed server cannot acknowledge the publication"
    );
    let status = first.wait().expect("wait");
    assert!(!status.success(), "the process aborted mid-publication");

    // Restart on the same silver directory; startup recovery finishes the work.
    let (mut second, url) = spawn_crash_server(dir.path(), &group, None);
    write_client_config(&config, &group, &url);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let mut converged = false;
    let mut last = String::new();
    while std::time::Instant::now() < deadline {
        let output = Command::cargo_bin("mash")
            .expect("binary")
            .args([
                "--config",
                config.to_str().unwrap(),
                "--db",
                database.to_str().unwrap(),
                "--models-dir",
                models.to_str().unwrap(),
                "status",
                "--shared",
            ])
            .env(
                "MASH_TEST_SHARED_CLIENT_TOKEN",
                shared_server::token("cli-node"),
            )
            .output()
            .expect("run status");
        last = String::from_utf8_lossy(&output.stdout).into_owned();
        if output.status.success()
            && let Ok(value) = serde_json::from_slice::<Value>(&output.stdout)
            && value["shared_index"]["nodes"][0]["revision"]
                .as_i64()
                .is_some_and(|revision| revision >= 1)
        {
            converged = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    assert!(
        converged,
        "recovery after restart publishes the committed batch; last status: {last}; server log: {}",
        std::fs::read_to_string(dir.path().join("server.log")).unwrap_or_default()
    );
    let _ = second.kill();
    let _ = second.wait();
    shared_server::cleanup_group(&group);
    cleanup_local_gold(&database, "cli-node");
}

/// A large first publication picks the baseline strategy without a flag.
#[test]
#[ignore = "requires MASH_TEST_QDRANT_URL pointing to an expendable Qdrant instance"]
fn index_chooses_a_baseline_automatically_for_a_large_first_publish() {
    let dir = tempfile::tempdir().expect("temp");
    let database = dir.path().join("local.sqlite3");
    seed_readiness_database(&database, Some(""));
    let models = dir.path().join("models");
    create_valid_model_marker(&models);
    let config = dir.path().join("config.toml");
    let group = format!(
        "auto-{}",
        dir.path().file_name().expect("name").to_string_lossy()
    );
    let server = shared_server::Server::new(dir.path(), &group, &["cli-node"]);
    std::fs::write(
        &config,
        toml::to_string(&toml::toml! {
            version = 1
            providers = {}
            shared = {
            group_id = (group.as_str()),node_id = "cli-node",api_url = (server.url.as_str()),token_env = "MASH_TEST_SHARED_CLIENT_TOKEN"
            }
        })
        .expect("TOML"),
    )
    .expect("config");
    configure_qdrant(&config);

    let output = Command::cargo_bin("mash")
        .expect("binary")
        .args([
            "--config",
            config.to_str().unwrap(),
            "--db",
            database.to_str().unwrap(),
            "--models-dir",
            models.to_str().unwrap(),
            "index",
        ])
        // One chat is "large" for this test; debug builds honour it.
        .env("MASH_TEST_AUTO_BACKFILL_MIN", "1")
        .env(
            "MASH_TEST_SHARED_CLIENT_TOKEN",
            shared_server::token("cli-node"),
        )
        .output()
        .expect("run");
    let response: Value = serde_json::from_slice(&output.stdout).expect("JSON response");
    assert!(
        matches!(output.status.code(), Some(0 | 9)),
        "status={:?} response={response}",
        output.status.code()
    );
    assert_eq!(response["shared"]["state"], "published", "{response}");

    // The server recorded a completed baseline without being asked.
    let catalog = Connection::open(dir.path().join("silver/sync/catalog.sqlite3"))
        .expect("open transfer catalog");
    let (state, total): (String, i64) = catalog
        .query_row(
            "SELECT state, total_chats FROM backfills WHERE backfill_id LIKE '%]auto-%'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("auto baseline row");
    assert_eq!(state, "complete");
    assert!(total >= 1);
    shared_server::cleanup_group(&group);
    cleanup_local_gold(&database, "cli-node");
}

/// A crash after the server published but before the client acknowledged leaves
/// the local journal pending; the next run acknowledges without republishing.
#[test]
#[ignore = "requires MASH_TEST_QDRANT_URL pointing to an expendable Qdrant instance"]
fn a_crash_before_acknowledgement_recovers_without_republishing() {
    let dir = tempfile::tempdir().expect("temp");
    let database = dir.path().join("local.sqlite3");
    seed_readiness_database(&database, Some(""));
    let models = dir.path().join("models");
    create_valid_model_marker(&models);
    let config = dir.path().join("config.toml");
    let group = format!(
        "ack-{}",
        dir.path().file_name().expect("name").to_string_lossy()
    );
    let server = shared_server::Server::new(dir.path(), &group, &["cli-node"]);
    std::fs::write(
        &config,
        toml::to_string(&toml::toml! {
            version = 1
            providers = {}
            shared = {
            group_id = (group.as_str()),node_id = "cli-node",api_url = (server.url.as_str()),token_env = "MASH_TEST_SHARED_CLIENT_TOKEN"
            }
        })
        .expect("TOML"),
    )
    .expect("config");
    configure_qdrant(&config);
    let invoke = |crash: Option<&str>| {
        let mut command = Command::cargo_bin("mash").expect("binary");
        command
            .args([
                "--config",
                config.to_str().unwrap(),
                "--db",
                database.to_str().unwrap(),
                "--models-dir",
                models.to_str().unwrap(),
                "index",
            ])
            .env(
                "MASH_TEST_SHARED_CLIENT_TOKEN",
                shared_server::token("cli-node"),
            );
        if let Some(point) = crash {
            command.env("MASH_TEST_CRASH_AT", point);
        }
        let output = command.output().expect("run");
        let response: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
        (output.status.code(), response)
    };

    let (code, _) = invoke(Some("after_publish_before_ack"));
    assert_ne!(code, Some(0), "the process aborts before acknowledging");

    // The second run finds the server already at this revision and only
    // acknowledges; it must not create another revision.
    let (code, response) = invoke(None);
    assert!(
        matches!(code, Some(0 | 9)),
        "retry status={code:?} response={response}"
    );
    assert_eq!(response["shared"]["state"], "published", "{response}");
    let freshness = service_status(&config, &database, &models);
    assert_eq!(
        freshness["shared_index"]["nodes"][0]["revision"].as_i64(),
        Some(1),
        "{freshness}"
    );
    shared_server::cleanup_group(&group);
    cleanup_local_gold(&database, "cli-node");
}

fn service_status(config: &Path, database: &Path, models: &Path) -> Value {
    let output = Command::cargo_bin("mash")
        .expect("binary")
        .args([
            "--config",
            config.to_str().unwrap(),
            "--db",
            database.to_str().unwrap(),
            "--models-dir",
            models.to_str().unwrap(),
            "status",
            "--shared",
        ])
        .env(
            "MASH_TEST_SHARED_CLIENT_TOKEN",
            shared_server::token("cli-node"),
        )
        .output()
        .expect("run status");
    serde_json::from_slice(&output.stdout).expect("status JSON")
}
