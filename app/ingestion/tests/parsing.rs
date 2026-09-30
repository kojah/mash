use super::*;

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
                crate::provider::Provider::Aider
                | crate::provider::Provider::Hermes
                | crate::provider::Provider::Opencode
                | crate::provider::Provider::Openclaw
                | crate::provider::Provider::Pi
                | crate::provider::Provider::Copilot
                | crate::provider::Provider::GeminiCli
                | crate::provider::Provider::Cline
                | crate::provider::Provider::Cursor
                | crate::provider::Provider::Windsurf => {
                    unreachable!("this fixture covers the JSONL providers")
                }
            };
            writeln!(file, "{value}").unwrap();
        }
        let parsed = match provider {
            crate::provider::Provider::Codex => parse_codex(file.path()),
            crate::provider::Provider::ClaudeCode => parse_claude(file.path()),
            crate::provider::Provider::Aider
            | crate::provider::Provider::Hermes
            | crate::provider::Provider::Opencode
            | crate::provider::Provider::Openclaw
            | crate::provider::Provider::Pi
            | crate::provider::Provider::Copilot
            | crate::provider::Provider::GeminiCli
            | crate::provider::Provider::Cline
            | crate::provider::Provider::Cursor
            | crate::provider::Provider::Windsurf => {
                unreachable!("this fixture covers the JSONL providers")
            }
        }
        .unwrap()
        .conversation
        .unwrap();
        assert_eq!(parsed.messages[0].content, context);
        assert_eq!(parsed.messages[0].search_projection.as_deref(), Some(""));
        assert_eq!(parsed.messages[1].content, mixed);
        assert_eq!(
            parsed.messages[1].search_projection.as_deref(),
            Some("fix cBioPortal importer")
        );
        assert_eq!(parsed.title.as_deref(), Some("fix cBioPortal importer"));
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
fn claude_parser_skips_malformed_lines_and_keeps_messages() {
    let mut file = tempfile::NamedTempFile::new().expect("temporary JSONL");
    writeln!(file, "not-json").expect("malformed line");
    writeln!(file, r#"{{"type":"user","sessionId":"s1","uuid":"u1","timestamp":"2026-01-01T00:00:00Z","message":{{"role":"user","content":"hello"}}}}"#).expect("user line");
    writeln!(file, r#"{{"type":"assistant","sessionId":"s1","uuid":"a1","message":{{"role":"assistant","content":[{{"type":"text","text":"world"}}]}}}}"#).expect("assistant line");

    let parsed = parse_claude(file.path()).expect("parse Claude history");
    let conversation = parsed.conversation.expect("conversation");
    assert_eq!(parsed.malformed_records, 1);
    assert_eq!(conversation.id, "s1");
    assert_eq!(conversation.messages.len(), 2);
    assert_eq!(conversation.messages[1].content, "world");
}

#[test]
fn claude_parser_separates_tool_results_from_searchable_text() {
    let mut file = tempfile::NamedTempFile::new().expect("temporary JSONL");
    writeln!(file, r#"{{"type":"user","sessionId":"s1","uuid":"mixed","message":{{"role":"user","content":[{{"type":"text","text":"keep this request"}},{{"type":"tool_result","content":"private tool payload"}}]}}}}"#).expect("mixed line");
    writeln!(file, r#"{{"type":"user","sessionId":"s1","uuid":"tool-only","message":{{"role":"user","content":[{{"type":"tool_result","content":"only tool payload"}}]}}}}"#).expect("tool-result line");

    let parsed = parse_claude(file.path()).expect("parse Claude history");
    let messages = parsed.conversation.expect("conversation").messages;
    assert_eq!(
        messages[0].content,
        "keep this request\nprivate tool payload"
    );
    assert_eq!(
        messages[0].search_projection.as_deref(),
        Some("keep this request")
    );
    assert_eq!(messages[1].content, "only tool payload");
    assert_eq!(messages[1].search_projection.as_deref(), Some(""));

    let mut object_file = tempfile::NamedTempFile::new().expect("temporary JSONL");
    writeln!(object_file, r#"{{"type":"user","sessionId":"s2","uuid":"object","message":{{"role":"user","content":{{"type":"tool_result","content":"object tool payload"}}}}}}"#).expect("object tool-result line");
    let object = parse_claude(object_file.path())
        .expect("parse object tool result")
        .conversation
        .expect("object conversation")
        .messages
        .remove(0);
    assert_eq!(object.content, "object tool payload");
    assert_eq!(object.search_projection.as_deref(), Some(""));
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
    assert!(parsed.messages[0].content.contains("internal reasoning"));
    assert!(parsed.messages[0].content.contains("privatecommandmarker"));
    assert_eq!(
        parsed.messages[0].search_projection.as_deref(),
        Some("I fixed the importer")
    );
    assert!(parsed.messages[1].content.contains("privatecommandmarker"));
    assert_eq!(parsed.messages[1].search_projection.as_deref(), Some(""));
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
    assert!(conversation.messages[1].content.contains("Tool search"));
    assert_eq!(
        conversation.messages[1].search_projection.as_deref(),
        Some("")
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
    assert_eq!(conversation.messages[0].content, "mirrored turn");
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
        conversation.messages[0].content,
        "Tool imagegen: draw a diagram"
    );
    assert_eq!(conversation.messages[1].role, crate::role::Role::Tool);
    assert_eq!(
        conversation.messages[0].search_projection.as_deref(),
        Some("")
    );
    assert_eq!(conversation.messages[1].content, "created diagram.png");
    assert_eq!(
        conversation.messages[1].search_projection.as_deref(),
        Some("")
    );
}
