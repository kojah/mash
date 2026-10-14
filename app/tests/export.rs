use super::*;

#[test]
fn export_imports_history_offline_and_requires_explicit_views_rebuild() {
    let root = tempfile::tempdir().unwrap();
    let provider = root.path().join("provider");
    std::fs::create_dir(&provider).unwrap();
    std::fs::write(provider.join("session.jsonl"),
        r#"{"type":"user","sessionId":"thread","uuid":"message","message":{"role":"user","content":"needle canonical evidence"}}"#).unwrap();
    let config = root.path().join("node.toml");
    std::fs::write(&config, format!("version=1\n[providers.claude-code]\nroots=[{}]\n[views.build]\nrepresentations='lexical-only'\n[views.query]\nintent='exact-mention'\n",
        toml::Value::String(provider.to_str().unwrap().into()))).unwrap();
    let original = root.path().join("original.sqlite3");
    let export = root.path().join("export");
    let imported = root.path().join("imported.sqlite3");
    let models = root.path().join("no-models");
    let base = [
        "--config",
        config.to_str().unwrap(),
        "--models-dir",
        models.to_str().unwrap(),
    ];
    let original_args = [base.as_slice(), &["--db", original.to_str().unwrap()]].concat();
    run_json(&[original_args.as_slice(), &["index"]].concat());
    let created = run_json(
        &[
            original_args.as_slice(),
            &["export", export.to_str().unwrap()],
        ]
        .concat(),
    );
    assert_eq!(created["counts"]["conversations"], 1);
    assert_eq!(created["counts"]["messages"], 1);
    std::fs::remove_dir_all(provider).unwrap();
    let result = run_json(
        &[
            base.as_slice(),
            &[
                "import",
                export.to_str().unwrap(),
                "--into",
                imported.to_str().unwrap(),
            ],
        ]
        .concat(),
    );
    assert_eq!(result["search_ready"], false);
    assert_eq!(result["recommended_action"], "rebuild-local-views");
    let imported_args = [base.as_slice(), &["--db", imported.to_str().unwrap()]].concat();
    let search_args = [imported_args.as_slice(), &["search", "needle", "--local"]].concat();
    let before = Command::cargo_bin("mash")
        .unwrap()
        .args(&search_args)
        .output()
        .unwrap();
    assert_eq!(error_kind(&before), "search-not-ready");
    let rebuilt = run_json(
        &[
            imported_args.as_slice(),
            &["index", "--rebuild-local-views"],
        ]
        .concat(),
    );
    assert_eq!(rebuilt["scanned_files"], 0);
    assert_eq!(rebuilt["model_inferences"], 0);
    let results = run_json(&search_args);
    assert_eq!(results["results"].as_array().unwrap().len(), 1);
    assert!(!models.exists());
}
