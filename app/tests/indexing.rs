use super::*;

mod incomplete;

#[test]
fn index_without_models_fails_before_creating_the_database() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database = directory.path().join("mash.sqlite3");
    let models = directory.path().join("models");
    let output = Command::cargo_bin("mash")
        .expect("mash binary")
        .args([
            "--db",
            database.to_str().unwrap(),
            "--models-dir",
            models.to_str().unwrap(),
            "index",
        ])
        .output()
        .expect("run index");

    assert!(!output.status.success());
    assert_eq!(output.stdout, Vec::<u8>::new());
    let error: Value = serde_json::from_slice(&output.stderr).expect("JSON error");
    assert_eq!(error["error"]["kind"], "model");
    assert_eq!(error["error"]["recommended_action"], "models install");
    assert!(!database.exists());
    assert!(!models.exists());
}

#[test]
fn index_without_pending_embeddings_validates_but_does_not_load_models() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database = directory.path().join("mash.sqlite3");
    let models = directory.path().join("models");
    let config = directory.path().join("config.toml");
    let claude = directory.path().join("claude");
    let codex = directory.path().join("codex");
    std::fs::create_dir_all(&claude).expect("Claude root");
    std::fs::create_dir_all(&codex).expect("Codex root");
    seed_readiness_database(&database, Some(""));
    create_valid_model_marker(&models);
    write_config(&config, &claude, &codex);

    let output = Command::cargo_bin("mash")
        .expect("mash binary")
        .args(["--config", config.to_str().unwrap()])
        .args(["--db", database.to_str().unwrap()])
        .args(["--models-dir", models.to_str().unwrap()])
        .arg("index")
        .output()
        .expect("run no-op index");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let indexed: Value = serde_json::from_slice(&output.stdout).expect("index JSON");
    assert_eq!(indexed["embeddings"], 0);
    assert_eq!(indexed["model_load_milliseconds"], 0);
}
