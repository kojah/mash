use super::models::{Content, Message, Part};
use crate::{dialogue, role::Role};

const CONTEXT: &[&str] = &[
    "user_info",
    "project_layout",
    "rules",
    "always_applied_workspace_rules",
    "agent_requestable_workspace_rules",
    "user_rules",
    "agent_skills",
    "available_skills",
    "cloud_instructions",
    "cloud_task_instructions",
    "open_and_recently_viewed_files",
    "system_reminder",
    "system-reminder",
    "instructions_update",
    "mcp_instructions",
    "mcp_file_system",
    "mcp_file_system_servers",
    "git_status",
    "agent_transcripts",
    "cursor_rules_context",
    "attached_files",
    "system_notification",
    "task_notification",
    "agent_notification",
];

#[derive(Default)]
pub struct Text {
    pub canonical: String,
    pub projection: String,
}

impl Text {
    fn context(&mut self, text: &str) {
        if !self.canonical.is_empty() {
            self.canonical.push('\n');
        }
        self.canonical.push_str(text);
        self.canonical.push('\n');
    }
    fn prose(&mut self, text: &str) {
        self.canonical.push_str(text);
        self.projection.push_str(text);
    }
}

pub fn projection(role: &Role, text: &str) -> String {
    let clean = dialogue::without_context_blocks(text, CONTEXT);
    let clean = dialogue::without_context_blocks(&clean, &["think", "thinking"]);
    dialogue::visible(role, &clean).trim().to_owned()
}

pub fn normalized(message: &Message) -> Text {
    let mut out = Text::default();
    match &message.content {
        Content::Text(text) => out.prose(text),
        Content::Parts(parts) => {
            for part in parts {
                match part {
                    Part::Text { text } => out.prose(text),
                    Part::Reasoning { text } => out.context(&format!("Thinking: {text}")),
                    Part::RedactedReasoning => out.context("Thinking: [redacted]"),
                    Part::ToolCall { tool_name, args } => {
                        out.context(&format!("Tool: {tool_name}\n{args}"));
                    }
                    Part::ToolResult { tool_name, result } => {
                        out.context(&format!("Tool result: {tool_name}\n{result}"));
                    }
                    Part::File { filename } => out.context(&format!(
                        "File: {}",
                        filename.as_deref().unwrap_or("attachment")
                    )),
                    Part::Image => out.context("Image attachment"),
                    Part::Unknown => out.context("Unknown message part"),
                }
            }
        }
    }
    out.projection = projection(&message.role, &out.projection);
    out
}
