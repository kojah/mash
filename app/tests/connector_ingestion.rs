use super::*;

fn ingest(database: &Path, config: &Path, input: &Path, models: &Path) -> std::process::Output {
    Command::cargo_bin("mash")
        .unwrap()
        .arg("--config")
        .arg(config)
        .arg("--db")
        .arg(database)
        .arg("--models-dir")
        .arg(models)
        .arg("index")
        .arg("--connector-batch")
        .arg(input)
        .output()
        .unwrap()
}

fn payload(name: &str, revision: u64) -> Value {
    serde_json::json!({
        "schema_version": 2, "source_id": "github:installation",
        "batch_id": name, "expected_revision": revision,
        "changes": [
            {"op":"upsert-object", "id":"repo:pr:42", "role":"container", "kind":"github:pull-request", "title":"Fix ingestion"},
            {"op":"upsert-object", "id":"review:1", "role":"entry", "kind":"github:review-comment", "parent":"repo:pr:42", "content":"Review with two participants", "author":{"id":"alice","kind":"human"}},
            {"op":"upsert-object", "id":"note", "role":"document", "kind":"obsidian:note", "content":"Independent document"}
        ]
    })
}

#[test]
fn connector_cli_commits_silver_and_replays_receipts_without_model_work() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let config = root.join("config.toml");
    std::fs::write(
        &config,
        "version=1\n[providers]\n[index]\ndestination='local'\n",
    )
    .unwrap();
    let database = root.join("silver.sqlite3");
    let models = root.join("models");
    let input = root.join("batch.json");
    std::fs::write(&input, serde_json::to_vec(&payload("first", 0)).unwrap()).unwrap();
    let accepted = ingest(&database, &config, &input, &models);
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    let receipt: mash::connectors::Receipt = serde_json::from_slice(&accepted.stdout).unwrap();
    assert_eq!(receipt.source_revision, 1);
    assert!(!receipt.views_ready);
    let replayed = ingest(&database, &config, &input, &models);
    assert!(replayed.status.success());
    assert_eq!(replayed.stdout, accepted.stdout);
    assert!(!models.exists());
    let source = "github:installation".to_owned().try_into().unwrap();
    let document = "note".to_owned().try_into().unwrap();
    assert_eq!(
        mash::connectors::silver::Store::open(&database)
            .unwrap()
            .object(&source, &document)
            .unwrap()
            .unwrap()
            .content
            .as_deref(),
        Some("Independent document")
    );

    std::fs::write(&input, serde_json::to_vec(&payload("stale", 0)).unwrap()).unwrap();
    let conflict = ingest(&database, &config, &input, &models);
    assert_eq!(conflict.status.code(), Some(12));
    assert_eq!(error_kind(&conflict), "conflict");
    assert_eq!(conflict.stdout, Vec::<u8>::new());
    assert!(!String::from_utf8_lossy(&conflict.stderr).contains("Review with two"));
}

#[test]
fn malformed_and_oversized_connector_inputs_never_create_storage() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let config = root.join("config.toml");
    std::fs::write(&config, "version=1\n[providers]\n").unwrap();
    let input = root.join("invalid.json");
    let database = root.join("missing.sqlite3");
    let models = root.join("models");
    std::fs::write(&input, b"{private input that must not be echoed").unwrap();
    let result = ingest(&database, &config, &input, &models);
    assert_eq!(error_kind(&result), "usage");
    assert!(!String::from_utf8_lossy(&result.stderr).contains("private input"));
    assert!(!database.exists());
    std::fs::File::create(&input)
        .unwrap()
        .set_len(mash::connectors::MAX_BATCH_BYTES as u64 + 1)
        .unwrap();
    assert_eq!(
        error_kind(&ingest(&database, &config, &input, &models)),
        "usage"
    );
    assert_eq!(
        error_kind(&ingest(&database, &config, root, &models)),
        "usage"
    );
    assert!(!database.exists());
    assert!(!models.exists());
}

#[test]
fn connector_batches_conflict_with_provider_and_rebuild_options() {
    for options in [
        vec!["--watch"],
        vec!["--full"],
        vec!["--rebuild-local-views"],
        vec!["--provider", "codex"],
        vec!["--since-days", "90"],
        vec!["--all-history"],
    ] {
        let output = Command::cargo_bin("mash")
            .unwrap()
            .args(["index", "--connector-batch", "unused.json"])
            .args(options)
            .output()
            .unwrap();
        assert_eq!(error_kind(&output), "usage");
        assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be used"));
    }
}

fn run(database: &Path, config: &Path, models: &Path, arguments: &[&str]) -> Value {
    let output = Command::cargo_bin("mash")
        .unwrap()
        .arg("--config")
        .arg(config)
        .arg("--db")
        .arg(database)
        .arg("--models-dir")
        .arg(models)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn connector_records_become_searchable_viewable_and_listable_after_indexing() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let config = root.join("config.toml");
    std::fs::write(
        &config,
        "version=1\n[providers]\n[index]\ndestination='local'\n\
         [views.build]\nrepresentations='lexical-only'\n[views.query]\nintent='exact-mention'\n",
    )
    .unwrap();
    let database = root.join("silver.sqlite3");
    let models = root.join("models");
    let input = root.join("batch.json");
    std::fs::write(&input, serde_json::to_vec(&payload("first", 0)).unwrap()).unwrap();
    assert!(ingest(&database, &config, &input, &models).status.success());
    run(&database, &config, &models, &["index"]);
    let found = run(
        &database,
        &config,
        &models,
        &["search", "participants", "--unit", "messages", "--local"],
    );
    let hit = &found["results"][0];
    let anchor = "connector#entry#github:installation#review:1";
    assert_eq!(hit["id"], anchor);
    assert_eq!(hit["source"]["connector"], "github:installation");
    assert_eq!(hit["object"]["role"], "entry");
    assert_eq!(hit["object"]["kind"], "github:review-comment");
    assert_eq!(hit["ancestors"][0]["kind"], "github:pull-request");
    assert_eq!(hit["author_kind"], "human");
    assert!(hit.get("role").is_none());
    let documents = run(
        &database,
        &config,
        &models,
        &[
            "search",
            "independent",
            "--unit",
            "chats",
            "--local",
            "--source",
            "github:installation",
        ],
    );
    assert_eq!(documents["results"][0]["object"]["role"], "document");
    let native_only = run(
        &database,
        &config,
        &models,
        &["search", "participants", "--local", "--provider", "codex"],
    );
    assert_eq!(native_only["results"], serde_json::json!([]));
    let viewed = run(&database, &config, &models, &["view", anchor]);
    assert_eq!(
        viewed["messages"][0]["content"],
        "Review with two participants"
    );
    let chats = run(&database, &config, &models, &["list-chats", "--local"]);
    assert_eq!(
        chats["chats"][0]["chat_id"],
        "connector#container#github:installation#repo:pr:42"
    );
    assert_eq!(
        chats["chats"][0]["source"]["connector"],
        "github:installation"
    );
    assert!(!models.exists());
}
