use super::*;

#[test]
#[ignore = "requires installed MASH_TEST_MODELS_DIR and disposable Qdrant REST/gRPC endpoints"]
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
    configure_qdrant(&config);
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

    assert_exact_search_without_models(&config, &database);
    assert_scoped_message_search(&config, &database, model_arg);
    assert_thread_search_and_view(&config, &database, model_arg);
    assert_views_query_defaults(&config, &database, model_arg);
    assert_views_ranking_defaults(&config, &database, model_arg);
    assert_source_independent_views_rebuild(&config, &database, &claude, &codex, model_arg);
    cleanup_local_gold(&database, "local");
}

fn assert_exact_search_without_models(config: &Path, database: &Path) {
    let original_config = std::fs::read_to_string(config).unwrap();
    let missing_models = database.with_extension("missing-models");
    let before = std::fs::read(database).unwrap();
    // Dense recipe staleness must not disable unchanged lexical artifacts.
    std::fs::write(
        config,
        format!("{original_config}\n[views.build]\nembedding_text='title-and-dialogue'\n[views.query]\nintent='exact-mention'\n"),
    )
    .unwrap();
    let status = run_json(&[
        "--config",
        config.to_str().unwrap(),
        "--db",
        database.to_str().unwrap(),
        "--models-dir",
        missing_models.to_str().unwrap(),
        "status",
    ]);
    assert_eq!(status["ready"], true);
    assert_eq!(status["local_artifacts"]["lexical"], "ready");
    assert_eq!(status["local_artifacts"]["dense"], "stale");
    assert_eq!(status["views"]["build"]["embedding_text"], "dialogue");
    let mut durations = Vec::new();
    for unit in ["chats", "messages"] {
        for query in ["login token", "private-output-marker"] {
            let started = std::time::Instant::now();
            let result = run_json(&[
                "--config",
                config.to_str().unwrap(),
                "--db",
                database.to_str().unwrap(),
                "--models-dir",
                missing_models.to_str().unwrap(),
                "search",
                query,
                "--intent",
                "exact-mention",
                "--local",
                "--unit",
                unit,
            ]);
            durations.push(started.elapsed().as_millis());
            assert_eq!(result["realized_mode"], "lexical");
            assert_eq!(result["intent"], "exact-mention");
            assert_eq!(
                result["results"].as_array().unwrap().len(),
                usize::from(query == "login token")
            );
            assert!(
                !serde_json::to_string(&result["results"])
                    .unwrap()
                    .contains("private-output-marker")
            );
        }
    }
    assert_eq!(std::fs::read(database).unwrap(), before);
    assert!(!missing_models.exists());
    std::fs::write(config, original_config).unwrap();
    eprintln!(
        "exact CLI synthetic two-message fixture: process-inclusive latency ms={durations:?}; SQLite bytes={}",
        before.len()
    );
}

fn assert_source_independent_views_rebuild(
    config: &Path,
    database: &Path,
    claude: &Path,
    codex: &Path,
    models: &str,
) {
    let connection = open_silver(database);
    let silver_tables = [
        "conversation_enrichment",
        "sessions",
        "messages",
        "turns",
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
    std::fs::write(config, format!("version=1\nproviders={{claude-code={{roots=[{:?}]}},codex={{roots=[{:?}]}}}}\n[index]\ndestination='both'\n[shared]\ngroup_id='group'\nnode_id='local'\napi_url='http://127.0.0.1:9'\ntoken_env='MASH_VIEWS_REBUILD_NO_TOKEN'\n",claude.to_str().unwrap(),codex.to_str().unwrap())).unwrap();
    configure_qdrant(config);
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
            "--rebuild-local-views",
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
        assert_eq!(indexed["rebuild_local_views"], true);
        assert_eq!(indexed["realized_mode"], "hybrid");
        assert_eq!(indexed["scanned_files"], 0);
        assert_eq!(indexed["ingestion_milliseconds"], 0);
        assert_eq!(indexed["model_inferences"], if full { 2 } else { 0 });
        assert!(indexed["shared"].is_null());
        for table in silver_tables {
            let unchanged = connection.query_row(&format!(
                "SELECT NOT EXISTS(SELECT * FROM {table} EXCEPT SELECT * FROM expected_{table}) AND NOT EXISTS(SELECT * FROM expected_{table} EXCEPT SELECT * FROM {table})"), [], |row| row.get::<_, bool>(0)).unwrap();
            assert!(unchanged, "views changed silver table {table}");
        }
    }
    assert_views_recipe_switches(config, database, models);
    assert_views_rebuild_recovers_without_sources(config, database, models);
    for table in silver_tables {
        let unchanged = connection.query_row(&format!(
            "SELECT NOT EXISTS(SELECT * FROM {table} EXCEPT SELECT * FROM expected_{table}) AND NOT EXISTS(SELECT * FROM expected_{table} EXCEPT SELECT * FROM {table})"), [], |row| row.get::<_, bool>(0)).unwrap();
        assert!(unchanged, "recovery changed silver table {table}");
    }
    std::fs::write(config, original).unwrap();
    assert_scoped_message_search(config, database, models);
}

fn assert_views_recipe_switches(config: &Path, database: &Path, models: &str) {
    let build_config = std::fs::read_to_string(config).unwrap();
    for (recipe, budget) in [
        ("title-and-dialogue", ""),
        ("title-and-dialogue", "max_embedding_chars=256\n"),
        ("dialogue", "max_embedding_chars=512\n"),
        ("dialogue", "text_preparation='collapse-whitespace'\n"),
        (
            "title-and-dialogue",
            "text_preparation='collapse-whitespace'\n",
        ),
        ("dialogue", ""),
    ] {
        std::fs::write(
            config,
            format!("{build_config}\n[views.build]\nembedding_text = '{recipe}'\n{budget}"),
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
                "--rebuild-local-views",
            ])
            .output()
            .unwrap();
        assert!(
            rebuilt.status.success(),
            "{}",
            String::from_utf8_lossy(&rebuilt.stderr)
        );
        let indexed: Value = serde_json::from_slice(&rebuilt.stdout).unwrap();
        assert_eq!(indexed["views"]["build"]["embedding_text"], recipe);
        assert_eq!(
            indexed["views"]["build"]["max_embedding_chars"],
            match budget {
                "max_embedding_chars=256\n" => serde_json::json!(256),
                "max_embedding_chars=512\n" => serde_json::json!(512),
                _ => Value::Null,
            }
        );
        assert_eq!(indexed["model_inferences"], 2);
        assert_eq!(indexed["scanned_files"], 0);
        assert!(indexed["shared"].is_null());
        if recipe == "title-and-dialogue" {
            assert_views_rebuild_recovers_without_sources(config, database, models);
        }
    }
    std::fs::write(config, &build_config).unwrap();
    let unchanged = run_json(&[
        "--config",
        config.to_str().unwrap(),
        "--db",
        database.to_str().unwrap(),
        "--models-dir",
        models,
        "index",
        "--rebuild-local-views",
    ]);
    // The last recipe is already the default; removing its explicit TOML
    // spelling must preserve both certified vectors without new inference.
    assert_eq!(unchanged["views"]["schema_version"], 1);
    assert_eq!(unchanged["views"]["build"]["embedding_text"], "dialogue");
    assert_eq!(unchanged["model_inferences"], 0);
    assert_eq!(unchanged["embeddings"], 2);
    assert_eq!(unchanged["reused_embeddings"], 2);
    assert_eq!(unchanged["scanned_files"], 0);
}

fn assert_views_rebuild_recovers_without_sources(config: &Path, database: &Path, models: &str) {
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
            "--rebuild-local-views",
            "--full",
        ])
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert_eq!(failed.stdout, [] as [u8; 0]);
    let status = run_json(&[
        "--config",
        config.to_str().unwrap(),
        "--db",
        database.to_str().unwrap(),
        "--models-dir",
        models,
        "status",
    ]);
    assert_eq!(
        status["ready"], false,
        "failed replacement must remain unready"
    );
    assert_ne!(status["local_artifacts"]["dense"], "ready");
    let recovered = run_json(&[
        "--config",
        config.to_str().unwrap(),
        "--db",
        database.to_str().unwrap(),
        "--models-dir",
        models,
        "index",
        "--rebuild-local-views",
    ]);
    assert_eq!(recovered["realized_mode"], "hybrid");
    assert_eq!(recovered["model_inferences"], 2);
    assert_eq!(recovered["scanned_files"], 0);
    assert!(recovered["shared"].is_null());
}

fn assert_views_query_defaults(config: &Path, database: &Path, models: &str) {
    let canonical_config = std::fs::read_to_string(config).unwrap();
    let before = std::fs::read(database).unwrap();
    let mut fusion_score = None;
    for (unit, limit, weight, override_weight) in [
        ("messages", 1, 1, None),
        ("messages", 1, 4, None),
        ("messages", 1, 4, Some(2)),
        ("messages", 1, 4, None),
        ("chats", 3, 1, None),
        ("chats", 3, 4, Some(2)),
    ] {
        std::fs::write(
            config,
            format!("{canonical_config}\n[views.query]\nunit = '{unit}'\nlimit = {limit}\n[views.query.fusion]\nlexical_weight={weight}\nsemantic_weight={weight}\n"),
        )
        .unwrap();
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
        if let Some(weight) = override_weight {
            let weight = weight.to_string();
            command.args(["--lexical-weight", &weight, "--semantic-weight", &weight]);
            command.arg("--diagnostics");
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
        if let Some(weight) = override_weight {
            let diagnostics = &found["diagnostics"];
            assert_eq!(diagnostics["policy_version"], 1);
            assert_eq!(diagnostics["fusion"]["lexical_weight"], weight);
            assert_eq!(diagnostics["limit"], limit);
            assert_eq!(diagnostics["unit"], unit);
            assert_eq!(diagnostics["requested_scope"], "local");
            assert_eq!(
                diagnostics["completed_sources"],
                serde_json::json!(["local"])
            );
            assert_eq!(diagnostics["returned_results"], results.len());
            assert_eq!(
                diagnostics["selected_artifacts"],
                serde_json::json!(["metadata", "lexical", "dense"])
            );
            assert!(diagnostics["elapsed_ms"].is_u64());
            let stages = diagnostics["stages"].as_array().unwrap();
            assert!(stages.len() <= 8);
            for name in ["local-candidates", "embed", "rerank", "fusion"] {
                let stage = stages.iter().find(|s| s["stage"] == name).unwrap();
                assert_eq!(stage["calls"], stage["completed_calls"]);
                assert!(stage["items_out"].is_u64());
                if name == "local-candidates" {
                    assert!(
                        stage["items_in"].is_null(),
                        "Qdrant does not report vectors examined"
                    );
                    assert_eq!(stage["filters_before_limit"], true);
                }
            }
        } else {
            assert!(found.get("diagnostics").is_none());
        }
        if unit == "messages" {
            let score = results[0]["fusion_score"].as_f64().unwrap();
            if weight == 1 {
                fusion_score = Some(score);
            } else {
                let expected = f64::from(override_weight.unwrap_or(weight));
                assert!((score - fusion_score.unwrap() * expected).abs() < 1e-6);
            }
        }
    }
    assert_eq!(std::fs::read(database).unwrap(), before);
    std::fs::write(config, canonical_config).unwrap();
}

fn assert_views_ranking_defaults(config: &Path, database: &Path, models: &str) {
    let original = std::fs::read_to_string(config).unwrap();
    let before = std::fs::read(database).unwrap();
    for (ranking, override_ranking, expected) in [
        ("contextual-rerank", None, "contextual-rerank"),
        ("contextual-rerank", Some("fusion"), "fusion"),
        ("contextual-rerank", None, "contextual-rerank"),
        ("fusion", Some("contextual-rerank"), "contextual-rerank"),
        ("fusion", None, "fusion"),
    ] {
        std::fs::write(
            config,
            format!("{original}\n[views.query]\nranking='{ranking}'\n"),
        )
        .unwrap();
        for unit in ["chats", "messages"] {
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
                "--unit",
                unit,
                "--diagnostics",
                "authentication",
            ]);

            if let Some(override_ranking) = override_ranking {
                command.args(["--ranking", override_ranking]);
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let response: Value = serde_json::from_slice(&output.stdout).unwrap();
            let results = response["results"].as_array().unwrap();
            assert_ne!(results.as_slice(), &[] as &[Value]);
            assert_eq!(response["diagnostics"]["ranking"], expected);
            let stages = response["diagnostics"]["stages"].as_array().unwrap();
            assert!(stages.iter().any(|s| s["stage"] == "embed"));
            assert_eq!(
                stages.iter().any(|s| s["stage"] == "rerank"),
                expected == "contextual-rerank"
            );
            assert_eq!(
                std::fs::read(database).unwrap(),
                before,
                "query policy modified index"
            );
        }
    }
    std::fs::write(config, original).unwrap();
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
        "--chat",
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
        "list-chats",
        "--local",
        "--provider",
        "claude-code",
    ]);
    let thread = catalog["chats"][0]["chat_id"].as_str().unwrap();
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
    assert_eq!(threads["unit"], "chats");
    let results = threads["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["chat_id"], "hybrid");
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
    // The snapshot copies silver only; its views starts empty.
    drop(source_connection);

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
