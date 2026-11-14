//! What Mashy's model may ask for. Every lookup only reads.
//!
//! Each is a plain description with a few plain arguments, and each answers
//! in short lines the model can quote from: IDs to link, titles, local dates.
//! The MCP tools most of them run on describe themselves for coding agents,
//! in schemas small models use badly and some services refuse, and answer in
//! JSON too long to read much of within a question.
use super::{Dash, Definition, Function, Tool};
use crate::core::{self, Operation};
use serde_json::{Value, json};
use std::fmt::Write as _;

/// Characters of one lookup's answer.
const MAX_RESULT: usize = 16_000;
/// Characters of one message when a chat is read.
const MAX_MESSAGE: usize = 1_500;
/// Characters of one of the reader's messages in a chat's outline.
const MAX_OUTLINED: usize = 320;
/// Messages of a chat one `read_chat` looks through. Most are tool activity,
/// which is left out, so far fewer are shown.
const PAGE: u32 = 400;
/// Messages an outline looks through.
const OUTLINE: u32 = 2_500;
/// Chats one listing shows.
const LISTED: usize = 60;

pub(super) const NAMES: [&str; 6] = [
    "search",
    "chats",
    "activity",
    "read_chat",
    "read_message",
    "memories",
];

/// What the lookups know about the reader, and what they have shown them.
pub(super) struct Reader {
    /// The reader's clock, so "yesterday" is their yesterday.
    pub(super) zone: chrono::FixedOffset,
    /// What this machine's own chats are filed under.
    pub(super) node: String,
    /// The memories shown so far. Their IDs are a hundred characters long,
    /// so the model names a memory by its place here, as `m1`, `m2`.
    pub(super) memories: Vec<String>,
}

impl Reader {
    pub(super) fn new(utc_offset_minutes: i32, node: String) -> Self {
        let seconds = utc_offset_minutes.clamp(-14 * 60, 14 * 60) * 60;
        Self {
            zone: chrono::FixedOffset::east_opt(seconds).unwrap_or_else(|| {
                chrono::FixedOffset::east_opt(0).expect("zero is a valid offset")
            }),
            node,
            memories: Vec::new(),
        }
    }

    /// The reader's date, time, weekday and offset now.
    pub(super) fn now(&self) -> String {
        chrono::Utc::now()
            .with_timezone(&self.zone)
            .format("%A %Y-%m-%d %H:%M (UTC%:z)")
            .to_string()
    }

    /// A stored time, in Unix milliseconds, on the reader's clock.
    fn time(&self, milliseconds: i64) -> String {
        chrono::DateTime::from_timestamp_millis(milliseconds).map_or_else(
            || "unknown time".to_owned(),
            |time| {
                time.with_timezone(&self.zone)
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
            },
        )
    }

    fn moment(&self, value: &Value) -> String {
        value
            .as_i64()
            .map_or_else(|| "unknown time".to_owned(), |time| self.time(time))
    }

    /// The start of a `YYYY-MM-DD` day on the reader's clock, in Unix seconds.
    fn day(&self, text: &str) -> Result<i64, String> {
        use chrono::TimeZone as _;
        let date = chrono::NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d")
            .map_err(|_| format!("Write dates as YYYY-MM-DD, not {}.", quoted(text)))?;
        date.and_hms_opt(0, 0, 0)
            .and_then(|start| self.zone.from_local_datetime(&start).single())
            .map(|start| start.timestamp())
            .ok_or_else(|| "That date is out of range.".to_owned())
    }

    /// A period as Unix seconds: from the start of `from` to the end of `to`.
    fn period(&self, from: Option<&str>, to: Option<&str>) -> Result<Period, String> {
        let after = from.map(|from| self.day(from)).transpose()?;
        let before = to
            .map(|to| self.day(to).map(|start| start + 86_400))
            .transpose()?;
        if let (Some(after), Some(before)) = (after, before)
            && before <= after
        {
            return Err("The period ends before it starts.".to_owned());
        }
        Ok(Period { after, before })
    }

    /// The record a memory link names: by its place among those shown, or in full.
    pub(super) fn memory(&self, name: &str) -> Option<&str> {
        let place: usize = name.strip_prefix('m')?.parse().ok()?;
        self.memories.get(place.checked_sub(1)?).map(String::as_str)
    }

    fn remember(&mut self, id: &str) -> usize {
        if let Some(place) = self.memories.iter().position(|known| known == id) {
            return place + 1;
        }
        self.memories.push(id.to_owned());
        self.memories.len()
    }
}

#[derive(Clone, Copy)]
struct Period {
    after: Option<i64>,
    before: Option<i64>,
}

fn quoted(text: &str) -> String {
    Value::String(clip(text, 80)).to_string()
}

/// At most `limit` characters of `text`.
pub(super) fn clip(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}

/// The start and the end of a long text, with the middle left out. People put
/// the point of a long message last as often as first.
pub(super) fn ends(text: &str, head: usize, tail: usize) -> String {
    let count = text.chars().count();
    if count <= head + tail {
        return text.to_owned();
    }
    let start: String = text.chars().take(head).collect();
    let end: String = text.chars().skip(count - tail).collect();
    format!("{start} […] {end}")
}

/// Text on one line.
fn line(text: &str, limit: usize) -> String {
    clip(
        &text.split_whitespace().collect::<Vec<_>>().join(" "),
        limit,
    )
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// The last part of a project's directory, which is how people name it.
fn project(value: &Value) -> &str {
    let path = text(value, "project").trim_end_matches('/');
    path.rsplit('/').next().unwrap_or(path)
}

/// Who wrote a message, in the words the instructions use.
fn speaker(role: &str) -> &str {
    match role {
        "user" => "reader",
        "assistant" => "agent",
        other => other,
    }
}

/// One lookup the model may ask for.
fn tool(name: &str, description: &str, properties: &Value, required: &[&str]) -> Tool {
    Tool {
        r#type: "function",
        function: Definition {
            name: name.to_owned(),
            description: description.to_owned(),
            parameters: json!({
                "type": "object",
                "properties": properties,
                "required": required,
            }),
        },
    }
}

/// Everything the model may ask for.
pub(super) fn tools() -> Vec<Tool> {
    let day = "A day as YYYY-MM-DD on the reader's clock.";
    vec![
        tool(
            "search",
            "Search what was said in the reader's past chats. By default it searches by meaning. Each result gives the chat_id and title and the matching messages with their message_id, who wrote them and when.",
            &json!({
                "query": {"type": "string", "description": "The reader's own words, a question, or an identifier."},
                "exact": {"type": "boolean", "description": "True to match the words themselves instead of the meaning: every word must appear in one message. Put a phrase in double quotes to match it as written. Use it for names, numbers, identifiers and remembered phrases."},
                "by": {"type": "string", "enum": ["reader", "agent"], "description": "Only messages written by the reader, or only by their coding agent. Use reader for what they said, asked or decided."},
                "in_chat": {"type": "string", "description": "A chat_id, to search inside that chat only."},
                "from": {"type": "string", "description": format!("Only messages from this day on. {day}")},
                "to": {"type": "string", "description": format!("Only messages up to and including this day. {day}")},
                "more": {"type": "integer", "description": "The number a previous search gave for its next results."}
            }),
            &["query"],
        ),
        tool(
            "chats",
            "List chats with chat_id, title, agent, project, when each started and when it was last active. Newest activity first. A chat that ran over several days is listed by its last activity; use activity to see what happened on a given day.",
            &json!({
                "from": {"type": "string", "description": format!("Only chats active on or after this day. {day}")},
                "to": {"type": "string", "description": format!("Only chats last active on or before this day. {day}")},
                "oldest_first": {"type": "boolean", "description": "True to list by when chats started, oldest first."},
                "title_contains": {"type": "string", "description": "Only chats whose title contains this text."},
                "agent": {"type": "string", "enum": ["claude-code", "codex", "opencode", "copilot", "cursor", "antigravity"]},
                "more": {"type": "integer", "description": "The number a previous listing gave for its next chats."}
            }),
            &[],
        ),
        tool(
            "activity",
            "What the reader worked on in a period: every chat with messages in it, how many messages each had then, and totals by agent and project. Use it first for any question about a day, week or month, and for counting.",
            &json!({
                "from": {"type": "string", "description": format!("The first day. {day}")},
                "to": {"type": "string", "description": format!("The last day, included; leave out for the one day. {day}")}
            }),
            &["from"],
        ),
        tool(
            "read_chat",
            "Read one chat's dialogue in order: what the reader and the agent said, with tool activity left out. It says how long the chat is and where to read on. A long chat does not fit in one read.",
            &json!({
                "chat_id": {"type": "string"},
                "node": {"type": "string", "description": "The machine the chat is on, when it is not this one."},
                "shared": {"type": "boolean", "description": "True when the chat is read from the shared index."},
                "offset": {"type": "integer", "description": "The position to start from; 0 is the beginning."},
                "from_end": {"type": "boolean", "description": "True to read the end of the chat: how it ended, the latest messages."},
                "on": {"type": "string", "description": format!("Start at the first message of this day. {day}")},
                "outline": {"type": "boolean", "description": "True to see only what the reader wrote, shortened, across thousands of messages. Use it first on a long chat to find the parts worth reading."}
            }),
            &["chat_id"],
        ),
        tool(
            "read_message",
            "Read one message in full with the dialogue around it, by a message_id from search.",
            &json!({
                "message_id": {"type": "string"},
                "context": {"type": "integer", "description": "Messages on each side, up to 10."}
            }),
            &["message_id"],
        ),
        tool(
            "memories",
            "List the decisions, preferences and facts saved from the reader's chats, newest first. Each has a short id such as m3, its kind, date, the project it applies to and the chat it came from.",
            &json!({
                "kind": {"type": "string", "enum": ["decision", "preference", "fact"]},
                "about": {"type": "string", "description": "One or two words the memory or its project must contain, such as a project name."},
                "chat_id": {"type": "string", "description": "Only memories saved from this chat."},
                "more": {"type": "integer", "description": "The number a previous listing gave for its next memories."}
            }),
            &[],
        ),
    ]
}

// What each lookup accepts. Models add arguments of their own now and then;
// those are ignored, never passed on.

#[derive(serde::Deserialize)]
struct Search {
    query: String,
    #[serde(default)]
    exact: bool,
    #[serde(default)]
    by: Option<String>,
    #[serde(default)]
    in_chat: Option<String>,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    more: Option<u32>,
}

#[derive(serde::Deserialize)]
struct Chats {
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    oldest_first: bool,
    #[serde(default)]
    title_contains: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    more: Option<u32>,
}

#[derive(serde::Deserialize)]
struct Activity {
    from: String,
    #[serde(default)]
    to: Option<String>,
}

#[derive(serde::Deserialize)]
struct ReadChat {
    chat_id: String,
    #[serde(default)]
    node: Option<String>,
    #[serde(default)]
    shared: bool,
    #[serde(default)]
    offset: u32,
    #[serde(default)]
    from_end: bool,
    #[serde(default)]
    on: Option<String>,
    #[serde(default)]
    outline: bool,
}

#[derive(serde::Deserialize)]
struct ReadMessage {
    message_id: String,
    #[serde(default)]
    context: Option<u32>,
}

#[derive(serde::Deserialize)]
struct Memories {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    about: Option<String>,
    #[serde(default)]
    chat_id: Option<String>,
    #[serde(default)]
    more: Option<usize>,
}

fn parse<T: serde::de::DeserializeOwned>(arguments: &str) -> Result<T, String> {
    let arguments = if arguments.trim().is_empty() {
        "{}"
    } else {
        arguments
    };
    serde_json::from_str(arguments).map_err(|_| "The arguments do not fit this lookup.".to_owned())
}

fn given(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

/// Runs one lookup and returns what the model reads. A lookup that is not on
/// the list, or whose arguments make no sense, is answered with a refusal the
/// model can read; nothing else happens. The console refuses any tool that is
/// not read-only, whatever this module asks it for.
pub(super) async fn run(state: &Dash, reader: &mut Reader, call: &Function) -> String {
    let answer = match call.name.as_str() {
        "search" => search(state, reader, &call.arguments).await,
        "chats" => chats(state, reader, &call.arguments).await,
        "activity" => activity(state, reader, &call.arguments).await,
        "read_chat" => read_chat(state, reader, &call.arguments).await,
        "read_message" => read_message(state, reader, &call.arguments).await,
        "memories" => memories(state, reader, &call.arguments).await,
        _ => Err("There is no such lookup.".to_owned()),
    };
    clip(&answer.unwrap_or_else(|refusal| refusal), MAX_RESULT)
}

/// One read-only MCP tool's reply, or what the model is told when it failed.
async fn console(state: &Dash, name: &str, arguments: Value) -> Result<Value, String> {
    let Value::Object(arguments) = arguments else {
        return Err("The lookup failed.".to_owned());
    };
    let answer = state
        .console
        .call(name, arguments)
        .await
        .map_err(|_| "The lookup failed.".to_owned())?;
    let reply = answer.reply().cloned().unwrap_or(Value::Null);
    if answer.failed() {
        let reason = reply
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("it could not be run");
        return Err(format!("The lookup failed: {}", clip(reason, 300)));
    }
    Ok(reply)
}

fn rows(reply: &Value, key: &str) -> Vec<Value> {
    reply
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

async fn search(state: &Dash, reader: &mut Reader, arguments: &str) -> Result<String, String> {
    let input: Search = parse(arguments)?;
    let period = reader.period(input.from.as_deref(), input.to.as_deref())?;
    let chat = given(input.in_chat);
    let by = match input.by.as_deref() {
        Some("reader") => Some("user"),
        Some("agent") => Some("assistant"),
        _ => None,
    };
    // Whole chats by default; single messages when the question is about who
    // said something or about one chat.
    let messages = chat.is_some() || by.is_some();
    let mut request = json!({
        "query": input.query,
        "limit": if messages { 12 } else { 8 },
        "intent": if input.exact { "exact-mention" } else { "related-discussion" },
        "unit": if messages { "messages" } else { "chats" },
        "offset": input.more.unwrap_or(0),
    });
    if let Some(chat) = chat {
        request["chat"] = json!([chat]);
    }
    if let Some(role) = by {
        request["role"] = json!([role]);
    }
    if let Some(after) = period.after {
        request["after"] = json!(after);
    }
    if let Some(before) = period.before {
        request["before"] = json!(before);
    }
    let reply = console(state, "search", request).await?;
    let results = rows(&reply, "results");
    if results.is_empty() {
        return Ok(if input.exact {
            "No message has all of those words. Try fewer words, or search by meaning with exact left out.".to_owned()
        } else {
            "Nothing matched. Try the reader's own words, or exact=true for a name or phrase."
                .to_owned()
        });
    }
    let mut shown = String::new();
    for result in &results {
        if messages {
            let about = result.get("metadata").unwrap_or(&Value::Null);
            let _ = writeln!(
                shown,
                "message_id={} | {} at {} | in chat_id={} {} | {}",
                text(result, "id"),
                speaker(text(result, "role")),
                reader.moment(&result["created_at"]),
                text(result, "chat_id"),
                quoted_title(text(about, "title")),
                line(text(result, "content"), 700),
            );
            continue;
        }
        let _ = writeln!(shown, "{}", chat_line(reader, result));
        for found in rows(result, "matches").iter().take(3) {
            let _ = writeln!(
                shown,
                "  message_id={} | {} at {} | {}",
                text(found, "message_id"),
                speaker(text(found, "role")),
                reader.moment(&found["created_at"]),
                line(text(found, "snippet"), 500),
            );
        }
    }
    if let Some(next) = reply.get("next_offset").and_then(Value::as_u64) {
        let _ = writeln!(
            shown,
            "There are more results: search again with more={next}."
        );
    }
    Ok(shown)
}

fn quoted_title(title: &str) -> String {
    if title.trim().is_empty() {
        "(untitled)".to_owned()
    } else {
        Value::String(line(title, 110)).to_string()
    }
}

/// One chat on one line, as every listing shows it.
fn chat_line(reader: &Reader, chat: &Value) -> String {
    let mut shown = format!(
        "chat_id={} | {} | {}",
        text(chat, "chat_id"),
        quoted_title(text(chat, "title")),
        text(chat, "provider"),
    );
    let node = text(chat, "node_id");
    if !node.is_empty() && node != reader.node {
        let _ = write!(shown, " | on machine {node}");
    }
    if !project(chat).is_empty() {
        let _ = write!(shown, " | project {}", project(chat));
    }
    if chat["created_at"].is_i64() {
        let _ = write!(shown, " | started {}", reader.moment(&chat["created_at"]));
    }
    if chat["updated_at"].is_i64() {
        let _ = write!(
            shown,
            " | last active {}",
            reader.moment(&chat["updated_at"])
        );
    }
    shown
}

async fn chats(state: &Dash, reader: &mut Reader, arguments: &str) -> Result<String, String> {
    let input: Chats = parse(arguments)?;
    let period = reader.period(input.from.as_deref(), input.to.as_deref())?;
    let offset = input.more.unwrap_or(0);
    let mut request = json!({"limit": LISTED, "offset": offset});
    if input.oldest_first {
        request["sort"] = json!("created_at");
        request["direction"] = json!("asc");
    }
    if let Some(after) = period.after {
        request["after"] = json!(after);
    }
    if let Some(before) = period.before {
        request["before"] = json!(before);
    }
    if let Some(title) = given(input.title_contains) {
        request["title_contains"] = json!(title);
    }
    if let Some(agent) = given(input.agent) {
        request["provider"] = json!([agent]);
    }
    let reply = console(state, "list_chats", request).await?;
    let listed = rows(&reply, "chats");
    if listed.is_empty() {
        return Ok("No chats match.".to_owned());
    }
    let mut shown = String::new();
    for chat in &listed {
        let _ = writeln!(shown, "{}", chat_line(reader, chat));
    }
    match reply.get("next_offset").and_then(Value::as_u64) {
        Some(next) => {
            let _ = writeln!(
                shown,
                "These are {} chats; there are more: list again with more={next}.",
                listed.len()
            );
        }
        None => {
            let _ = writeln!(shown, "That is all of them: {} chats.", listed.len());
        }
    }
    Ok(shown)
}

/// Every chat with messages in a period. A chat that ran for days is found
/// by the messages it had then, not by when it was last touched.
async fn activity(state: &Dash, reader: &mut Reader, arguments: &str) -> Result<String, String> {
    let input: Activity = parse(arguments)?;
    let to = input.to.as_deref().unwrap_or(&input.from);
    let period = reader.period(Some(&input.from), Some(to))?;
    let (Some(after), Some(before)) = (period.after, period.before) else {
        return Err("Give the first day of the period.".to_owned());
    };
    let counted = console(
        state,
        "query_silver",
        json!({"request": {
            "target": "messages",
            "filter": {"since": after * 1000, "until": before * 1000},
            "group_by": "chat",
            "limit": 1000,
        }}),
    )
    .await?;
    let mut counts: Vec<(String, u64)> = rows(&counted, "rows")
        .iter()
        .map(|row| {
            (
                text(row, "group_key").to_owned(),
                row["count"].as_u64().unwrap_or(0),
            )
        })
        .collect();
    if counts.is_empty() {
        return Ok("No chat on this machine has messages in that period.".to_owned());
    }
    counts.sort_by_key(|left| std::cmp::Reverse(left.1));
    // A chat with messages in the period was last active in or after it.
    let mut known = std::collections::HashMap::new();
    let mut offset = 0;
    while known.len() < counts.len() && offset < 2_000 {
        let page = console(
            state,
            "list_chats",
            json!({"limit": 100, "offset": offset, "after": after, "local": true}),
        )
        .await?;
        for chat in rows(&page, "chats") {
            known.insert(text(&chat, "chat_id").to_owned(), chat);
        }
        match page.get("next_offset").and_then(Value::as_u64) {
            Some(next) => offset = next,
            None => break,
        }
    }
    let mut agents = std::collections::BTreeMap::<String, usize>::new();
    let mut projects = std::collections::BTreeMap::<String, usize>::new();
    for (id, _) in &counts {
        if let Some(chat) = known.get(id) {
            *agents.entry(text(chat, "provider").to_owned()).or_default() += 1;
            *projects.entry(project(chat).to_owned()).or_default() += 1;
        }
    }
    let tally = |counted: std::collections::BTreeMap<String, usize>| {
        let mut counted: Vec<_> = counted.into_iter().collect();
        counted.sort_by_key(|left| std::cmp::Reverse(left.1));
        counted
            .iter()
            .take(12)
            .map(|(name, chats)| format!("{name} {chats}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut shown = format!(
        "{} chats had messages from {} to {} (the reader's clock).\nChats by agent: {}.\nChats by project: {}.\nBusiest first; \"then\" is the number of messages in the period, tool activity included:\n",
        counts.len(),
        input.from.trim(),
        to.trim(),
        tally(agents),
        tally(projects),
    );
    for (id, count) in counts.iter().take(LISTED) {
        match known.get(id) {
            Some(chat) => {
                let _ = writeln!(shown, "{} | then {count} messages", chat_line(reader, chat));
            }
            None => {
                let _ = writeln!(shown, "chat_id={id} | then {count} messages");
            }
        }
    }
    if counts.len() > LISTED {
        let _ = writeln!(
            shown,
            "The other {} chats had fewer messages each and are not listed.",
            counts.len() - LISTED
        );
    }
    Ok(shown)
}

/// Harness text that agent tools wrap into the reader's messages.
static WRAPPED: std::sync::LazyLock<Option<regex::Regex>> = std::sync::LazyLock::new(|| {
    regex::Regex::new(
        r"(?s)<(system-reminder|task-notification|local-command-stdout|local-command-caveat)>.*?</(system-reminder|task-notification|local-command-stdout|local-command-caveat)>",
    )
    .ok()
});

/// What a message says, when it is dialogue: tool calls, tool output, an
/// agent's notes to itself and harness text are not.
fn said(message: &crate::storage::Message) -> Option<(&'static str, String)> {
    let who = match message.role.as_str() {
        "user" => "reader",
        "assistant" => "agent",
        _ => return None,
    };
    if !message.tools.is_empty() {
        return None;
    }
    let content = match WRAPPED.as_ref() {
        Some(wrapped) => wrapped.replace_all(&message.content, " ").into_owned(),
        None => message.content.clone(),
    };
    let content = content.trim();
    if content.is_empty() || content.starts_with("[Thinking]") || content.starts_with("Reasoning:")
    {
        return None;
    }
    Some((who, content.to_owned()))
}

async fn page(
    state: &Dash,
    input: &ReadChat,
    offset: u32,
    limit: u32,
) -> Result<core::ViewResponse, String> {
    let request = core::Request {
        paths: state.paths.clone(),
        operation: Operation::ChatMessages {
            chat_id: input.chat_id.clone(),
            offset,
            limit,
            node: input.node.clone().filter(|node| !node.is_empty()),
            shared: input.shared || state.shared_only,
        },
    };
    match core::execute(request).await {
        Ok(core::Response::View(view)) => Ok(view),
        Ok(_) => Err("The chat could not be read.".to_owned()),
        Err(error) => Err(clip(&error.error.message, 300)),
    }
}

/// Where in a chat the first message of a day sits.
async fn first_of_day(state: &Dash, reader: &Reader, chat: &str, day: &str) -> Result<u32, String> {
    let since = reader.day(day)? * 1000;
    let found = console(
        state,
        "query_silver",
        json!({"request": {
            "target": "messages",
            "filter": {"chat": chat, "since": since},
            "order": "created_at",
            "limit": 1,
        }}),
    )
    .await?;
    rows(&found, "rows")
        .first()
        .and_then(|row| row["ordinal"].as_u64())
        .and_then(|ordinal| u32::try_from(ordinal).ok())
        .ok_or_else(|| format!("This chat has no messages on or after {}.", day.trim()))
}

/// The lines that fit in one answer, and the position of the first one that
/// did not. What does not fit is dropped from the far end: the start when
/// reading the end of a chat, otherwise the end.
fn fit(mut lines: Vec<(i64, String)>, from_end: bool) -> (Vec<(i64, String)>, Option<i64>) {
    let budget = MAX_RESULT - 600;
    let mut size = 0;
    let mut dropped = None;
    if from_end {
        let mut kept = Vec::new();
        for (ordinal, text) in lines.into_iter().rev() {
            size += text.chars().count() + 1;
            if size > budget {
                dropped = Some(ordinal);
                break;
            }
            kept.push((ordinal, text));
        }
        kept.reverse();
        lines = kept;
    } else if let Some(cut) = lines.iter().position(|(_, text)| {
        size += text.chars().count() + 1;
        size > budget
    }) {
        dropped = Some(lines[cut].0);
        lines.truncate(cut);
    }
    (lines, dropped)
}

async fn read_chat(state: &Dash, reader: &mut Reader, arguments: &str) -> Result<String, String> {
    let input: ReadChat = parse(arguments)?;
    let first = page(state, &input, 0, 1).await?;
    if first.messages.is_empty() {
        return Ok("There is no chat with this ID here, or it has no messages. Say so; do not describe it or link it.".to_owned());
    }
    let total = first
        .chat
        .as_ref()
        .and_then(|chat| u32::try_from(chat.message_count).ok());
    if input.outline {
        return outline(state, reader, &input, total).await;
    }
    let start = if let Some(day) = given(input.on.clone()) {
        first_of_day(state, reader, &input.chat_id, &day).await?
    } else if input.from_end {
        total.map_or(input.offset, |total| total.saturating_sub(PAGE))
    } else {
        input.offset
    };
    let view = page(state, &input, start, PAGE).await?;
    if view.messages.is_empty() {
        return Ok(match total {
            Some(total) => {
                format!("This chat has {total} messages; there is none at position {start}.")
            }
            None => format!("There is no message at position {start}."),
        });
    }
    let lines: Vec<(i64, String)> = view
        .messages
        .iter()
        .filter_map(|message| {
            let (who, content) = said(message)?;
            let when = message
                .created_at
                .map_or_else(String::new, |time| format!(" {}", reader.time(time)));
            Some((
                message.ordinal,
                format!(
                    "[{}] {who}{when}: {}",
                    message.ordinal,
                    ends(&content, MAX_MESSAGE * 2 / 3, MAX_MESSAGE / 3)
                ),
            ))
        })
        .collect();
    let last = view.messages.last().map_or(0, |message| message.ordinal);
    let (lines, dropped) = fit(lines, input.from_end);
    let mut shown = match total {
        Some(total) => format!("This chat has {total} messages, numbered from 0. "),
        None => String::new(),
    };
    let _ = writeln!(
        shown,
        "Dialogue from position {start} to {} follows; tool activity is left out.",
        dropped
            .filter(|_| !input.from_end)
            .map_or(last, |cut| cut - 1),
    );
    if input.from_end
        && let Some(cut) = dropped
    {
        let _ = writeln!(
            shown,
            "It starts after position {cut}; earlier messages did not fit."
        );
    }
    if lines.is_empty() {
        shown.push_str("(Only tool activity here, no dialogue.)\n");
    }
    for (_, text) in &lines {
        let _ = writeln!(shown, "{text}");
    }
    let next = dropped.filter(|_| !input.from_end).unwrap_or(last + 1);
    if total.is_none_or(|total| next < i64::from(total)) && !input.from_end {
        let _ = writeln!(shown, "The chat goes on: read on with offset={next}.");
    } else if !input.from_end || dropped.is_none() {
        shown.push_str("That is the end of the chat.\n");
    }
    Ok(shown)
}

/// What the reader wrote across a long stretch of a chat, shortened: enough
/// to see what it was about and where to read.
async fn outline(
    state: &Dash,
    reader: &Reader,
    input: &ReadChat,
    total: Option<u32>,
) -> Result<String, String> {
    let mut shown = match total {
        Some(total) => format!("This chat has {total} messages, numbered from 0. "),
        None => String::new(),
    };
    let _ = writeln!(
        shown,
        "What the reader wrote from position {} on, shortened; read a part in full with read_chat and offset.",
        input.offset
    );
    let mut offset = input.offset;
    let mut ended = false;
    let mut full = false;
    while offset < input.offset.saturating_add(OUTLINE) && !full {
        let view = page(state, input, offset, 500).await?;
        if view.messages.is_empty() {
            ended = true;
            break;
        }
        for message in &view.messages {
            let Some(("reader", content)) = said(message) else {
                continue;
            };
            let when = message
                .created_at
                .map_or_else(String::new, |time| format!(" {}", reader.time(time)));
            let entry = format!(
                "[{}]{when}: {}",
                message.ordinal,
                ends(
                    &line(&content, 4_000),
                    MAX_OUTLINED * 2 / 3,
                    MAX_OUTLINED / 3
                )
            );
            if shown.chars().count() + entry.chars().count() > MAX_RESULT - 400 {
                full = true;
                offset = u32::try_from(message.ordinal).unwrap_or(offset);
                break;
            }
            let _ = writeln!(shown, "{entry}");
        }
        if !full {
            offset += u32::try_from(view.messages.len()).unwrap_or(500);
            ended = view.messages.len() < 500;
            if ended {
                break;
            }
        }
    }
    if ended || total.is_some_and(|total| offset >= total) {
        shown.push_str("That is the end of the chat.\n");
    } else {
        let _ = writeln!(
            shown,
            "The outline stops at position {offset}; continue it with outline=true and offset={offset}."
        );
    }
    Ok(shown)
}

async fn read_message(
    state: &Dash,
    reader: &mut Reader,
    arguments: &str,
) -> Result<String, String> {
    let input: ReadMessage = parse(arguments)?;
    let view = console(
        state,
        "view",
        json!({"id": input.message_id, "context": input.context.unwrap_or(3).min(10)}),
    )
    .await?;
    let mut shown = format!(
        "In chat_id={}, which has {} messages:\n",
        text(&view, "chat_id"),
        view["message_count"].as_u64().unwrap_or(0)
    );
    for message in rows(&view, "messages") {
        let wanted = text(&message, "id") == input.message_id;
        // Around the message only dialogue matters; the message itself is
        // shown whatever it is.
        if !wanted && (message.get("tools").is_some() || text(&message, "role") == "tool") {
            continue;
        }
        let content = text(&message, "content").trim();
        if content.is_empty() {
            continue;
        }
        let _ = writeln!(
            shown,
            "[{}] {} {}{}: {}",
            message["ordinal"].as_i64().unwrap_or(0),
            speaker(text(&message, "role")),
            reader.moment(&message["created_at"]),
            if wanted {
                " (the message asked for)"
            } else {
                ""
            },
            if wanted {
                ends(content, 6_000, 3_000)
            } else {
                ends(content, 800, 400)
            },
        );
    }
    Ok(shown)
}

/// The project a memory applies to, as people name it.
fn applies_to(record: &Value) -> String {
    record
        .pointer("/scope/identity")
        .and_then(Value::as_str)
        .map(|identity| {
            let identity = identity.trim_end_matches(".git").trim_end_matches('/');
            identity.rsplit('/').next().unwrap_or(identity).to_owned()
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "everything".to_owned())
}

/// Whether a memory is a decision, a preference or a fact.
fn kind(body: &Value) -> Option<&str> {
    body.pointer("/details/kind")
        .or_else(|| body.get("kind"))
        .and_then(Value::as_str)
}

/// The words of one memory that a question's words are matched against.
fn searchable(record: &Value, body: &Value) -> String {
    format!(
        "{} {} {} {}",
        text(body, "statement"),
        text(body, "rationale"),
        record
            .pointer("/scope/identity")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        kind(body).unwrap_or_default(),
    )
    .to_lowercase()
}

async fn memories(state: &Dash, reader: &mut Reader, arguments: &str) -> Result<String, String> {
    let input: Memories = parse(arguments)?;
    let mut all = Vec::new();
    let mut offset = 0;
    // There are tens of memories, not thousands: all of them are read, and
    // the filters the model may use are applied here.
    while offset < 1_000 {
        let mut request = json!({"limit": 100, "offset": offset});
        if let Some(kind) = given(input.kind.clone()) {
            request["kind"] = json!(kind);
        }
        if let Some(chat) = given(input.chat_id.clone()) {
            request["chat"] = json!(chat);
        }
        let reply = console(state, "list_records", request).await?;
        all.extend(rows(&reply, "records"));
        match reply.get("next_offset").and_then(Value::as_u64) {
            Some(next) if next > offset => offset = next,
            _ => break,
        }
    }
    let words: Vec<String> = given(input.about).map_or_default(|about| {
        about
            .to_lowercase()
            .split_whitespace()
            .map(str::to_owned)
            .collect()
    });
    let mut entries = Vec::new();
    for record in &all {
        // The listing gives a memory either with its revisions, the current
        // one holding the words, or flat, as its current words alone.
        let current = record["current_revision"].as_u64();
        let revision = rows(record, "revisions")
            .into_iter()
            .find(|revision| revision["revision"].as_u64() == current)
            .unwrap_or_else(|| record.clone());
        let body = revision
            .get("body")
            .cloned()
            .unwrap_or_else(|| record.clone());
        if text(&body, "statement").is_empty() {
            continue;
        }
        let known = searchable(record, &body);
        if !words.iter().all(|word| known.contains(word)) {
            continue;
        }
        entries.push((record, revision, body));
    }
    if entries.is_empty() {
        return Ok(format!(
            "No saved memory matches, out of {} looked through. Memories hold only what was explicitly saved; search the chats for anything else.",
            all.len()
        ));
    }
    // Newest first, whatever order they were listed in.
    entries.sort_by_key(|(_, revision, _)| std::cmp::Reverse(revision["recorded_at"].as_i64()));
    let skip = input.more.unwrap_or(0);
    let mut shown = format!("{} memories match. Newest first:\n", entries.len());
    let mut listed = 0;
    for (record, revision, body) in entries.iter().skip(skip) {
        let scope = applies_to(record);
        let mut entry = format!(
            "m{} | {} | saved {} | applies to {scope} | {}",
            reader.remember(text(record, "id")),
            kind(body).unwrap_or("memory"),
            reader.moment(&revision["recorded_at"]),
            line(text(body, "statement"), 420),
        );
        if !text(body, "rationale").is_empty() {
            let _ = write!(entry, " | why: {}", line(text(body, "rationale"), 220));
        }
        if let Some(chat) = rows(revision, "evidence")
            .first()
            .map(|evidence| text(evidence, "chat_id").to_owned())
            .filter(|chat| !chat.is_empty())
        {
            let _ = write!(entry, " | from chat_id={chat}");
        }
        if shown.chars().count() + entry.chars().count() > MAX_RESULT - 300 {
            break;
        }
        let _ = writeln!(shown, "{entry}");
        listed += 1;
    }
    if skip + listed < entries.len() {
        let _ = writeln!(
            shown,
            "{} more are not shown: list again with more={}, or narrow with kind or about.",
            entries.len() - skip - listed,
            skip + listed
        );
    }
    Ok(shown)
}
