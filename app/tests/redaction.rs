use super::*;

#[test]
fn cli_masks_archived_credentials_without_changing_local_history() {
    cli_output_redaction(true);
}

#[test]
fn output_redaction_can_be_turned_off_in_configuration() {
    cli_output_redaction(false);
}

fn cli_output_redaction(masked: bool) {
    let root = tempfile::tempdir().unwrap();
    let chats = root.path().join("chats");
    std::fs::create_dir(&chats).unwrap();
    let source = chats.join("session.jsonl");
    let body = r#"{"type":"user","sessionId":"session","uuid":"message","message":{"role":"user","content":"credentialneedle password=\"hunter2\" Authorization: Bearer opaque-session-value"}}"#;
    std::fs::write(&source, format!("{body}\n")).unwrap();
    let config = root.path().join("node.toml");
    std::fs::write(&config, format!("version=1\n[providers.claude-code]\nroots=['{}']\n[views.build]\nrepresentations='lexical-only'\n[views.query]\nintent='exact-mention'\n{}", chats.display(), if masked { "" } else { "[redaction]\noutput=false\n" })).unwrap();
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
        .query_row(
            "SELECT id FROM objects WHERE origin='provider' AND object_role='entry' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    for args in [
        vec!["view", message_id.as_str()],
        vec!["list-chats", "--local"],
        vec!["search", "credentialneedle", "--local"],
        vec![
            "search",
            "credentialneedle",
            "--local",
            "--unit",
            "messages",
        ],
    ] {
        let value = run_json(&[prefix.as_slice(), args.as_slice()].concat());
        let output = value.to_string();
        if masked {
            assert!(
                !output.contains("hunter2") && !output.contains("opaque-session-value"),
                "{output}"
            );
            assert!(
                output.contains("REDACTED_SECRET") || output.contains("*****"),
                "{output}"
            );
        } else if args[0] != "list-chats" {
            // Listings carry only chat metadata, which holds no secret here.
            assert!(output.contains("hunter2"), "{output}");
            assert!(!output.contains("REDACTED_SECRET"), "{output}");
        }
    }
    assert_eq!(
        std::fs::read_to_string(source).unwrap(),
        format!("{body}\n")
    );
    let canonical: String = connection
        .query_row(
            "SELECT content FROM objects WHERE origin='provider' AND object_role='entry' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(canonical.contains("hunter2"));
}
