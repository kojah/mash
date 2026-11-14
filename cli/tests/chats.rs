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
    let connection = super::support::open_silver(&database);
    connection.execute("UPDATE chats SET title='cBioPortal migration',created_at=1000,updated_at=3000 WHERE id='session'", []).unwrap();
    let arguments = [
        "--config",
        config.to_str().unwrap(),
        "--db",
        database.to_str().unwrap(),
        "--models-dir",
        models.to_str().unwrap(),
        "list-chats",
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
    assert_eq!(result["chats"][0]["chat_id"], "session");
    assert_eq!(result["chats"][0]["provider"], "codex");
    assert_eq!(result["chats"][0]["updated_at"], 3000);
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
            "list-chats",
            "--local",
            "--limit",
            "0",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(error_kind(&output), "usage");
}

/// S-001/S-002/S-003: list-chats rows name a message `view` accepts, a chat ID
/// or unknown ID is a typed not-found error with a non-zero exit, and help
/// describes the argument.
#[test]
fn cli_view_reads_list_chats_lookup_and_rejects_chat_and_unknown_ids() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    std::fs::write(&config, "version = 1\nproviders = {}\n").unwrap();
    let database = directory.path().join("index.sqlite3");
    seed_readiness_database(&database, None);
    let base = [
        "--config",
        config.to_str().unwrap(),
        "--db",
        database.to_str().unwrap(),
    ];
    let listed = run_json(&[base.as_slice(), &["list-chats", "--local"]].concat());
    assert_eq!(
        listed["chats"][0]["lookup"],
        json!({"view": {"id": "message"}})
    );
    let id = listed["chats"][0]["lookup"]["view"]["id"].as_str().unwrap();
    let viewed = run_json(&[base.as_slice(), &["view", id]].concat());
    assert_eq!(viewed["messages"][0]["id"], "message");
    // R2 S-003: the default window names its chat and says nothing was cut.
    assert_eq!(viewed["chat_id"], "session", "{viewed}");
    assert_eq!(viewed["message_count"], 1, "{viewed}");
    assert_eq!(viewed["omitted_before"], 0, "{viewed}");
    assert_eq!(viewed["omitted_after"], 0, "{viewed}");
    assert!(viewed.get("notice").is_none(), "{viewed}");
    // Once the chat continues, the same default view says so.
    open_silver(&database)
        .execute(
            "INSERT INTO messages(id,chat_id,ordinal,role,content_packed) VALUES('reply','session',1,'assistant','use logrotate')",
            [],
        )
        .unwrap();
    let viewed = run_json(&[base.as_slice(), &["view", id]].concat());
    assert_eq!(viewed["messages"].as_array().unwrap().len(), 1, "{viewed}");
    assert_eq!(viewed["message_count"], 2, "{viewed}");
    assert_eq!(viewed["omitted_after"], 1, "{viewed}");
    let notice = viewed["notice"].as_str().unwrap();
    assert!(
        notice.contains("Showing 1 of this chat's 2 messages; 1 later message is not shown.")
            && notice.contains("`--context N`"),
        "{notice}"
    );
    let all = run_json(&[base.as_slice(), &["view", "reply", "--context", "1"]].concat());
    assert_eq!(all["messages"].as_array().unwrap().len(), 2, "{all}");
    assert_eq!(all["omitted_before"], 0, "{all}");
    assert!(all.get("notice").is_none(), "{all}");
    for (id, expected) in [
        ("session", "is a chat ID"),
        ("no-such-thing", "no message with ID"),
    ] {
        let output = Command::cargo_bin("mash")
            .unwrap()
            .args([base.as_slice(), &["view", id, "--context", "10"]].concat())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(13), "{id}");
        assert!(output.stdout.is_empty(), "{id}");
        assert_eq!(error_kind(&output), "not-found", "{id}");
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        let message = error["error"]["message"].as_str().unwrap();
        assert!(message.contains(expected), "{id}: {message}");
        if id == "session" {
            assert!(message.contains("\"message\""), "{message}");
        }
    }
    let output = Command::cargo_bin("mash")
        .unwrap()
        .args(["view", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("<MESSAGE_ID>"), "{help}");
    assert!(help.contains("lookup.view.id"), "{help}");
    assert!(help.contains("before and after"), "{help}");
}
