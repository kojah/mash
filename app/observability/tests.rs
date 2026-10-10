use super::test_support::Capture;
use super::*;
use std::sync::{Arc, Mutex};
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn partial_index_result_is_traced_as_incomplete_without_source_text() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source-canary.jsonl");
    std::fs::write(&source, "malformed-history-canary\n").unwrap();
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        format!("version = 1\n[providers.claude-code]\nroots = [{source:?}]\n"),
    )
    .unwrap();
    let models = directory.path().join("models");
    crate::test_support::create_valid_model_marker(&models);
    let (bytes, dispatcher) = test_support::capture();
    let result = crate::core::execute(crate::core::Request {
        paths: crate::core::Paths {
            config: Some(config),
            db: Some(directory.path().join("index.sqlite3")),
            models_dir: Some(models),
        },
        operation: crate::core::Operation::Index(crate::core::IndexRequest::default()),
    })
    .with_subscriber(dispatcher)
    .await
    .unwrap();
    assert!(result.needs_retry());
    let output = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    assert!(output.contains("\"outcome\":\"incomplete\""));
    assert!(!output.contains("\"outcome\":\"success\""));
    assert!(!output.contains("source-canary"));
    assert!(!output.contains("malformed-history-canary"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_requests_keep_blocking_spans_and_exclude_dependency_payloads() {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let capture = Capture(bytes.clone());
    let dispatcher = tracing::Dispatch::new(subscriber(move || capture.clone()));
    let request = |id| {
        let dispatcher = dispatcher.clone();
        async move {
            use tracing::Instrument;
            async {
                crate::blocking(move || {
                    let _stage = tracing::info_span!("blocking_stage").entered();
                    tracing::info!(outcome = "success", "blocking completed");
                    tracing::info!(target: "reqwest", secret = "credential-canary", "excluded dependency");
                    Ok(())
                }).await.unwrap();
            }.instrument(tracing::info_span!("request", request_id = id))
                .await;
        }
        .with_subscriber(dispatcher)
    };
    tokio::join!(request(11_u64), request(22_u64));
    let output = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    assert!(!output.contains("credential-canary"));
    let mut requests = std::collections::BTreeSet::new();
    for line in output.lines() {
        let event: serde_json::Value = serde_json::from_str(line).unwrap();
        if event["fields"]["message"] == "blocking completed" {
            let spans = event["spans"].as_array().unwrap();
            requests.insert(spans[0]["request_id"].as_u64().unwrap());
            assert_eq!(spans[1]["name"], "blocking_stage");
        }
    }
    assert_eq!(requests, [11, 22].into());
    assert!(output.contains("time.busy"));
}

#[tokio::test]
async fn failed_core_request_does_not_record_query_filters_paths_or_error_text() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    std::fs::write(&config, "version = 1\nproviders = {}\n").unwrap();
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let capture = Capture(bytes.clone());
    let subscriber = subscriber(move || capture.clone());
    let request = crate::core::Request {
        paths: crate::core::Paths {
            config: Some(config),
            db: Some(directory.path().join("path-canary")),
            models_dir: None,
        },
        operation: crate::core::Operation::Search {
            shared_vector_search: None,
            fusion: crate::views::config::FusionOverrides::default(),
            ranking: None,
            diagnostics: false,
            query: "query-canary".into(),
            intent: None,
            limit: Some(5),
            unit: Some(crate::search_results::SearchUnit::Chats),
            filters: Box::default(),
            local: true,
            shared: true,
        },
    };
    assert!(
        crate::core::execute(request)
            .with_subscriber(subscriber)
            .await
            .is_err()
    );
    let output = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    assert!(!output.contains("query-canary"));
    assert!(!output.contains("path-canary"));
    assert!(!output.contains("mutually exclusive"));
    assert!(output.contains("Usage"), "{output}");
    assert!(output.contains("request_id"));
}

#[test]
fn parser_threads_inherit_request_context_without_recording_source_content() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("path-canary.jsonl");
    std::fs::write(&source, r#"{"type":"user","sessionId":"parent","uuid":"u1","message":{"role":"user","content":"history-canary"}}"#).unwrap();
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let capture = Capture(bytes.clone());
    let dispatcher = tracing::Dispatch::new(subscriber(move || capture.clone()));
    tracing::dispatcher::with_default(&dispatcher, || {
        tracing::info_span!("request", request_id = 44_u64).in_scope(|| {
            let mut storage =
                crate::storage::Storage::open_writer(&directory.path().join("index.sqlite3"))
                    .unwrap();
            let options = crate::silver::IndexOptions {
                providers: [(crate::provider::Provider::ClaudeCode, vec![source])].into(),
                roots_are_authoritative: true,
                since_days: None,
            };
            crate::silver::index(&mut storage, &options).unwrap();
        });
    });
    let output = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    assert!(!output.contains("path-canary"));
    assert!(!output.contains("history-canary"));
    let mut stages = std::collections::BTreeSet::new();
    for line in output.lines() {
        let event: serde_json::Value = serde_json::from_str(line).unwrap();
        if matches!(
            event["span"]["name"].as_str(),
            Some("parse_source" | "apply_source")
        ) {
            assert_eq!(event["spans"][0]["request_id"], 44);
            stages.insert(event["span"]["name"].as_str().unwrap().to_owned());
        }
    }
    assert_eq!(stages.len(), 2);
}

#[tokio::test]
#[ignore = "requires explicitly installed MASH_TEST_MODELS_DIR"]
async fn real_model_index_traces_planning_inference_and_storage_under_one_request() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("path-canary.jsonl");
    std::fs::write(&source, r#"{"type":"user","sessionId":"parent","uuid":"u1","message":{"role":"user","content":"history-canary about indexing search models"}}"#).unwrap();
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        toml::to_string(&toml::toml! {
            version=1
            providers={claude-code={roots=[(source.to_str().unwrap())]}}
        })
        .unwrap(),
    )
    .unwrap();
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let capture = Capture(bytes.clone());
    let subscriber = subscriber(move || capture.clone());
    let response = crate::core::execute(crate::core::Request {
        paths: crate::core::Paths {
            config: Some(config),
            db: Some(directory.path().join("index.sqlite3")),
            models_dir: Some(std::env::var_os("MASH_TEST_MODELS_DIR").unwrap().into()),
        },
        operation: crate::core::Operation::Index(crate::core::IndexRequest {
            all_history: true,
            ..Default::default()
        }),
    })
    .with_subscriber(subscriber)
    .await
    .unwrap();
    assert!(matches!(response, crate::core::Response::Index(_)));
    let storage =
        crate::storage::Storage::open_existing(&directory.path().join("index.sqlite3")).unwrap();
    assert_eq!(storage.counts().unwrap().messages, 1);
    let models =
        crate::semantic::Models::new(std::env::var_os("MASH_TEST_MODELS_DIR").unwrap().into());
    assert_eq!(storage.embedding_count(&models.generation()).unwrap(), 1);
    let output = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    assert!(!output.contains("path-canary"));
    assert!(!output.contains("history-canary"));
    let mut stages = std::collections::BTreeSet::new();
    for line in output.lines() {
        let event: serde_json::Value = serde_json::from_str(line).unwrap();
        if let Some(name) = event["span"]["name"].as_str() {
            stages.insert(name.to_owned());
            if matches!(
                name,
                "cpu_embedding_batch" | "coreml_embedding_batch" | "write_embeddings"
            ) {
                assert_eq!(event["spans"][0]["name"], "request");
                assert!(event["spans"][0]["request_id"].is_u64());
            }
        }
    }
    for stage in [
        "discover_sources",
        "prepare_sources",
        "parser_admission",
        "parse_source",
        "queue_parsed_source",
        "apply_source",
        "plan_embeddings",
        "read_embedding_wave",
        "embedding_wave",
        "write_embeddings",
        "commit_writer",
    ] {
        assert!(stages.contains(stage), "missing stage {stage}");
    }
    let inference = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "coreml_embedding_batch"
    } else {
        "cpu_embedding_batch"
    };
    assert!(stages.contains(inference));
}
