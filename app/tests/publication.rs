use super::*;

#[test]
#[ignore = "requires MASH_TEST_QDRANT_URL pointing to an expendable Qdrant instance"]
#[allow(clippy::too_many_lines)]
fn qdrant_publication_retries_no_change_index_and_offline_forget_through_cli() {
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
}
