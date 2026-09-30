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
    assert_thread_search_and_view(&config, &database, model_arg);
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
