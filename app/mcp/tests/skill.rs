//! Validate shipped examples without executing tools, loading models or opening storage.
use super::super::*;
use clap::Parser;
use pulldown_cmark::{CodeBlockKind, Event, Parser as Markdown, Tag, TagEnd};
use serde::de::DeserializeOwned;
use serde_json::Value;

const HISTORY: &str = include_str!("../../../plugins/mash/skills/history/SKILL.md");

fn arguments<T: DeserializeOwned + schemars::JsonSchema>(text: &str) -> Result<(), String> {
    let value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let _: T = serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
    // Serde checks actual runtime decoding; the generated schema also describes
    // top-level field names and bounds that are enforced later by the core.
    let schema = serde_json::to_value(schemars::schema_for!(T)).unwrap();
    let properties = schema["properties"]
        .as_object()
        .ok_or("missing properties")?;
    for (key, value) in value.as_object().ok_or("expected argument object")? {
        let property = properties
            .get(key)
            .ok_or_else(|| format!("unknown field {key}"))?;
        if let Some(number) = value.as_f64() {
            for (bound, lower) in [("minimum", true), ("maximum", false)] {
                if let Some(limit) = property[bound].as_f64()
                    && ((lower && number < limit) || (!lower && number > limit))
                {
                    return Err(format!("{key} violates {bound} {limit}"));
                }
            }
        }
    }
    Ok(())
}

fn mcp(tool: &str, text: &str) -> Result<(), String> {
    if !Mash::tool_router()
        .list_all()
        .iter()
        .any(|entry| entry.name == tool)
    {
        return Err(format!("unregistered tool {tool}"));
    }
    match tool {
        "search" => arguments::<Search>(text),
        "list_chats" => arguments::<ListChats>(text),
        "view" => arguments::<View>(text),
        "status" => arguments::<Status>(text),
        "list_attachments" => arguments::<ListAttachments>(text),
        "get_attachment" => arguments::<GetAttachment>(text),
        _ => Err(format!("add runtime argument validation for {tool}")),
    }
}

fn block(info: &str, text: &str) -> Result<(), String> {
    if let Some(tool) = info.strip_prefix("json mcp:") {
        mcp(tool, text)
    } else if info == "bash cli" {
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let words = shlex::split(line).ok_or("invalid shell quoting")?;
            if words.first().map(String::as_str) != Some("mash") {
                return Err("CLI examples must invoke mash".into());
            }
            crate::cli::Cli::try_parse_from(words).map_err(|error| error.to_string())?;
        }
        Ok(())
    } else {
        Err(format!(
            "unvalidated example fence {info:?}; use json mcp:TOOL or bash cli"
        ))
    }
}

fn validate(document: &str) -> Result<usize, String> {
    let mut current = None;
    let mut text = String::new();
    let mut count = 0;
    for event in Markdown::new(document) {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                let CodeBlockKind::Fenced(info) = kind else {
                    return Err("examples must use labeled fences".into());
                };
                current = Some(info.into_string());
                text.clear();
            }
            Event::Text(part) if current.is_some() => text.push_str(&part),
            Event::End(TagEnd::CodeBlock) => {
                let info = current.take().ok_or("missing example label")?;
                block(&info, &text).map_err(|error| format!("{info}: {error}"))?;
                count += 1;
            }
            _ => {}
        }
    }
    Ok(count)
}

#[test]
fn history_skill_examples_match_runtime_contracts() {
    assert!(validate(HISTORY).expect("history skill examples must match Rust contracts") > 0);
}

#[test]
fn history_skill_rejects_parameter_enum_range_and_cli_drift() {
    // Reproduce the real failure: attachment parameter names do not belong to view.
    let broken = HISTORY.replace(
        "\"id\":\"RETURNED_MESSAGE_ID\"",
        "\"message_id\":\"RETURNED_MESSAGE_ID\"",
    );
    assert!(validate(&broken).unwrap_err().contains("message_id"));
    assert!(mcp("view", r#"{"id":"message","shared":true}"#).is_err());
    assert!(mcp("search", r#"{"query":"test","intent":"invented"}"#).is_err());
    assert!(mcp("list_chats", r#"{"limit":101}"#).is_err());
    assert!(mcp("removed_tool", "{}").is_err());
    assert!(block("bash cli", "mash view MESSAGE_ID --invented-option").is_err());
    assert!(block("json", "{}").is_err());
    assert!(mcp("list_threads", "{}").is_err());
    assert!(mcp("list_chats", "{}").is_ok());
    assert!(mcp("search", r#"{"query":"test","unit":"threads"}"#).is_err());
    assert!(mcp("search", r#"{"query":"test","conversation":["id"]}"#).is_err());
    assert!(mcp("search", r#"{"query":"test","unit":"chats","chat":["id"]}"#).is_ok());
    assert!(block("bash cli", "mash list-threads").is_err());
    assert!(block("bash cli", "mash search test --unit chat-messages").is_err());
    assert!(block("bash cli", "mash search test --unit messages --chat id").is_ok());
}
