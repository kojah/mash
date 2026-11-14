//! The published MCP tool reference (`docs/tools.md`), rendered from the same
//! tool definitions a connected agent receives.

use super::Mash;
use serde_json::{Map, Value};
use std::fmt::Write as _;

/// Every tool, grouped by what a reader wants to do. A tool missing from these
/// groups fails generation, so a new tool cannot go undocumented.
const GROUPS: &[(&str, &[&str])] = &[
    (
        "Search and read chats",
        &[
            "search",
            "list_chats",
            "view",
            "list_attachments",
            "get_attachment",
            "query_silver",
        ],
    ),
    (
        "Save and recall records",
        &[
            "record",
            "recall_records",
            "list_records",
            "get_record",
            "find_record_matches",
            "get_record_capture",
            "extract_records",
            "get_extraction_checkpoint",
        ],
    ),
    (
        "Set up and maintain",
        &[
            "status",
            "help",
            "index",
            "models_install",
            "forget",
            "export_silver",
            "import_silver",
        ],
    ),
];

const INTRODUCTION: &str = "\
# MCP tool reference

<!-- Generated from Mash's tool definitions by `make generate-docs`; do not edit. -->

These are the tools an agent sees once [Mash is connected](agents.md), or that
[another MCP client](mcp.md) can call. You rarely call them yourself: ask your
agent in plain words and it picks the tool. Open a tool to see what it does
and accepts; the descriptions are the ones your agent reads.

**READ** tools never change your index. **WRITE** tools do. Nothing runs from
this page: the dashboard's tool explorer runs the read tools against your own
history.
";

/// Text safe inside the page's HTML, on one line, with `backticked` words
/// as code.
fn escape(text: &str) -> String {
    let text = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\n', " ");
    // An unpaired backtick stays as written.
    if !text.matches('`').count().is_multiple_of(2) {
        return text;
    }
    let mut html = String::with_capacity(text.len());
    for (index, part) in text.split('`').enumerate() {
        if index.is_multiple_of(2) {
            html.push_str(part);
        } else {
            let _ = write!(html, "<code>{part}</code>");
        }
    }
    html
}

fn code(values: &[String]) -> String {
    values
        .iter()
        .map(|value| format!("<code>{}</code>", escape(value)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A description as sentences: the first heads a tool's row, the rest are
/// its points. Splits where a sentence ends before a capital letter.
fn sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character.is_whitespace()
            && current.ends_with(['.', '!', '?'])
            && characters.peek().is_some_and(|next| next.is_uppercase())
        {
            sentences.push(std::mem::take(&mut current));
        } else {
            current.push(character);
        }
    }
    if !current.is_empty() {
        sentences.push(current);
    }
    sentences
}

/// The names a string-valued definition accepts, from `enum` or `const` alternatives.
fn choices(schema: &Map<String, Value>) -> Option<Vec<String>> {
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        return Some(
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        );
    }
    let alternatives = schema.get("oneOf").and_then(Value::as_array)?;
    alternatives
        .iter()
        .map(|alternative| {
            alternative
                .get("const")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

fn resolve<'a>(
    schema: &'a Map<String, Value>,
    definitions: Option<&'a Map<String, Value>>,
) -> (&'a Map<String, Value>, Option<&'a str>) {
    let named = schema
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| reference.strip_prefix("#/$defs/"))
        .and_then(|name| Some((definitions?.get(name)?.as_object()?, name)));
    match named {
        Some((definition, name)) => (definition, Some(name)),
        None => (schema, None),
    }
}

/// What a parameter accepts, in words a reader can act on.
fn kind(schema: &Map<String, Value>, definitions: Option<&Map<String, Value>>) -> String {
    // An optional value is "this or null"; describe the value.
    if let Some(alternatives) = schema.get("anyOf").and_then(Value::as_array) {
        let present: Vec<&Map<String, Value>> = alternatives
            .iter()
            .filter_map(Value::as_object)
            .filter(|alternative| alternative.get("type").and_then(Value::as_str) != Some("null"))
            .collect();
        if let [only] = present.as_slice() {
            return kind(only, definitions);
        }
    }
    let (schema, name) = resolve(schema, definitions);
    if let Some(values) = choices(schema) {
        return format!("one of {}", code(&values));
    }
    let types: Vec<&str> = match schema.get("type") {
        Some(Value::String(one)) => vec![one.as_str()],
        Some(Value::Array(several)) => several
            .iter()
            .filter_map(Value::as_str)
            .filter(|one| *one != "null")
            .collect(),
        _ => vec![],
    };
    match types.as_slice() {
        ["array"] => {
            let items = schema
                .get("items")
                .and_then(Value::as_object)
                .map_or_else(|| "values".to_owned(), |items| kind(items, definitions));
            format!("list of {items}")
        }
        ["integer"] => "whole number".to_owned(),
        ["boolean"] => "true or false".to_owned(),
        ["string"] => "text".to_owned(),
        ["number"] => "number".to_owned(),
        _ => name.map_or_else(
            || "object".to_owned(),
            |name| format!("object (<code>{name}</code>)"),
        ),
    }
}

fn default(schema: &Map<String, Value>) -> Option<String> {
    match schema.get("default")? {
        Value::Null => None,
        Value::Array(values) if values.is_empty() => None,
        Value::Object(fields) if fields.is_empty() => None,
        Value::String(text) => Some(format!("<code>{}</code>", escape(text))),
        other => Some(format!("<code>{}</code>", escape(&other.to_string()))),
    }
}

/// The parameter rows of one tool, as HTML without blank lines so the whole
/// tool stays one HTML block in Markdown.
fn parameters(page: &mut String, schema: &Map<String, Value>) {
    let properties = schema.get("properties").and_then(Value::as_object);
    let Some(properties) = properties.filter(|properties| !properties.is_empty()) else {
        page.push_str("<p class=\"none\">Takes no parameters.</p>\n");
        return;
    };
    let definitions = schema.get("$defs").and_then(Value::as_object);
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map_or_default(|names| names.iter().filter_map(Value::as_str).collect());
    page.push_str("<h4>Parameters</h4>\n<div class=\"params\">\n");
    // Required parameters first, then the rest by name.
    let mut names: Vec<&String> = properties.keys().collect();
    names.sort_by_key(|name| (!required.contains(&name.as_str()), name.as_str()));
    for name in names {
        let Some(property) = properties.get(name).and_then(Value::as_object) else {
            continue;
        };
        let (definition, _) = resolve(property, definitions);
        let mut description = property
            .get("description")
            .or_else(|| definition.get("description"))
            .and_then(Value::as_str)
            .map_or_default(escape);
        if let Some(value) = default(property) {
            if !description.is_empty() {
                description.push(' ');
            }
            let _ = write!(description, "<small>Default {value}.</small>");
        }
        let marker = if required.contains(&name.as_str()) {
            "<b>required</b>"
        } else {
            ""
        };
        let _ = writeln!(
            page,
            "<div class=\"param\"><span class=\"name\"><code>{name}</code><small>{}</small>{marker}</span><span class=\"about\">{description}</span></div>",
            kind(property, definitions)
        );
    }
    page.push_str("</div>\n");
}

/// Render `docs/tools.md` from the Rust-owned tool definitions.
///
/// # Errors
/// Returns an error when a tool is missing from, or listed twice in, the page's groups.
fn generate_tool_reference() -> Result<String, String> {
    let tools = Mash::tools().list_all();
    let grouped: Vec<&str> = GROUPS
        .iter()
        .flat_map(|(_, names)| names.iter().copied())
        .collect();
    for tool in &tools {
        if grouped.iter().filter(|name| **name == tool.name).count() != 1 {
            return Err(format!(
                "list {} exactly once in the tool reference groups",
                tool.name
            ));
        }
    }
    let mut page = INTRODUCTION.to_owned();
    for (group, names) in GROUPS {
        let _ = write!(page, "\n## {group}\n");
        for name in *names {
            let tool = tools
                .iter()
                .find(|tool| tool.name == *name)
                .ok_or_else(|| format!("the tool reference lists unknown tool {name}"))?;
            let annotations = tool.annotations.as_ref();
            let read = annotations.and_then(|hints| hints.read_only_hint) == Some(true);
            let (class, badge) = if read {
                ("read", "READ")
            } else {
                ("write", "WRITE")
            };
            let description = sentences(tool.description.as_deref().unwrap_or_default());
            let (headline, points) = description
                .split_first()
                .map_or(("", [].as_slice()), |(first, rest)| (first.as_str(), rest));
            let _ = write!(
                page,
                "\n<details class=\"mash-tool {class}\" id=\"tool-{name}\">\n<summary><span class=\"badge\">{badge}</span><code>{name}</code><span class=\"about\">{}</span></summary>\n",
                escape(headline)
            );
            if !points.is_empty() {
                page.push_str("<ul>\n");
                for point in points {
                    let _ = writeln!(page, "<li>{}</li>", escape(point));
                }
                page.push_str("</ul>\n");
            }
            if !read && annotations.and_then(|hints| hints.destructive_hint) == Some(true) {
                page.push_str("<p class=\"none\">Changes or removes stored data.</p>\n");
            }
            parameters(&mut page, &tool.input_schema);
            page.push_str("</details>\n");
        }
    }
    Ok(page)
}

#[cfg(test)]
mod tests {
    /// `make generate-docs` sets `MASH_WRITE_TOOL_REFERENCE` to rewrite the page.
    #[test]
    fn tool_reference_is_current() {
        let page = super::generate_tool_reference().unwrap();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/tools.md");
        if std::env::var_os("MASH_WRITE_TOOL_REFERENCE").is_some() {
            std::fs::write(path, &page).unwrap();
        }
        let published = std::fs::read_to_string(path).unwrap_or_default();
        assert!(
            page == published.replace("\r\n", "\n"),
            "docs/tools.md is stale; regenerate it with make generate-docs"
        );
    }
}
