use super::*;

#[test]
fn cli_masks_archived_credentials_without_changing_local_history() {
    let root = tempfile::tempdir().unwrap();
    let chats = root.path().join("chats");
    std::fs::create_dir(&chats).unwrap();
    let source = chats.join("session.jsonl");
    let body = r#"{"type":"user","sessionId":"session","uuid":"message","message":{"role":"user","content":"credentialneedle password=\"hunter2\" Authorization: Bearer opaque-session-value"}}"#;
    std::fs::write(&source, format!("{body}\n")).unwrap();
    let config = root.path().join("node.toml");
    std::fs::write(&config, format!("version=1\n[providers.claude-code]\nroots=['{}']\n[gold.build]\nrepresentations='lexical-only'\n[gold.query]\nintent='exact-mention'\n", chats.display())).unwrap();
    let db = root.path().join("index.sqlite3");
    let prefix = [
        "--config",
        config.to_str().unwrap(),
        "--db",
        db.to_str().unwrap(),
    ];
    run_json(&[prefix.as_slice(), &["index"]].concat());
    let connection = Connection::open(&db).unwrap();
    let message_id: String = connection
        .query_row("SELECT id FROM messages LIMIT 1", [], |row| row.get(0))
        .unwrap();
    for args in [
        vec!["view", message_id.as_str()],
        vec!["list-chats", "--local"],
        vec!["search", "credentialneedle", "--local", "--no-worker"],
        vec![
            "search",
            "credentialneedle",
            "--local",
            "--no-worker",
            "--unit",
            "messages",
        ],
    ] {
        let value = run_json(&[prefix.as_slice(), args.as_slice()].concat());
        let output = value.to_string();
        assert!(
            !output.contains("hunter2") && !output.contains("opaque-session-value"),
            "{output}"
        );
        assert!(
            output.contains("REDACTED_SECRET") || output.contains("*****"),
            "{output}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(source).unwrap(),
        format!("{body}\n")
    );
    let canonical: String = connection
        .query_row("SELECT content FROM messages LIMIT 1", [], |row| row.get(0))
        .unwrap();
    assert!(canonical.contains("hunter2"));
}
