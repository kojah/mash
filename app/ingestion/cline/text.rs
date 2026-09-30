//! Separate native visible prose from model input and operational context.
use super::models::{self, Block, Content, ToolBlock, ToolContent};
use crate::role::Role;
use serde::Deserialize;
use std::collections::HashSet;

fn strip_envelope(text: &str) -> String {
    let text = crate::search_text::without_context_blocks(
        text,
        &["mode_notice", "file_content", "environment_details"],
    );
    let mut out = text.trim().to_owned();
    for tag in ["user_input", "user_command"] {
        let opening = format!("<{tag}");
        let closing = format!("</{tag}>");
        // These are provider-generated outer envelopes, not arbitrary XML in prose.
        if out.starts_with(&opening)
            && out[opening.len()..].starts_with([' ', '>'])
            && let Some(end) = out.find('>')
            && let Some(body) = out[end + 1..].strip_suffix(&closing)
        {
            out = body.trim().to_owned();
        }
    }
    out
}

fn append_text(out: &mut String, text: &str) {
    if text.is_empty() {
        return;
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(text);
}

pub(super) struct Text {
    pub(super) canonical: String,
    pub(super) projection: String,
    pub(super) interactive_reply: bool,
}

pub(super) fn sdk_text(content: Content, role: &Role, questions: &mut HashSet<String>) -> Text {
    let mut canonical = String::new();
    let mut projection = String::new();
    let mut interactive_reply = false;
    match content {
        Content::Text(text) => {
            canonical.clone_from(&text);
            projection = text;
        }
        Content::Blocks(blocks) => {
            for block in blocks {
                match block {
                    Block::Text { text } => {
                        append_text(&mut canonical, &text);
                        append_text(&mut projection, &text);
                    }
                    Block::Thinking { thinking } => append_text(&mut canonical, &thinking),
                    Block::ToolUse { id, name, input } => {
                        append_text(&mut canonical, &format!("[tool: {name}]"));
                        if let Some(input) = input {
                            append_text(&mut canonical, &input.to_string());
                            if *role == Role::Assistant
                                && matches!(name.as_str(), "ask_question" | "ask_followup_question")
                                && let Ok(question) = models::Question::deserialize(&input)
                            {
                                append_text(&mut projection, &question.question);
                                if let Some(id) = id {
                                    questions.insert(id);
                                }
                            }
                            // These native tools carry the assistant's displayed
                            // final answer rather than ordinary tool arguments.
                            if *role == Role::Assistant
                                && matches!(name.as_str(), "attempt_completion" | "submit_and_exit")
                                && let Ok(completion) =
                                    serde_json::from_value::<models::Completion>(input)
                                && let Some(text) = completion.summary.or(completion.result)
                            {
                                append_text(&mut projection, &text);
                            }
                        }
                    }
                    Block::ToolResult {
                        tool_use_id,
                        is_error,
                        content,
                    } => {
                        append_text(&mut canonical, "[tool result]");
                        let reply = *role == Role::User
                            && !is_error
                            && tool_use_id.is_some_and(|id| questions.remove(&id));
                        interactive_reply |= reply;
                        match content {
                            ToolContent::Text(text) => {
                                append_text(&mut canonical, &text);
                                if reply {
                                    append_text(&mut projection, &text);
                                }
                            }
                            ToolContent::Blocks(blocks) => {
                                for block in blocks {
                                    match block {
                                        ToolBlock::Text { text } => {
                                            append_text(&mut canonical, &text);
                                            if reply {
                                                append_text(&mut projection, &text);
                                            }
                                        }
                                        ToolBlock::File { path } => {
                                            append_text(&mut canonical, &format!("[file: {path}]"));
                                        }
                                        ToolBlock::Operational => {}
                                    }
                                }
                            }
                        }
                    }
                    Block::File { path } => append_text(&mut canonical, &format!("[file: {path}]")),
                    Block::Operational => {}
                }
            }
        }
    }
    if *role == Role::User {
        projection = strip_envelope(&projection);
        if projection.starts_with("[TASK RESUMPTION]")
            || projection.contains(super::migration::RESUME_WARNING)
            || projection
                == "The user approved switching to act mode. Continue with the approved plan now."
            || projection.starts_with("<hook_context")
        {
            projection.clear();
        }
    }
    projection = crate::search_text::project(role, &projection).into_owned();
    Text {
        canonical,
        projection,
        interactive_reply,
    }
}
