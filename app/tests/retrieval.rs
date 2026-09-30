use super::*;

#[test]
fn search_without_models_fails_before_database_access() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let models = directory.path().join("models");
    let output = Command::cargo_bin("mash")
        .expect("mash binary")
        .args([
            "--db",
            directory.path().join("missing.sqlite3").to_str().unwrap(),
            "--models-dir",
            models.to_str().unwrap(),
            "search",
            "needle",
        ])
        .output()
        .expect("run search");

    assert!(!output.status.success());
    assert_eq!(output.stdout, Vec::<u8>::new());
    let error: Value = serde_json::from_slice(&output.stderr).expect("JSON error");
    assert_eq!(error["error"]["kind"], "model");
    assert_eq!(error["error"]["recommended_action"], "models install");
    assert!(!models.exists());
}

#[test]
fn search_with_missing_embeddings_recommends_index_without_loading_models() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let models = directory.path().join("models");
    let database = directory.path().join("mash.sqlite3");
    create_valid_model_marker(&models);
    seed_readiness_database(&database, None);

    let output = Command::cargo_bin("mash")
        .expect("mash binary")
        .args([
            "--db",
            database.to_str().unwrap(),
            "--models-dir",
            models.to_str().unwrap(),
            "search",
            "needle",
        ])
        .output()
        .expect("run search");

    assert!(!output.status.success());
    assert_eq!(output.stdout, Vec::<u8>::new());
    let error: Value = serde_json::from_slice(&output.stderr).expect("JSON error");
    assert_eq!(error["error"]["kind"], "search-not-ready");
    assert_eq!(error["error"]["recommended_action"], "index");
}

#[test]
fn merged_search_requires_explicit_indexing_before_inference_or_http() {
    let directory = tempfile::tempdir().unwrap();
    let models = directory.path().join("models");
    create_valid_model_marker(&models); // Marker is valid; inference assets are intentionally unusable.
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        r#"
version = 1
providers = {}
[shared]
group_id = "test"
node_id = "test"
api_url = "http://127.0.0.1:9"
token_env = "MASH_TEST_UNSET_INDEX_READINESS_TOKEN"
"#,
    )
    .unwrap();
    let database = directory.path().join("local.sqlite3");
    for exists in [false, true] {
        if exists {
            seed_readiness_database(&database, None);
        }
        let before = std::fs::read(&database).ok();
        let output = Command::cargo_bin("mash")
            .unwrap()
            .args([
                "--config",
                config.to_str().unwrap(),
                "--db",
                database.to_str().unwrap(),
                "--models-dir",
                models.to_str().unwrap(),
                "search",
                "--no-worker",
                "needle",
            ])
            .output()
            .unwrap();
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(output.status.code(), Some(if exists { 8 } else { 3 }));
        assert_eq!(error["error"]["recommended_action"], "index");
        assert_eq!(output.stdout, Vec::<u8>::new());
        assert_eq!(std::fs::read(&database).ok(), before);
    }
}

#[test]
fn invalid_installed_assets_fail_without_lexical_results() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let models = directory.path().join("models");
    std::fs::create_dir_all(&models).expect("model root");
    std::fs::write(models.join("installed.json"), "not-json").expect("broken marker");
    let output = Command::cargo_bin("mash")
        .expect("mash binary")
        .args([
            "--db",
            directory.path().join("mash.sqlite3").to_str().unwrap(),
            "--models-dir",
            models.to_str().unwrap(),
            "search",
            "needle",
        ])
        .output()
        .expect("run search");

    assert!(!output.status.success());
    assert_eq!(output.stdout, Vec::<u8>::new());
    let error: Value = serde_json::from_slice(&output.stderr).expect("JSON error");
    assert_eq!(error["error"]["kind"], "model");
    assert_eq!(error["error"]["recommended_action"], "models install");
}

#[test]
fn missing_listed_model_asset_is_a_typed_model_error() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let models = directory.path().join("models");
    create_valid_model_marker(&models);
    std::fs::remove_file(models.join("dummy-model")).expect("remove listed model asset");
    let output = Command::cargo_bin("mash")
        .expect("mash binary")
        .args([
            "--db",
            directory.path().join("mash.sqlite3").to_str().unwrap(),
            "--models-dir",
            models.to_str().unwrap(),
            "search",
            "needle",
        ])
        .output()
        .expect("run search");

    assert!(!output.status.success());
    let error: Value = serde_json::from_slice(&output.stderr).expect("JSON error");
    assert_eq!(error["error"]["kind"], "model");
    assert_eq!(error["error"]["recommended_action"], "models install");
}

#[test]
fn view_returns_context_only_canonical_content_without_models() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database = directory.path().join("mash.sqlite3");
    seed_readiness_database(&database, Some(""));
    let viewed = run_json(&[
        "--db",
        database.to_str().unwrap(),
        "view",
        "message",
        "--context",
        "0",
    ]);
    assert_eq!(viewed["messages"][0]["content"], "canonical tool output");
}
