use super::*;
use serde_json::json;

#[test]
fn cli_lists_metadata_with_typed_filters_without_installed_models() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    std::fs::write(&config, "version = 1\nproviders = {}\n").unwrap();
    let database = directory.path().join("index.sqlite3");
    let models = directory.path().join("uninstalled-models");
    seed_readiness_database(&database, None);
    let connection = Connection::open(&database).unwrap();
    connection.execute("UPDATE conversations SET title='cBioPortal migration',created_at=1000,updated_at=3000 WHERE id='session'", []).unwrap();
    let arguments = [
        "--config",
        config.to_str().unwrap(),
        "--db",
        database.to_str().unwrap(),
        "--models-dir",
        models.to_str().unwrap(),
        "list-threads",
        "--local",
        "--diagnostics",
        "--provider",
        "codex",
        "--title-contains",
        "CBIOportal",
        "--after",
        "2",
        "--before",
        "4",
        "--sort",
        "title",
        "--direction",
        "asc",
        "--limit",
        "1",
    ];
    let result = run_json(&arguments);
    assert_eq!(result["threads"][0]["conversation_id"], "session");
    assert_eq!(result["threads"][0]["provider"], "codex");
    assert_eq!(result["threads"][0]["updated_at"], 3000);
    assert_eq!(result["next_offset"], Value::Null);
    assert_eq!(
        result["diagnostics"]["selected_artifacts"],
        json!(["metadata"])
    );
    assert_eq!(result["diagnostics"]["requested_scope"], "local");
    assert_eq!(result["diagnostics"]["completed_sources"], json!(["local"]));
    assert_eq!(result["diagnostics"]["returned_results"], 1);
    assert!(!models.exists());
    let output = Command::cargo_bin("mash")
        .unwrap()
        .args([
            "--config",
            config.to_str().unwrap(),
            "--db",
            database.to_str().unwrap(),
            "list-threads",
            "--local",
            "--limit",
            "0",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(error_kind(&output), "usage");
}
