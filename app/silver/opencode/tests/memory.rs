use super::*;

#[test]
fn large_tool_heavy_session_preserves_every_message_and_checkpoint() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("opencode.db");
    let upstream = fixture(&path);
    let output = "x".repeat(1024 * 1024);
    for i in 0..40 {
        let data = serde_json::json!({"content": [
            {"type": "text", "text": format!("answer {i}")},
            {"type": "tool", "name": "read", "state": {"output": output}}
        ]});
        upstream
            .execute(
                "INSERT INTO session_message VALUES(?1,'alpha','assistant',?2,1790726401000,?3)",
                rusqlite::params![format!("large-{i}"), i + 10, data.to_string()],
            )
            .unwrap();
    }
    let source = discover(&path).unwrap().remove(0);
    let checkpoint = source.stamp().unwrap();
    let chat = parse(&source.identity).unwrap().chat.unwrap();
    assert_eq!(chat.messages.len(), 42);
    for (i, message) in chat.messages.rows().unwrap().skip(2).enumerate() {
        let message = message.unwrap();
        assert!(message.content.contains(&output));
        assert_eq!(
            message.dialogue_text.as_deref(),
            Some(format!("answer {i}").as_str())
        );
    }
    drop(chat);
    let local = directory.path().join("index.sqlite3");
    let mut writer = Storage::open_writer(&local).unwrap();
    index(&mut writer, &options(&path)).unwrap();
    writer.commit_writer().unwrap();
    assert_eq!(writer.counts().unwrap().messages, 43);
    assert!(
        writer
            .source_checkpoint_matches(Provider::Opencode, &source.identity, &checkpoint)
            .unwrap()
    );
    drop(writer);
    let mut writer = Storage::open_writer(&local).unwrap();
    index(&mut writer, &options(&path)).unwrap();
    writer.commit_writer().unwrap();
    assert_eq!(writer.counts().unwrap().messages, 43);
}

#[test]
fn oversized_current_and_fragmented_legacy_sessions_preserve_index_and_checkpoint() {
    for current in [true, false] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("opencode.db");
        let upstream = fixture(&path);
        let local = directory.path().join("index.sqlite3");
        let source = discover(&path).unwrap().remove(usize::from(!current));
        let checkpoint = source.stamp().unwrap();
        let mut writer = Storage::open_writer(&local).unwrap();
        index(&mut writer, &options(&path)).unwrap();
        writer.commit_writer().unwrap();
        drop(writer);

        // Each native record is small enough to pass the 8 MiB boundary, but
        // canonical text plus searchable copies must share one retention cap.
        let text = "bounded history ".repeat(70_000);
        let data = if current {
            serde_json::to_string(&serde_json::json!({"text": text})).unwrap()
        } else {
            serde_json::to_string(&serde_json::json!({"type":"text", "text": text})).unwrap()
        };
        assert!(data.len() < 8 * 1024 * 1024);
        for i in 0..31 {
            if current {
                upstream
                    .execute(
                        "INSERT INTO session_message VALUES(?1,'alpha','user',?2,1790726401000,?3)",
                        rusqlite::params![format!("large-{i}"), i + 10, data],
                    )
                    .unwrap();
            } else {
                upstream
                    .execute(
                        "INSERT INTO part VALUES(?1,'legacy','beta',?2)",
                        rusqlite::params![format!("large-{i}"), data],
                    )
                    .unwrap();
            }
        }
        let error = parse(&source.identity).err().unwrap();
        assert!(error.error.message.contains("memory limit"));
        let mut writer = Storage::open_writer(&local).unwrap();
        assert!(index(&mut writer, &options(&path)).is_err());
        writer.commit_writer().unwrap();
        assert_eq!(writer.counts().unwrap().messages, 3);
        assert!(
            writer
                .source_checkpoint_matches(Provider::Opencode, &source.identity, &checkpoint)
                .unwrap()
        );
        assert!(
            !crate::storage::test_support::projections_with_text(&writer, "cbioportal", None)
                .is_empty()
        );
        drop(writer);

        upstream
            .execute_batch(
                "DELETE FROM session_message WHERE id LIKE 'large-%';
            DELETE FROM part WHERE id LIKE 'large-%';",
            )
            .unwrap();
        let mut writer = Storage::open_writer(&local).unwrap();
        index(&mut writer, &options(&path)).unwrap();
        writer.commit_writer().unwrap();
        assert_eq!(writer.counts().unwrap().messages, 3);
        assert!(
            writer
                .source_checkpoint_matches(
                    Provider::Opencode,
                    &source.identity,
                    &source.stamp().unwrap()
                )
                .unwrap()
        );
    }
}

#[test]
fn structural_amplification_and_metadata_overflow_fail_before_retention() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("opencode.db");
    let upstream = fixture(&path);
    let source = discover(&path).unwrap().remove(0);
    let data = format!(
        "{{\"content\":[{}{{\"type\":\"other\"}}]}}",
        "{\"type\":\"other\"},".repeat(65_536)
    );
    assert!(data.len() < 8 * 1024 * 1024);
    upstream
        .execute(
            "UPDATE session_message SET data=?1 WHERE id='assistant'",
            [&data],
        )
        .unwrap();
    assert!(
        parse(&source.identity)
            .err()
            .unwrap()
            .error
            .message
            .contains("structural")
    );
    upstream
        .execute(
            "UPDATE session SET title=?1 WHERE id='alpha'",
            ["x".repeat(8 * 1024 * 1024 + 1)],
        )
        .unwrap();
    assert!(
        parse(&source.identity)
            .err()
            .unwrap()
            .error
            .message
            .contains("memory limit")
    );
}
