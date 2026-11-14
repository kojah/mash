//! The bundled history skill's CLI examples must parse with the real CLI.
//! `mash-app` checks the same document's MCP examples.
use clap::Parser;
use pulldown_cmark::{CodeBlockKind, Event, Parser as Markdown, Tag, TagEnd};

const HISTORY: &str = include_str!("../../plugins/mash/skills/mash-history/SKILL.md");

fn parses(line: &str) -> Result<(), String> {
    let words = shlex::split(line).ok_or("invalid shell quoting")?;
    super::Cli::try_parse_from(words)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Every non-empty line of the document's `bash cli` fences.
fn examples(document: &str) -> Vec<String> {
    let mut inside = false;
    let mut lines = Vec::new();
    for event in Markdown::new(document) {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                inside = info.as_ref() == "bash cli";
            }
            Event::Text(part) if inside => lines.extend(
                part.lines()
                    .filter(|line| !line.trim().is_empty())
                    .map(str::to_owned),
            ),
            Event::End(TagEnd::CodeBlock) => inside = false,
            _ => {}
        }
    }
    lines
}

#[test]
fn history_skill_cli_examples_parse() {
    let mut checked = 0;
    for line in examples(HISTORY) {
        parses(&line).unwrap_or_else(|error| panic!("{line}: {error}"));
        checked += 1;
    }
    assert!(checked > 0, "the skill has no CLI examples to check");
}

#[test]
fn history_skill_rejects_cli_drift() {
    assert!(parses("mash view MESSAGE_ID --invented-option").is_err());
    assert!(parses("mash list-threads").is_err());
    assert!(parses("mash search test --unit chat-messages").is_err());
    assert!(parses("mash search test --unit messages --chat id").is_ok());
}
