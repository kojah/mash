//! Structured tool activity from provider-format fixtures.
use super::*;
use crate::tools::Activity;
use serde_json::json;

fn parsed(provider: Provider, fixture: &str) -> Vec<NormalizedMessage> {
    let file = tempfile::NamedTempFile::new().unwrap();
    fs::write(file.path(), fixture).unwrap();
    let parsed = match provider {
        Provider::ClaudeCode => parse_claude(file.path()),
        _ => parse_codex(file.path()),
    }
    .unwrap();
    parsed
        .conversation
        .unwrap()
        .messages
        .into_rows()
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn ids(activity: &Activity) -> (Vec<Option<&str>>, Vec<Option<&str>>) {
    (
        activity
            .tool_calls
            .iter()
            .map(|call| call.call_id.as_deref())
            .collect(),
        activity
            .tool_results
            .iter()
            .map(|result| result.call_id.as_deref())
            .collect(),
    )
}

#[test]
fn claude_records_parallel_calls_and_linked_results_with_error_flags() {
    let messages = parsed(
        Provider::ClaudeCode,
        include_str!("../../../fixtures/claude-native-tools-2.1.296.jsonl"),
    );
    let by_text = |text: &str| {
        messages
            .iter()
            .find(|message| message.content.contains(text))
            .unwrap()
    };
    let call = by_text("Checking both.");
    assert_eq!(call.dialogue_text.as_deref(), Some("Checking both."));
    let calls = &call.tools.tool_calls;
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].name.as_deref(), Some("Bash"));
    assert_eq!(calls[0].call_id.as_deref(), Some("toolu_01A"));
    assert_eq!(
        calls[0].input,
        Some(json!({"command": "ls fixtures", "description": "List fixtures"}))
    );
    assert_eq!(calls[1].name.as_deref(), Some("mcp__mash__search"));
    assert_eq!(
        calls[1].input,
        Some(json!({"query": "importer", "limit": 3}))
    );
    // The flattened canonical text is unchanged.
    assert!(
        call.content
            .contains(r#"Tool Bash: {"command":"ls fixtures""#)
    );

    let results = by_text("claude.jsonl");
    assert_eq!(results.dialogue_text.as_deref(), Some(""));
    assert_eq!(
        ids(&results.tools),
        (vec![], vec![Some("toolu_01A"), Some("toolu_01B")])
    );
    assert_eq!(
        results
            .tools
            .tool_results
            .iter()
            .map(|result| result.is_error)
            .collect::<Vec<_>>(),
        [Some(false), Some(true)]
    );
    let legacy = by_text("legacy result");
    assert_eq!(legacy.tools.tool_results[0].is_error, None);
    assert!(by_text("index is unavailable.").tools.is_empty());
    assert!(by_text("list the fixtures").tools.is_empty());
}

#[test]
fn codex_records_calls_and_results_linked_by_call_id() {
    let messages = parsed(
        Provider::Codex,
        include_str!("../../../fixtures/codex-native-tools-0.160.1.jsonl"),
    );
    let activity = messages
        .iter()
        .map(|message| (message.role.as_str(), &message.tools))
        .collect::<Vec<_>>();
    assert_eq!(activity.len(), 6);
    let exec = &activity[1].1.tool_calls[0];
    assert_eq!(activity[1].0, "assistant");
    assert_eq!(exec.name.as_deref(), Some("exec"));
    assert_eq!(exec.call_id.as_deref(), Some("call_exec"));
    assert_eq!(exec.input, Some(json!("rg -n importer app")));
    assert_eq!(activity[2].0, "tool");
    assert_eq!(ids(activity[2].1), (vec![], vec![Some("call_exec")]));
    assert_eq!(activity[2].1.tool_results[0].is_error, None);
    let send = &activity[3].1.tool_calls[0];
    assert_eq!(send.name.as_deref(), Some("send_message"));
    // Encoded arguments stay the provider's verbatim string.
    assert_eq!(
        send.input,
        Some(json!(r#"{"to":"reviewer","text":"patch ready"}"#))
    );
    assert_eq!(ids(activity[4].1), (vec![], vec![Some("call_send")]));
    assert!(activity[0].1.is_empty() && activity[5].1.is_empty());
    assert_eq!(
        messages[3].content,
        r#"Tool send_message: {"to":"reviewer","text":"patch ready"}"#
    );
}

#[test]
fn unknown_blocks_keep_context_without_inventing_activity() {
    let messages = parsed(
        Provider::ClaudeCode,
        concat!(
            r#"{"type":"assistant","sessionId":"s","uuid":"a","message":{"role":"assistant","content":["#,
            r#"{"type":"server_tool_use","id":"srv_1","name":"web_search","input":{"q":"x"}},"#,
            r#"{"type":"tool_use","id":"toolu_9","name":"Read","input":null}]}}"#,
            "\n"
        ),
    );
    let calls = &messages[0].tools.tool_calls;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].call_id.as_deref(), Some("toolu_9"));
    // A present JSON null is the provider's input, distinct from an absent one.
    assert_eq!(calls[0].input, Some(serde_json::Value::Null));
}

#[test]
fn reconciled_claude_copies_keep_activity_from_a_newer_interpretation() {
    let message = |tools: Activity| NormalizedMessage {
        attachments: Vec::new(),
        id: "shared".into(),
        ordinal: 0,
        context: crate::message_context::MessageContext::Root,
        role: crate::role::Role::Assistant,
        content: "Tool Bash: {}".into(),
        dialogue_text: Some(String::new()),
        created_at: Some(1),
        tools,
    };
    let current = Activity::call(Some("Bash"), Some("toolu_1"), Some(&json!({})));
    for order in [
        [Activity::default(), current.clone()],
        [current.clone(), Activity::default()],
    ] {
        let (messages, _) =
            claude::reconcile(order.into_iter().map(|tools| Ok((message(tools), None)))).unwrap();
        assert_eq!(messages.inline()[0].tools, current);
    }
}
