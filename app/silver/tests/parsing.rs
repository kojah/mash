use super::*;

#[test]
fn rejected_jsonl_records_report_physical_positions_without_content() {
    use crate::silver::diagnostics::Category;
    let canary = "PRIVATE-RECORD-CONTENT-CANARY";
    for provider in [Provider::ClaudeCode, Provider::Codex] {
        for (record, category) in [
            (
                format!(r#"{{"private":"é{canary}","type":}}"#),
                Category::Syntax,
            ),
            (format!(r#"{{"sessionId":["{canary}"]}}"#), Category::Data),
            (
                format!(r#"{{"private":"{canary}","#),
                Category::UnexpectedEof,
            ),
        ] {
            let source = tempfile::NamedTempFile::new().unwrap();
            let input = format!("\r\n{{\"type\":\"system\"}}\r\n\n{record}\r\nnot-json\n");
            fs::write(source.path(), &input).unwrap();
            let parsed = match provider {
                Provider::ClaudeCode => parse_claude(source.path()),
                Provider::Codex => parse_codex(source.path()),
                _ => unreachable!(),
            }
            .unwrap();
            assert_eq!(parsed.malformed_records, 2);
            let issue = parsed.first_record_issue.unwrap();
            assert_eq!(issue.provider, provider);
            assert_eq!(issue.source, source.path());
            assert_eq!(issue.line, 4);
            assert_eq!(issue.category, category);
            if category == Category::Syntax {
                assert_eq!(issue.column, record.len(), "column counts UTF-8 bytes");
            }
            let serialized = serde_json::to_string(&issue).unwrap();
            assert!(!serialized.contains(canary));
            assert!(!issue.to_string().contains(canary));
            assert_eq!(fs::read_to_string(source.path()).unwrap(), input);
        }
    }
}

#[test]
fn both_parsers_keep_canonical_context_but_search_only_real_requests() {
    let context = "# AGENTS.md instructions\n<INSTRUCTIONS>cbioportal rules</INSTRUCTIONS><environment_context>cwd</environment_context>";
    let mixed = "<system-reminder>skill text</system-reminder>fix cBioPortal importer";
    for provider in [
        crate::provider::Provider::Codex,
        crate::provider::Provider::ClaudeCode,
    ] {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for (id, text) in [("context", context), ("request", mixed)] {
            let value = match provider {
                crate::provider::Provider::Codex => {
                    serde_json::json!({"type":"response_item","payload":{"type":"message","id":id,"role":"user","content":[{"type":"input_text","text":text}]}})
                }
                crate::provider::Provider::ClaudeCode => {
                    serde_json::json!({"type":"user","sessionId":"chat","uuid":id,"message":{"role":"user","content":text}})
                }
                crate::provider::Provider::Opencode
                | crate::provider::Provider::Copilot
                | crate::provider::Provider::Cursor
                | crate::provider::Provider::Antigravity => {
                    unreachable!("this fixture covers the JSONL providers")
                }
            };
            writeln!(file, "{value}").unwrap();
        }
        let parsed = match provider {
            crate::provider::Provider::Codex => parse_codex(file.path()),
            crate::provider::Provider::ClaudeCode => parse_claude(file.path()),
            crate::provider::Provider::Opencode
            | crate::provider::Provider::Copilot
            | crate::provider::Provider::Cursor
            | crate::provider::Provider::Antigravity => {
                unreachable!("this fixture covers the JSONL providers")
            }
        }
        .unwrap()
        .conversation
        .unwrap();
        assert_eq!(parsed.messages.inline()[0].content, context);
        assert_eq!(
            parsed.messages.inline()[0].dialogue_text.as_deref(),
            Some("")
        );
        assert_eq!(parsed.messages.inline()[1].content, mixed);
        assert_eq!(
            parsed.messages.inline()[1].dialogue_text.as_deref(),
            Some("fix cBioPortal importer")
        );
        assert_eq!(parsed.title.as_deref(), Some("fix cBioPortal importer"));
    }
}

#[test]
fn malformed_jsonl_cannot_be_masked_by_fragment_ancestry_or_identity_errors() {
    let source = tempfile::NamedTempFile::new().unwrap();
    let prefix = r#"{"type":"user","uuid":"root","sessionId":"original","message":{"content":"original request"}}"#;
    for suffix in [
        r#"{"type":"user","uuid":"child","sessionId":"original","parentUuid":"missing-parent","message":{"content":"orphan fragment"}}"#,
        r#"{"type":"user","uuid":"child","sessionId":"different","message":{"content":"different session fragment"}}"#,
        r#"{"type":"user","uuid":"root","sessionId":"original","parentUuid":"conflicting-parent","message":{"content":"conflicting replay fragment"}}"#,
    ] {
        let input = format!("{prefix}\n{{broken\n{suffix}\n{{\"sessionId\":[]}}\n");
        fs::write(source.path(), &input).unwrap();
        let parsed = parse_claude(source.path()).unwrap();
        assert!(
            parsed.conversation.is_none(),
            "an incomplete source exposes no normalized fragments"
        );
        assert_eq!(
            parsed.malformed_records, 2,
            "later decode failures are still counted"
        );
        let issue = parsed.first_record_issue.unwrap();
        assert_eq!(issue.line, 2);
        assert_eq!(issue.category, diagnostics::Category::Syntax);
        assert_eq!(fs::read_to_string(source.path()).unwrap(), input);
    }
}

#[test]
fn session_metadata_keeps_first_project_and_logged_repository() {
    let mut project = None;
    let mut repository = None;
    capture_metadata(
        &serde_json::from_value(serde_json::json!({"type":"session_meta","payload":{"cwd":"/work/project","git":{"repository_url":"https://example.test/team/repo.git"}}})).unwrap(),
        &mut project,
        &mut repository,
    );
    capture_metadata(
        &serde_json::from_value(
            serde_json::json!({"cwd":"/work/later","git":{"repository_url":"different"}}),
        )
        .unwrap(),
        &mut project,
        &mut repository,
    );
    assert_eq!(project.as_deref(), Some("/work/project"));
    assert_eq!(
        repository.as_deref(),
        Some("https://example.test/team/repo.git")
    );
    let mut project = None;
    let mut repository = None;
    capture_metadata(
        &serde_json::from_value(serde_json::json!({"cwd":"/work/claude"})).unwrap(),
        &mut project,
        &mut repository,
    );
    assert_eq!(project.as_deref(), Some("/work/claude"));
    assert_eq!(repository, None);
}

#[test]
fn claude_agent_identity_uses_native_session_and_agent_not_file_location() {
    let directory = tempfile::tempdir().unwrap();
    let record = r#"{"type":"user","sessionId":"parent","agentId":"alpha","uuid":"u1","message":{"role":"user","content":"agent sentinel"}}"#;
    let first = directory.path().join("agent-alpha.jsonl");
    let copied = directory.path().join("moved.jsonl");
    fs::write(&first, record).unwrap();
    fs::write(&copied, record).unwrap();
    let original = parse_claude(&first).unwrap().conversation.unwrap();
    let moved = parse_claude(&copied).unwrap().conversation.unwrap();
    assert_eq!(original.id, moved.id);
    assert_ne!(original.id, "parent");
    fs::write(
        &first,
        format!("{record}\n{}", record.replace("alpha", "beta")),
    )
    .unwrap();
    assert!(
        parse_claude(&first)
            .err()
            .unwrap()
            .error
            .message
            .contains("multiple agent identities")
    );
}

#[test]
fn claude_workflow_events_require_agent_consistency_only_when_dialogue_exists() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("journal.jsonl");
    let events = r#"{"type":"started","agentId":"alpha"}
{"type":"result","agentId":"beta"}"#;
    fs::write(&path, events).unwrap();
    let empty = parse_claude(&path).unwrap();
    assert!(empty.conversation.is_none());
    assert_eq!(empty.malformed_records, 0);

    let message = r#"{"type":"user","sessionId":"chat","agentId":"dialogue-agent","uuid":"message","parentUuid":null,"message":{"role":"user","content":"retained dialogue"}}"#;
    fs::write(&path, message).unwrap();
    let plain = parse_claude(&path).unwrap().conversation.unwrap();
    fs::write(&path, format!("{events}\n{message}\n{events}")).unwrap();
    assert!(
        parse_claude(&path)
            .err()
            .unwrap()
            .error
            .message
            .contains("multiple agent identities")
    );
    let consistent = r#"{"type":"started","agentId":"dialogue-agent"}"#;
    let metadata_identified = message.replace("\"agentId\":\"dialogue-agent\",", "");
    fs::write(&path, format!("{consistent}\n{metadata_identified}")).unwrap();
    let mixed = parse_claude(&path).unwrap().conversation.unwrap();
    assert_eq!(plain.id, mixed.id);
    assert_eq!(plain.messages.inline(), mixed.messages.inline());

    fs::write(&path, format!("{events}\n{{broken")).unwrap();
    let malformed = parse_claude(&path).unwrap();
    assert!(malformed.conversation.is_none());
    assert_eq!(malformed.malformed_records, 1);
}

#[test]
fn claude_parser_reports_malformed_lines_without_normalized_fragments() {
    let mut file = tempfile::NamedTempFile::new().expect("temporary JSONL");
    writeln!(file, "not-json").expect("malformed line");
    writeln!(file, r#"{{"type":"user","sessionId":"s1","uuid":"u1","timestamp":"2026-01-01T00:00:00Z","message":{{"role":"user","content":"hello"}}}}"#).expect("user line");
    writeln!(file, r#"{{"type":"assistant","sessionId":"s1","uuid":"a1","message":{{"role":"assistant","content":[{{"type":"text","text":"world"}}]}}}}"#).expect("assistant line");

    let parsed = parse_claude(file.path()).expect("parse Claude history");
    assert!(parsed.conversation.is_none());
    assert_eq!(parsed.malformed_records, 1);
    assert_eq!(parsed.first_record_issue.unwrap().line, 1);
}

#[test]
fn claude_parser_separates_tool_results_from_searchable_text() {
    let mut file = tempfile::NamedTempFile::new().expect("temporary JSONL");
    writeln!(file, r#"{{"type":"user","sessionId":"s1","uuid":"mixed","message":{{"role":"user","content":[{{"type":"text","text":"keep this request"}},{{"type":"tool_result","content":"private tool payload"}}]}}}}"#).expect("mixed line");
    writeln!(file, r#"{{"type":"user","sessionId":"s1","uuid":"tool-only","message":{{"role":"user","content":[{{"type":"tool_result","content":"only tool payload"}}]}}}}"#).expect("tool-result line");

    let parsed = parse_claude(file.path()).expect("parse Claude history");
    let messages = parsed.conversation.expect("conversation").messages;
    assert_eq!(
        messages.inline()[0].content,
        "keep this request\nprivate tool payload"
    );
    assert_eq!(
        messages.inline()[0].dialogue_text.as_deref(),
        Some("keep this request")
    );
    assert_eq!(messages.inline()[1].content, "only tool payload");
    assert_eq!(messages.inline()[1].dialogue_text.as_deref(), Some(""));

    let mut object_file = tempfile::NamedTempFile::new().expect("temporary JSONL");
    writeln!(object_file, r#"{{"type":"user","sessionId":"s2","uuid":"object","message":{{"role":"user","content":{{"type":"tool_result","content":"object tool payload"}}}}}}"#).expect("object tool-result line");
    let object = parse_claude(object_file.path())
        .expect("parse object tool result")
        .conversation
        .expect("object conversation")
        .messages
        .inline_mut()
        .remove(0);
    assert_eq!(object.content, "object tool payload");
    assert_eq!(object.dialogue_text.as_deref(), Some(""));
}

#[test]
fn claude_keeps_reasoning_and_tool_arguments_only_in_canonical_context() {
    let file = tempfile::NamedTempFile::new().unwrap();
    fs::write(file.path(), concat!(
        "{\"type\":\"assistant\",\"sessionId\":\"tools\",\"uuid\":\"mixed\",\"message\":{\"role\":\"assistant\",\"content\":[",
        "{\"type\":\"text\",\"text\":\"I fixed the importer\"},",
        "{\"type\":\"thinking\",\"thinking\":\"internal reasoning\"},",
        "{\"type\":\"tool_use\",\"name\":\"shell\",\"input\":{\"command\":\"privatecommandmarker\"}}]}}\n",
        "{\"type\":\"assistant\",\"sessionId\":\"tools\",\"uuid\":\"call\",\"message\":{\"role\":\"assistant\",\"content\":{\"type\":\"tool_use\",\"name\":\"shell\",\"input\":\"privatecommandmarker\"}}}\n",
    )).unwrap();
    let parsed = parse_claude(file.path()).unwrap().conversation.unwrap();
    assert!(
        parsed.messages.inline()[0]
            .content
            .contains("internal reasoning")
    );
    assert!(
        parsed.messages.inline()[0]
            .content
            .contains("privatecommandmarker")
    );
    assert_eq!(
        parsed.messages.inline()[0].dialogue_text.as_deref(),
        Some("I fixed the importer")
    );
    assert!(
        parsed.messages.inline()[1]
            .content
            .contains("privatecommandmarker")
    );
    assert_eq!(
        parsed.messages.inline()[1].dialogue_text.as_deref(),
        Some("")
    );
}

#[test]
fn codex_parser_keeps_messages_and_tool_calls() {
    let mut file = tempfile::NamedTempFile::new().expect("temporary JSONL");
    writeln!(file, r#"{{"type":"session_meta","payload":{{"id":"c1"}}}}"#).expect("meta line");
    writeln!(file, r#"{{"type":"response_item","payload":{{"type":"message","id":"m1","role":"user","content":[{{"type":"input_text","text":"find it"}}]}}}}"#).expect("message line");
    writeln!(file, r#"{{"type":"response_item","payload":{{"type":"function_call","call_id":"call1","name":"search","arguments":"{{\"q\":\"needle\"}}"}}}}"#).expect("tool line");

    let parsed = parse_codex(file.path()).expect("parse Codex history");
    let conversation = parsed.conversation.expect("conversation");
    assert_eq!(conversation.id, "c1");
    assert_eq!(conversation.messages.len(), 2);
    assert!(
        conversation.messages.inline()[1]
            .content
            .contains("Tool search")
    );
    assert_eq!(
        conversation.messages.inline()[1].dialogue_text.as_deref(),
        Some("")
    );
}

#[test]
fn codex_native_migration_preserves_repeated_requests_without_mirrors() {
    // Output of unmodified codex-cli 0.159.2 migrate-rollouts on synthetic history.
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(include_bytes!(
        "../../../docs/fixtures/codex-native-migration-0.159.2.jsonl"
    ))
    .unwrap();
    let parsed = parse_codex(file.path()).unwrap();
    assert_eq!(parsed.malformed_records, 0);
    let conversation = parsed.conversation.unwrap();
    assert_eq!(conversation.id, "11111111-1111-4111-8111-111111111111");
    assert_eq!(conversation.project.as_deref(), Some("/synthetic/mash"));
    assert_eq!(
        conversation.title.as_deref(),
        Some("find cbioportal conversations")
    );
    assert_eq!(conversation.messages.len(), 4);
    for (index, message) in conversation.messages.inline().iter().enumerate() {
        let ordinal = i64::try_from(index).unwrap();
        assert_eq!(message.ordinal, ordinal);
        assert_eq!(
            message.role,
            if index % 2 == 0 {
                crate::role::Role::User
            } else {
                crate::role::Role::Assistant
            }
        );
        assert_eq!(
            message.created_at,
            Some(1_790_762_401_000 + 2_000 * ordinal)
        );
    }
    let messages = &conversation.messages;
    assert_eq!(
        messages.inline()[0].content,
        "find cbioportal conversations"
    );
    assert_eq!(messages.inline()[0].content, messages.inline()[2].content);
    assert_ne!(messages.inline()[0].id, messages.inline()[2].id);
    assert_eq!(messages.inline()[1].content, "Here is answer 0.");
    assert_eq!(messages.inline()[3].content, "Here is answer 1.");
    assert_eq!(
        messages.inline()[1].id,
        stable_id(
            "message",
            &["codex", &file.path().to_string_lossy(), "answer-0"]
        )
    );
    // Rereading must retain identity even though mirrored lifecycle IDs differ.
    let reread = parse_codex(file.path()).unwrap().conversation.unwrap();
    assert!(
        messages
            .inline()
            .iter()
            .map(|m| &m.id)
            .eq(reread.messages.inline().iter().map(|m| &m.id))
    );
}

#[test]
fn codex_native_completed_only_dialogue_is_recovered() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(include_bytes!(
        "../../../docs/fixtures/codex-native-lifecycle-only-0.159.2.jsonl"
    ))
    .unwrap();
    let parsed = parse_codex(file.path()).unwrap();
    assert_eq!(parsed.malformed_records, 0);
    let conversation = parsed.conversation.unwrap();
    assert_eq!(conversation.id, "22222222-2222-4222-8222-222222222222");
    assert_eq!(conversation.messages.len(), 4);
    assert_eq!(
        conversation.messages.inline()[0].content,
        "find cbioportal conversations"
    );
    assert_eq!(
        conversation.messages.inline()[2].content,
        conversation.messages.inline()[0].content
    );
    assert_ne!(
        conversation.messages.inline()[0].id,
        conversation.messages.inline()[2].id
    );
    assert_eq!(
        conversation.messages.inline()[1].content,
        "Here is answer 0."
    );
    assert_eq!(
        conversation.messages.inline()[3].content,
        "Here is answer 1."
    );
    assert_eq!(
        conversation.messages.inline()[0].created_at,
        Some(1_790_762_402_000)
    );
    assert_eq!(
        conversation.messages.inline()[1].role,
        crate::role::Role::Assistant
    );
}

#[test]
fn codex_completed_dialogue_does_not_promote_transient_or_unknown_items() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(include_bytes!(
        "../../../docs/fixtures/codex-native-lifecycle-only-0.159.2.jsonl"
    ))
    .unwrap();
    for line in [
        r#"{"type":"event_msg","payload":{"type":"item_started","turn_id":"transient","item":{"type":"AgentMessage","id":"started","content":[{"type":"Text","text":"unfinished answer"}]}}}"#,
        r#"{"type":"event_msg","payload":{"type":"agent_message_content_delta","item_id":"started","delta":"streaming fragment"}}"#,
        r#"{"type":"event_msg","payload":{"type":"item_completed","turn_id":"unknown","item":{"type":"FutureItem","id":"future","content":[{"type":"Text","text":"opaque future data"}]}}}"#,
        r#"{"type":"event_msg","payload":{"type":"item_completed","turn_id":"empty","item":{"type":"UserMessage","id":"image-only","content":[{"type":"local_image","path":"/synthetic/image.png"}]}}}"#,
    ] {
        writeln!(file, "{line}").unwrap();
    }
    let parsed = parse_codex(file.path()).unwrap();
    assert_eq!(parsed.malformed_records, 0);
    let conversation = parsed.conversation.unwrap();
    assert_eq!(conversation.messages.len(), 5);
    let image = &conversation.messages.inline()[4];
    assert_eq!(image.dialogue_text.as_deref(), Some(""));
    assert_eq!(image.attachments.len(), 1);
    assert!(
        matches!(&image.attachments[0], crate::attachments::Source::File {path,..} if path == Path::new("/synthetic/image.png"))
    );
}

#[test]
fn codex_partial_mirrors_keep_unmatched_dialogue_in_both_orders() {
    // Remove different raw records from the actual migrated fixture, then swap
    // remaining adjacent response/completed pairs to exercise reverse arrival.
    let fixture = include_str!("../../../docs/fixtures/codex-native-migration-0.159.2.jsonl");
    for reverse in [false, true] {
        let mut lines = fixture.lines().collect::<Vec<_>>();
        lines.remove(10); // second assistant raw response
        lines.remove(1); // first user raw response
        if reverse {
            lines.swap(3, 4); // first assistant completed/raw
            // second user's raw is before the next task boundary; leave it there.
        }
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{}", lines.join("\n")).unwrap();
        let conversation = parse_codex(file.path()).unwrap().conversation.unwrap();
        assert_eq!(conversation.messages.len(), 4, "reverse={reverse}");
        assert_eq!(
            conversation.messages.inline()[0].content,
            "find cbioportal conversations"
        );
        assert_eq!(
            conversation.messages.inline()[1].content,
            "Here is answer 0."
        );
        assert_eq!(
            conversation.messages.inline()[2].content,
            "find cbioportal conversations"
        );
        assert_eq!(
            conversation.messages.inline()[3].content,
            "Here is answer 1."
        );
    }
}

#[test]
fn codex_turn_boundaries_do_not_merge_separate_identical_requests() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(
        file,
        r#"{{"type":"event_msg","payload":{{"type":"task_started","turn_id":"old"}}}}"#
    )
    .unwrap();
    writeln!(file, r#"{{"type":"response_item","payload":{{"type":"message","id":"raw-old","role":"user","content":[{{"type":"input_text","text":"same request"}}]}}}}"#).unwrap();
    writeln!(
        file,
        r#"{{"type":"event_msg","payload":{{"type":"task_complete","turn_id":"old"}}}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"type":"event_msg","payload":{{"type":"task_started","turn_id":"new"}}}}"#
    )
    .unwrap();
    writeln!(file, r#"{{"type":"event_msg","payload":{{"type":"item_completed","turn_id":"new","item":{{"type":"UserMessage","id":"completed-new","content":[{{"type":"text","text":"same request"}}]}}}}}}"#).unwrap();
    let conversation = parse_codex(file.path()).unwrap().conversation.unwrap();
    assert_eq!(conversation.messages.len(), 2);
    assert_eq!(
        conversation.messages.inline()[0].content,
        conversation.messages.inline()[1].content
    );
    assert_ne!(
        conversation.messages.inline()[0].id,
        conversation.messages.inline()[1].id
    );
}

#[test]
fn codex_parser_keeps_the_rollout_session_identity() {
    let mut file = tempfile::NamedTempFile::new().expect("temporary JSONL");
    writeln!(
        file,
        r#"{{"type":"session_meta","payload":{{"id":"rollout-session"}}}}"#
    )
    .expect("rollout metadata");
    writeln!(file, r#"{{"type":"response_item","payload":{{"type":"message","id":"m1","role":"user","content":[{{"type":"input_text","text":"original turn"}}]}}}}"#)
        .expect("message line");
    writeln!(
        file,
        r#"{{"type":"session_meta","payload":{{"id":"nested-session"}}}}"#
    )
    .expect("nested metadata");

    let parsed = parse_codex(file.path()).expect("parse Codex history");
    let conversation = parsed.conversation.expect("conversation");
    assert_eq!(conversation.id, "rollout-session");
}

#[test]
fn codex_parser_ignores_mirrored_event_messages() {
    let mut file = tempfile::NamedTempFile::new().expect("temporary JSONL");
    writeln!(
        file,
        r#"{{"type":"session_meta","payload":{{"id":"c-mirror"}}}}"#
    )
    .expect("meta line");
    writeln!(file, r#"{{"type":"response_item","payload":{{"type":"message","id":"m1","role":"user","content":[{{"type":"input_text","text":"mirrored turn"}}]}}}}"#)
        .expect("response item");
    writeln!(
        file,
        r#"{{"type":"event_msg","payload":{{"type":"user_message","message":"mirrored turn"}}}}"#
    )
    .expect("mirrored event");

    let parsed = parse_codex(file.path()).expect("parse Codex history");
    let conversation = parsed.conversation.expect("conversation");
    assert_eq!(conversation.messages.len(), 1);
    assert_eq!(conversation.messages.inline()[0].content, "mirrored turn");
}

#[test]
fn codex_parser_keeps_custom_tool_calls() {
    let mut file = tempfile::NamedTempFile::new().expect("temporary JSONL");
    writeln!(file, r#"{{"type":"session_meta","payload":{{"id":"c2"}}}}"#).expect("meta line");
    writeln!(
        file,
        r#"{{"type":"response_item","payload":{{"type":"custom_tool_call","id":"tool1","call_id":"call1","name":"imagegen","input":"draw a diagram"}}}}"#
    )
    .expect("custom tool line");
    writeln!(
        file,
        r#"{{"type":"response_item","payload":{{"type":"custom_tool_call_output","id":"tool2","call_id":"call1","output":"created diagram.png"}}}}"#
    )
    .expect("custom tool output line");

    let parsed = parse_codex(file.path()).expect("parse Codex history");
    let conversation = parsed.conversation.expect("conversation");
    assert_eq!(conversation.id, "c2");
    assert_eq!(conversation.messages.len(), 2);
    assert_eq!(
        conversation.messages.inline()[0].content,
        "Tool imagegen: draw a diagram"
    );
    assert_eq!(
        conversation.messages.inline()[1].role,
        crate::role::Role::Tool
    );
    assert_eq!(
        conversation.messages.inline()[0].dialogue_text.as_deref(),
        Some("")
    );
    assert_eq!(
        conversation.messages.inline()[1].content,
        "created diagram.png"
    );
    assert_eq!(
        conversation.messages.inline()[1].dialogue_text.as_deref(),
        Some("")
    );
}
