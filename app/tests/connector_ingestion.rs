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
    assert_eq!(hit["author"]["kind"], "human");
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

#[test]
fn later_deliveries_are_searchable_at_once_and_never_take_search_down() {
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

    // A later batch, as a connector delivers while it keeps syncing.
    let later = serde_json::json!({
        "schema_version": 2, "source_id": "github:installation",
        "batch_id": "second", "expected_revision": 1,
        "changes": [
            {"op":"upsert-object", "id":"review:2", "role":"entry", "kind":"github:review-comment",
             "parent":"repo:pr:42", "content":"Freshly delivered quokka", "author":{"id":"bob","kind":"human"}}
        ]
    });
    std::fs::write(&input, serde_json::to_vec(&later).unwrap()).unwrap();
    let delivered = ingest(&database, &config, &input, &models);
    assert!(
        delivered.status.success(),
        "{}",
        String::from_utf8_lossy(&delivered.stderr)
    );
    let receipt: mash::connectors::Receipt = serde_json::from_slice(&delivered.stdout).unwrap();
    assert!(receipt.views_ready);

    // No `mash index` in between: the new record and the earlier ones are found.
    let fresh = run(
        &database,
        &config,
        &models,
        &["search", "quokka", "--unit", "messages", "--local"],
    );
    assert_eq!(
        fresh["results"][0]["id"],
        "connector#entry#github:installation#review:2"
    );
    let earlier = run(
        &database,
        &config,
        &models,
        &["search", "participants", "--unit", "messages", "--local"],
    );
    assert_eq!(
        earlier["results"][0]["id"],
        "connector#entry#github:installation#review:1"
    );
    assert!(!models.exists());
}

/// Each connector unit's anchor and digest, as views hold them.
fn connector_units(root: &Path) -> Vec<(String, Vec<u8>)> {
    let views = Connection::open(root.join("silver.views.sqlite3")).unwrap();
    let mut statement = views
        .prepare("SELECT anchor, digest FROM connector_units ORDER BY anchor")
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn scoped_refreshes_match_a_full_rebuild_through_renames_moves_and_nesting() {
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
    let source = "chat:team";
    let deliver = |name: &str, revision: u64, changes: Value| {
        let batch = serde_json::json!({
            "schema_version": 2, "source_id": source, "batch_id": name,
            "expected_revision": revision, "changes": changes
        });
        std::fs::write(&input, serde_json::to_vec(&batch).unwrap()).unwrap();
        let output = ingest(&database, &config, &input, &models);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let receipt: mash::connectors::Receipt = serde_json::from_slice(&output.stdout).unwrap();
        receipt.views_ready
    };
    let container = |id: &str, title: &str, parent: Option<&str>| {
        let mut object = serde_json::json!({"op":"upsert-object","id":id,"role":"container",
            "kind":"chat:channel","title":title});
        if let Some(parent) = parent {
            object["parent"] = parent.into();
        }
        object
    };
    let entry = |id: &str, parent: &str, text: &str, at: i64| {
        serde_json::json!({"op":"upsert-object","id":id,"role":"entry","kind":"chat:message",
            "parent":parent,"content":text,"created_at":at,"author":{"id":"a","kind":"human"}})
    };
    deliver(
        "1",
        0,
        serde_json::json!([
            container("general", "General", None),
            container("random", "Random", None),
            entry("m1", "general", "first message", 10),
            entry("m2", "general", "second message", 30)
        ]),
    );
    run(&database, &config, &models, &["index"]);
    let steps = [
        // A new, older message shifts the positions of later ones.
        serde_json::json!([
            container("general", "General", None),
            entry("m0", "general", "earlier message", 5)
        ]),
        // Renaming a channel changes every message's ancestry in it.
        serde_json::json!([container("general", "General chat", None)]),
        // A thread moves to another channel with its reply.
        serde_json::json!([
            entry("m3", "m1", "a reply", 40),
            entry("m1", "random", "first message", 10)
        ]),
        // A nested channel under one that is then renamed.
        serde_json::json!([
            container("nested", "Nested", Some("random")),
            entry("n1", "nested", "nested message", 50)
        ]),
        serde_json::json!([container("random", "Random things", None)]),
        // Re-sending an unchanged channel changes nothing.
        serde_json::json!([container("general", "General chat", None)]),
    ];
    for (revision, changes) in steps.into_iter().enumerate() {
        let revision = u64::try_from(revision).unwrap() + 1;
        assert!(deliver(&format!("step-{revision}"), revision, changes));
        let scoped = connector_units(root);
        run(
            &database,
            &config,
            &models,
            &["index", "--rebuild-local-views", "--full"],
        );
        assert_eq!(scoped, connector_units(root), "after step {revision}");
    }
}
