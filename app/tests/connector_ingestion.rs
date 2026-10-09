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
        "schema_version": 1, "source_id": "github:installation",
        "batch_id": name, "expected_revision": revision,
        "changes": [
            {"op":"upsert-chat", "id":"repo:pr:42", "title":"Fix ingestion", "metadata":{"native_kind":"pull-request"}},
            {"op":"upsert-message", "id":"review:1", "chat_id":"repo:pr:42", "content":"Review with two participants", "author":{"id":"alice","kind":"human"}},
            {"op":"upsert-document", "id":"note", "content":"Independent document"}
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
    assert!(!receipt.gold_ready);
    let replayed = ingest(&database, &config, &input, &models);
    assert!(replayed.status.success());
    assert_eq!(replayed.stdout, accepted.stdout);
    assert!(!models.exists());
    let source = "github:installation".to_owned().try_into().unwrap();
    let document = "note".to_owned().try_into().unwrap();
    assert_eq!(
        mash::connectors::silver::Store::open(&database)
            .unwrap()
            .document(&source, &document)
            .unwrap()
            .unwrap()
            .content,
        "Independent document"
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
        vec!["--rebuild-local-gold"],
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
