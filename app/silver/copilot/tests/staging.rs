use super::*;
use crate::silver::{messages::Messages, parsers::stable_id};

fn write_large(path: &Path, body: &str) {
    let mut output = File::create(path).unwrap();
    writeln!(output, "{}", header("large-native-session")).unwrap();
    writeln!(
        output,
        "{}",
        record(
            "first-human",
            "user.message",
            json!({"content":"Human importer question","source":"user"})
        )
    )
    .unwrap();
    for index in 0..600 {
        writeln!(
            output,
            "{}",
            record(
                &format!("context-{index}"),
                "user.message",
                json!({"content":body,"source":"skill-pdf"})
            )
        )
        .unwrap();
    }
    let mut subagent = record(
        "subagent",
        "assistant.message",
        json!({"content":"private subagent answer"}),
    );
    subagent["agentId"] = json!("subagent-lane");
    writeln!(output, "{subagent}").unwrap();
    writeln!(
        output,
        "{}",
        record(
            "clear",
            "session.context_cleared",
            json!({"initialMessage":"model reset note"})
        )
    )
    .unwrap();
    writeln!(
        output,
        "{}",
        record(
            "last-human",
            "user.message",
            json!({"content":"Fresh structural variant question","source":"user"})
        )
    )
    .unwrap();
    drop(output);
}

#[test]
fn large_native_log_spills_payloads_without_changing_titles_or_context_lanes() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let body = "privateinjectedcontext".repeat(1600);
    write_large(file.path(), &body);

    let chat = parse(file.path()).unwrap().chat.unwrap();
    assert!(matches!(chat.messages, Messages::Staged(_)));
    let scratch = chat.messages.staging_path().unwrap().to_path_buf();
    assert_eq!(chat.title.as_deref(), Some("Human importer question"));
    assert_eq!(chat.messages.summary().unwrap().searchable, 2);
    let mut count = 0;
    let mut previous = String::new();
    for row in chat.messages.rows().unwrap() {
        let row = row.unwrap();
        let ordinal = row.ordinal;
        assert_eq!(ordinal, count);
        match ordinal {
            0 | 601 | 602 => assert_eq!(row.context, MessageContext::Root),
            _ => assert_eq!(
                row.context,
                MessageContext::Parent {
                    id: previous.clone()
                }
            ),
        }
        if (1..=600).contains(&ordinal) {
            assert_eq!(row.content, body);
            assert_eq!(row.dialogue_text.as_deref(), Some(""));
            assert_eq!(
                row.id,
                stable_id(
                    "message",
                    &[
                        "copilot",
                        "large-native-session",
                        &format!("context-{}", ordinal - 1)
                    ]
                )
            );
        }
        previous = row.id.clone();
        count += 1;
    }
    assert_eq!(count, 604);
    drop(chat);
    assert!(!scratch.exists());
    std::fs::OpenOptions::new()
        .append(true)
        .open(file.path())
        .unwrap()
        .write_all(b"{\"type\":")
        .unwrap();
    assert!(
        parse(file.path()).is_err(),
        "late malformed data must reject a spilled source"
    );
}
