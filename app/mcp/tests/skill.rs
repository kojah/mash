//! Validate shipped examples without executing tools, loading models or opening storage.
use super::super::*;
use clap::Parser;
use pulldown_cmark::{CodeBlockKind, Event, Parser as Markdown, Tag, TagEnd};
use serde::de::DeserializeOwned;
use serde_json::Value;

const HISTORY: &str = include_str!("../../../plugins/mash/skills/history/SKILL.md");

fn arguments<T: DeserializeOwned + schemars::JsonSchema>(text: &str) -> Result<T, String> {
    let value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let input: T = serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
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
    Ok(input)
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
        "search" => {
            let input = arguments::<Search>(text)?;
            core::validate_search_request(&Request {
                paths: Paths::default(),
                operation: input.operation(),
            })
            .map_err(|error| error.error.message)
        }
        "list_chats" => {
            let input = arguments::<ListChats>(text)?;
            core::validate_chat_request(&input.input, input.local, input.shared)
                .map_err(|error| error.error.message)
        }
        "view" => arguments::<View>(text).map(|_| ()),
        "list_records" => arguments::<crate::records::List>(text).map(|_| ()),
        "get_record" => arguments::<crate::records::Get>(text).map(|_| ()),
        "recall_records" => arguments::<crate::records::recall::Request>(text).map(|_| ()),
        "record" => arguments::<crate::records::Mutation>(text).map(|_| ()),
        "status" => {
            let input = arguments::<Status>(text)?;
            input.coverage.map_or(Ok(()), |coverage| {
                coverage.validate().map_err(|error| error.error.message)
            })
        }
        "list_attachments" => arguments::<ListAttachments>(text).map(|_| ()),
        "get_attachment" => arguments::<GetAttachment>(text).map(|_| ()),
        "help" => super::super::guidance::validate_help(&arguments(text)?),
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
fn history_skill_generation_is_current() {
    let (skill, stamp) = super::super::guidance::generate_history_skill().unwrap();
    assert_eq!(skill, HISTORY, "regenerate with make generate-skills");
    assert_eq!(
        stamp,
        include_str!("../../../plugins/mash/skills/history/contract.json")
    );
}

#[test]
fn history_skill_live_examples_match_core_validators() {
    let examples = super::super::guidance::examples().unwrap();
    assert!(examples.len() > 2);
    for example in examples {
        mcp(
            example.tool,
            &serde_json::to_string(&example.arguments).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn history_skill_rejects_parameter_enum_range_and_cli_drift() {
    // The thin skill delegates parameters to live help, whose tool name is validated.
    let broken = HISTORY.replace("\"tool\":\"search\"", "\"tool\":\"invented\"");
    assert!(validate(&broken).unwrap_err().contains("unknown tool"));
    assert!(mcp("view", r#"{"message_id":"message"}"#).is_err());
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
    assert!(mcp("search", r#"{"query":"test","offset":199,"limit":200}"#).is_ok());
    assert!(mcp("search", r#"{"query":"test","offset":200}"#).is_err());
    assert!(
        mcp(
            "search",
            r#"{"query":"test","unit":"messages","offset":49}"#
        )
        .is_ok()
    );
    assert!(
        mcp(
            "search",
            r#"{"query":"test","unit":"messages","offset":50}"#
        )
        .is_err()
    );
    assert!(mcp("search", r#"{"query":"test","limit":0}"#).is_err());
    assert!(mcp("list_chats", r#"{"title_contains":""}"#).is_err());
    assert!(mcp("list_chats", r#"{"title_contains":"\n"}"#).is_err());
    assert!(mcp("list_chats", r#"{"local":true,"shared":true}"#).is_err());
    assert!(mcp("search", r#"{"query":"test","local":true,"shared":true}"#).is_err());
    assert!(
        mcp(
            "search",
            r#"{"query":"test","intent":"exact-mention","ranking":"fusion"}"#
        )
        .is_err()
    );
    assert!(mcp("help", r#"{"tool":"list_threads"}"#).is_err());
    assert!(block("bash cli", "mash list-threads").is_err());
    assert!(block("bash cli", "mash search test --unit chat-messages").is_err());
    assert!(block("bash cli", "mash search test --unit messages --chat id").is_ok());
}
