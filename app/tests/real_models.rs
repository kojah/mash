use super::*;

#[test]
#[ignore = "requires MASH_TEST_MODELS_DIR containing an explicit model installation"]
fn real_models_index_and_run_hybrid_search() {
    let models = std::env::var_os("MASH_TEST_MODELS_DIR")
        .map(std::path::PathBuf::from)
        .expect("explicit model directory");
    let directory = tempfile::tempdir().expect("temporary directory");
    let claude = directory.path().join("claude");
    let codex = directory.path().join("codex");
    std::fs::create_dir_all(&claude).expect("Claude root");
    std::fs::create_dir_all(&codex).expect("Codex root");
    std::fs::write(
        claude.join("session.jsonl"),
        concat!(
            "{\"type\":\"user\",\"sessionId\":\"hybrid\",\"uuid\":\"m1\",",
            "\"message\":{\"role\":\"user\",\"content\":\"repair authentication credentials\"}}\n",
            "{\"type\":\"assistant\",\"sessionId\":\"hybrid\",\"uuid\":\"m2\",",
            "\"message\":{\"role\":\"assistant\",\"content\":[",
            "{\"type\":\"text\",\"text\":\"refresh the login token\"},",
            "{\"type\":\"tool_result\",\"content\":\"private-output-marker\"}]}}\n"
        ),
    )
    .expect("Claude fixture");
    let database = directory.path().join("mash.sqlite3");
    let config = directory.path().join("config.toml");
    write_config(&config, &claude, &codex);
    let model_arg = models.to_str().unwrap();
    let output = Command::cargo_bin("mash")
        .expect("mash binary")
        .args([
            "--config",
            config.to_str().unwrap(),
            "--db",
            database.to_str().unwrap(),
            "--models-dir",
            model_arg,
            "index",
            "--full",
        ])
        .output()
        .expect("index");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let indexed: Value = serde_json::from_slice(&output.stdout).expect("index JSON");
    assert_eq!(indexed["realized_mode"], "hybrid");
    assert_eq!(indexed["indexed_messages"], 2);
    assert_eq!(indexed["searchable_messages"], 2);
    let progress = output
        .stderr
        .split(|&byte| byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<Value>(line).expect("index progress JSON"))
        .collect::<Vec<_>>();
    assert_eq!(progress.last().unwrap()["phase"], "complete");
    assert!(
        progress
            .iter()
            .any(|event| event["phase"] == "semantic-embeddings"
                && event["eta_seconds"] == 0
                && event["stored_vectors"] == 2)
    );
    for timing in [
        "model_load_milliseconds",
        "storage_setup_milliseconds",
        "ingestion_milliseconds",
        "search_refresh_milliseconds",
        "embedding_milliseconds",
        "total_milliseconds",
    ] {
        assert!(indexed[timing].is_u64(), "missing index timing: {timing}");
    }

    assert_scoped_message_search(&config, &database, model_arg);
    assert_thread_search_and_view(&config, &database, model_arg);
    #[cfg(unix)]
    assert_worker_stage_diagnostics(&config, &database, model_arg);
    assert_gold_query_defaults(&config, &database, model_arg);
    assert_source_independent_gold_rebuild(&config, &database, &claude, &codex, model_arg);
}

fn assert_source_independent_gold_rebuild(
    config: &Path,
    database: &Path,
    claude: &Path,
    codex: &Path,
    models: &str,
) {
    let connection = Connection::open(database).unwrap();
    let silver_tables = [
        "conversations",
        "messages",
        "tombstones",
        "claude_sources",
        "claude_source_messages",
        "source_checkpoints",
        "silver_state",
        "silver_scans",
        "silver_rejections",
    ];
    for table in silver_tables {
        connection
            .execute_batch(&format!(
                "CREATE TEMP TABLE expected_{table} AS SELECT * FROM {table}"
            ))
            .unwrap();
    }
    std::fs::remove_dir_all(claude).unwrap();
    std::fs::remove_dir_all(codex).unwrap();
    let original = std::fs::read_to_string(config).unwrap();
    std::fs::write(config, format!("version=1\nproviders={{claude-code={{roots=[{:?}]}},codex={{roots=[{:?}]}}}}\n[index]\ndestination='both'\n[shared]\ngroup_id='group'\nnode_id='node'\napi_url='http://127.0.0.1:9'\ntoken_env='MASH_GOLD_REBUILD_NO_TOKEN'\n",claude.to_str().unwrap(),codex.to_str().unwrap())).unwrap();
    for full in [false, true] {
        let mut command = Command::cargo_bin("mash").unwrap();
        command.args([
            "--config",
            config.to_str().unwrap(),
            "--db",
            database.to_str().unwrap(),
            "--models-dir",
            models,
            "index",
            "--rebuild-local-gold",
        ]);
        if full {
            command.arg("--full");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let indexed: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(indexed["rebuild_local_gold"], true);
        assert_eq!(indexed["realized_mode"], "hybrid");
        assert_eq!(indexed["scanned_files"], 0);
        assert_eq!(indexed["ingestion_milliseconds"], 0);
        assert_eq!(indexed["model_inferences"], if full { 2 } else { 0 });
        assert!(indexed["shared"].is_null());
        for table in silver_tables {
            let unchanged = connection.query_row(&format!(
                "SELECT NOT EXISTS(SELECT * FROM {table} EXCEPT SELECT * FROM expected_{table}) AND NOT EXISTS(SELECT * FROM expected_{table} EXCEPT SELECT * FROM {table})"), [], |row| row.get::<_, bool>(0)).unwrap();
            assert!(unchanged, "gold changed silver table {table}");
        }
    }
    assert_gold_recipe_switches(config, database, models);
    assert_gold_rebuild_recovers_without_sources(config, database, models);
    for table in silver_tables {
        let unchanged = connection.query_row(&format!(
            "SELECT NOT EXISTS(SELECT * FROM {table} EXCEPT SELECT * FROM expected_{table}) AND NOT EXISTS(SELECT * FROM expected_{table} EXCEPT SELECT * FROM {table})"), [], |row| row.get::<_, bool>(0)).unwrap();
        assert!(unchanged, "recovery changed silver table {table}");
    }
    std::fs::write(config, original).unwrap();
    assert_scoped_message_search(config, database, models);
}

fn assert_gold_recipe_switches(config: &Path, database: &Path, models: &str) {
    let build_config = std::fs::read_to_string(config).unwrap();
    for recipe in ["title-and-dialogue", "dialogue"] {
        std::fs::write(
            config,
            format!("{build_config}\n[gold.build]\nembedding_text = '{recipe}'\n"),
        )
        .unwrap();
        let stale = Command::cargo_bin("mash")
            .unwrap()
            .args([
                "--config",
                config.to_str().unwrap(),
                "--db",
                database.to_str().unwrap(),
                "--models-dir",
                models,
                "search",
                "qdrant",
                "--no-worker",
            ])
            .output()
            .unwrap();
        assert!(
            !stale.status.success(),
            "changed recipe must require explicit rebuild"
        );
        let rebuilt = Command::cargo_bin("mash")
            .unwrap()
            .args([
                "--config",
                config.to_str().unwrap(),
                "--db",
                database.to_str().unwrap(),
                "--models-dir",
                models,
                "index",
                "--rebuild-local-gold",
            ])
            .output()
            .unwrap();
        assert!(
            rebuilt.status.success(),
            "{}",
            String::from_utf8_lossy(&rebuilt.stderr)
        );
        let indexed: Value = serde_json::from_slice(&rebuilt.stdout).unwrap();
        assert_eq!(indexed["gold"]["build"]["embedding_text"], recipe);
        assert_eq!(indexed["model_inferences"], 2);
        assert_eq!(indexed["scanned_files"], 0);
        assert!(indexed["shared"].is_null());
        if recipe == "title-and-dialogue" {
            assert_gold_rebuild_recovers_without_sources(config, database, models);
        }
    }
    std::fs::write(config, &build_config).unwrap();
    let connection = Connection::open(database).unwrap();
    connection
        .execute_batch("DROP TABLE gold_manifest; PRAGMA user_version=36;")
        .unwrap();
    let migrated = run_json(&[
        "--config",
        config.to_str().unwrap(),
        "--db",
        database.to_str().unwrap(),
        "--models-dir",
        models,
        "index",
        "--rebuild-local-gold",
    ]);
    assert_eq!(migrated["gold"]["schema_version"], 1);
    assert_eq!(migrated["gold"]["build"]["embedding_text"], "dialogue");
    assert_eq!(migrated["model_inferences"], 0);
    assert_eq!(migrated["scanned_files"], 0);
}

fn assert_gold_rebuild_recovers_without_sources(config: &Path, database: &Path, models: &str) {
    let broken = database.with_file_name("broken-models");
    create_valid_model_marker(&broken);
    let failed = Command::cargo_bin("mash")
        .unwrap()
        .args([
            "--config",
            config.to_str().unwrap(),
            "--db",
            database.to_str().unwrap(),
            "--models-dir",
            broken.to_str().unwrap(),
            "index",
            "--rebuild-local-gold",
            "--full",
        ])
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert_eq!(failed.stdout, [] as [u8; 0]);
    let connection = Connection::open(database).unwrap();
    let ready: Option<String> = connection
        .query_row(
            "SELECT semantic_ready_generation FROM derived_state WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(ready.is_none(), "failed replacement must remain unready");
    let recovered = run_json(&[
        "--config",
        config.to_str().unwrap(),
        "--db",
        database.to_str().unwrap(),
        "--models-dir",
        models,
        "index",
        "--rebuild-local-gold",
    ]);
    assert_eq!(recovered["realized_mode"], "hybrid");
    assert_eq!(recovered["model_inferences"], 2);
    assert_eq!(recovered["scanned_files"], 0);
    assert!(recovered["shared"].is_null());
}

fn assert_gold_query_defaults(config: &Path, database: &Path, models: &str) {
    let canonical_config = std::fs::read_to_string(config).unwrap();
    let before = std::fs::read(database).unwrap();
    for (unit, limit) in [("messages", 1), ("threads", 3)] {
        std::fs::write(
            config,
            format!("{canonical_config}\n[gold.query]\nunit = '{unit}'\nlimit = {limit}\n"),
        )
        .unwrap();
        for direct in [true, false] {
            let mut command = Command::cargo_bin("mash").unwrap();
            command.args([
                "--config",
                config.to_str().unwrap(),
                "--db",
                database.to_str().unwrap(),
                "--models-dir",
                models,
                "search",
                "--local",
                "authentication",
            ]);
            if direct {
                command.arg("--no-worker");
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let found: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(found["unit"], unit);
            let results = found["results"].as_array().unwrap();
            assert_ne!(results.as_slice(), &[] as &[Value]);
            assert!(results.len() <= limit);
        }
    }
    assert_eq!(std::fs::read(database).unwrap(), before);
    std::fs::write(config, canonical_config).unwrap();
}

#[cfg(unix)]
fn assert_worker_stage_diagnostics(config: &Path, database: &Path, models: &str) {
    let mut previous = None;
    let mut worker = None;
    for traced in [true, true, false] {
        let mut command = Command::cargo_bin("mash").unwrap();
        if traced {
            command.env("MASH_TRACE", "1");
        } else {
            command.env_remove("MASH_TRACE");
        }
        let output = command
            .args([
                "--config",
                config.to_str().unwrap(),
                "--db",
                database.to_str().unwrap(),
                "--models-dir",
                models,
                "search",
                "--local",
                "--unit",
                "messages",
                "authentication",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let found: Value = serde_json::from_slice(&output.stdout).unwrap();
        if let Some(previous) = &previous {
            assert_eq!(&found, previous);
        }
        previous = Some(found);
        let stderr = String::from_utf8(output.stderr).unwrap();
        if !traced {
            assert_eq!(stderr, "");
            continue;
        }
        for sensitive in [
            "authentication",
            "private-output-marker",
            config.to_str().unwrap(),
            database.to_str().unwrap(),
            models,
        ] {
            assert!(
                !stderr.contains(sensitive),
                "private data in worker diagnostics"
            );
        }
        let events: Vec<Value> = stderr
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let reply = events
            .iter()
            .find(|event| event["fields"]["message"] == "worker reply")
            .unwrap();
        let pid = reply["fields"]["worker_process_id"].as_u64().unwrap();
        if let Some(worker) = worker {
            assert_eq!(pid, worker, "same cached worker");
        }
        worker = Some(pid);
        let stages: Vec<_> = events
            .iter()
            .filter(|event| event["fields"]["message"] == "worker stage")
            .collect();
        assert!(stages.len() <= 15);
        for name in [
            "search",
            "local_search",
            "load_search_models",
            "embed",
            "hybrid_search",
            "lexical_candidates",
            "load_vectors",
            "score_vectors",
            "rerank_inference",
        ] {
            let stage = stages
                .iter()
                .find(|event| event["fields"]["stage"] == name)
                .unwrap_or_else(|| panic!("missing worker stage {name}"));
            assert!(stage["fields"]["calls"].as_u64().unwrap() > 0);
            assert!(
                stage["fields"]["total_us"].as_u64().unwrap()
                    >= stage["fields"]["max_us"].as_u64().unwrap()
            );
            assert_eq!(stage["fields"]["trace_id"], reply["fields"]["trace_id"]);
        }
    }
}

fn assert_scoped_message_search(config: &Path, database: &Path, model_arg: &str) {
    let thread = listed_thread(config, database);
    let found = run_json(&[
        "--config",
        config.to_str().unwrap(),
        "--db",
        database.to_str().unwrap(),
        "--models-dir",
        model_arg,
        "search",
        "--unit",
        "messages",
        "--provider",
        "claude-code",
        "--conversation",
        &thread,
        "authentication",
    ]);
    assert_eq!(found["realized_mode"], "hybrid");
    assert!(found.get("fallback_mode").is_none());
    assert!(found.get("fallback_reason").is_none());
    assert!(found["results"][0]["lexical_score"].is_number());
    assert!(found["results"][0]["semantic_score"].is_number());
    assert!(found["results"][0]["rerank_score"].is_number());
    assert!(found["results"].as_array().is_some_and(|results| {
        results.iter().any(|result| {
            result["content"]
                .as_str()
                .is_some_and(|content| content.contains("authentication credentials"))
        })
    }));
}

fn listed_thread(config: &Path, database: &Path) -> String {
    let catalog = run_json(&[
        "--config",
        config.to_str().unwrap(),
        "--db",
        database.to_str().unwrap(),
        "list-threads",
        "--local",
        "--provider",
        "claude-code",
    ]);
    let thread = catalog["threads"][0]["conversation_id"].as_str().unwrap();
    assert_eq!(thread, "hybrid");
    thread.to_owned()
}

fn assert_thread_search_and_view(config: &Path, database: &Path, model_arg: &str) {
    let threads = run_json(&[
        "--config",
        config.to_str().unwrap(),
        "--db",
        database.to_str().unwrap(),
        "--models-dir",
        model_arg,
        "search",
        "authentication",
        "--limit",
        "1",
    ]);
    assert_eq!(threads["unit"], "threads");
    let results = threads["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["conversation_id"], "hybrid");
    let matches = results[0]["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 2);
    assert!(
        matches[0]["rerank_score"].as_f64().unwrap()
            >= matches[1]["rerank_score"].as_f64().unwrap()
    );
    let anchor = matches[0]["message_id"].as_str().unwrap();
    let context = run_json(&[
        "--config",
        config.to_str().unwrap(),
        "--db",
        database.to_str().unwrap(),
        "view",
        anchor,
        "--context",
        "1",
    ]);
    assert_eq!(context["messages"].as_array().unwrap().len(), 2);
}

#[test]
#[ignore = "requires explicit MASH_BENCHMARK_DB, MASH_BENCHMARK_CONFIG, and MASH_TEST_MODELS_DIR"]
fn benchmark_real_corpus_semantic_generation() {
    let source = std::env::var_os("MASH_BENCHMARK_DB")
        .map(std::path::PathBuf::from)
        .expect("explicit benchmark source database");
    let config = std::env::var_os("MASH_BENCHMARK_CONFIG")
        .map(std::path::PathBuf::from)
        .expect("explicit benchmark configuration");
    let models = std::env::var_os("MASH_TEST_MODELS_DIR")
        .map(std::path::PathBuf::from)
        .expect("explicit model directory");
    let directory = tempfile::tempdir().expect("temporary benchmark directory");
    let database = directory.path().join("benchmark.sqlite3");
    let source_connection =
        Connection::open_with_flags(&source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("open benchmark source");
    source_connection
        .execute("VACUUM INTO ?1", [database.to_string_lossy().as_ref()])
        .expect("snapshot benchmark database");
    drop(source_connection);
    let connection = Connection::open(&database).expect("open benchmark snapshot");
    connection
        .execute("DELETE FROM message_embeddings", [])
        .expect("clear derived embeddings");
    drop(connection);

    let output = Command::cargo_bin("mash")
        .expect("mash binary")
        .args([
            "--config",
            config.to_str().unwrap(),
            "--db",
            database.to_str().unwrap(),
            "--models-dir",
            models.to_str().unwrap(),
            "index",
        ])
        .output()
        .expect("benchmark semantic index");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let indexed: Value = serde_json::from_slice(&output.stdout).expect("benchmark index JSON");
    let stored = indexed["embeddings"].as_u64().expect("stored embeddings");
    let inferred = indexed["model_inferences"]
        .as_u64()
        .expect("model inference count");
    let elapsed = indexed["embedding_milliseconds"]
        .as_u64()
        .expect("embedding elapsed time");
    assert!(inferred <= stored);
    #[allow(clippy::cast_precision_loss)]
    let rate = stored as f64 / (elapsed.max(1) as f64 / 1_000.0);
    eprintln!(
        "semantic benchmark stored={stored} inferred={inferred} elapsed_ms={elapsed} rate={rate:.1}"
    );
    assert!(rate >= 220.0, "semantic generation rate was {rate:.1}/s");
}
