use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_forwards_shared_vector_overrides_without_persisting_them() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("node.toml");
    let database = root.path().join("index.sqlite3");
    let models = root.path().join("uninstalled-models");
    std::fs::write(&config, "version=1\nproviders={}\n[views.build]\nrepresentations='lexical-only'\n[views.query]\nintent='exact-mention'\n").unwrap();
    let base = [
        "--config",
        config.to_str().unwrap(),
        "--db",
        database.to_str().unwrap(),
        "--models-dir",
        models.to_str().unwrap(),
    ];
    run_json(&[base.as_slice(), &["index"]].concat());
    let before = std::fs::read(&database).unwrap();
    let mut service = test_support::vector::Service::new().await;
    std::fs::write(&config, service.configuration()).unwrap();
    for (algorithm, expected) in [
        (None, None),
        (
            Some("hnsw"),
            Some(serde_json::json!({"algorithm":"hnsw","ef_runtime":80})),
        ),
        (None, None),
        (
            Some("exact"),
            Some(serde_json::json!({"algorithm":"exact"})),
        ),
        (None, None),
    ] {
        let mut args = [
            base.as_slice(),
            &[
                "search",
                "database",
                "--shared",
                "--intent",
                "related-discussion",
                "--ranking",
                "fusion",
                "--diagnostics",
            ],
        ]
        .concat();

        if let Some(algorithm) = algorithm {
            args.extend(["--shared-vector-search", algorithm]);
            if algorithm == "hnsw" {
                args.extend(["--shared-hnsw-ef", "80"]);
            }
        }
        let response = run_json(&args);
        assert_eq!(response["shared"], true);
        assert_eq!(response["results"], serde_json::json!([]));
        assert_eq!(
            service.received().await.get("vector_search"),
            expected.as_ref()
        );
        assert_eq!(
            response["diagnostics"].get("shared_vector_override"),
            expected.as_ref()
        );
        assert_eq!(std::fs::read(&database).unwrap(), before);
    }
    assert!(!models.exists());
}
