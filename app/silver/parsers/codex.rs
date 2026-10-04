//! Reconcile nearest raw/completed dialogue counterparts without global text deduplication.
use super::{ExtractedMessage, parse_timestamp, truncate_chars};
use crate::role::Role;
use crate::silver::MAX_TOOL_OUTPUT_CHARS;
use crate::silver::json::{self, AgentText, CompletedItem, Kind, Record, UserText};

#[derive(Clone, Copy, PartialEq)]
enum Origin {
    Raw,
    Completed,
}

struct Candidate {
    origin: Origin,
    role: Role,
    digest: blake3::Hash,
    turn: Option<String>,
    next_turn: bool,
}

#[derive(Default)]
pub(super) struct Extractor {
    turn: Option<String>,
    completed_user: bool,
    pending: Option<Candidate>,
}

impl Extractor {
    pub(super) fn next(&mut self, raw: &Record) -> Option<ExtractedMessage> {
        let payload = raw.payload.as_ref()?;
        if raw.kind == Kind::EventMsg {
            match payload.kind {
                Kind::TaskStarted => {
                    let turn = payload.turn_id.clone()?;
                    if let Some(candidate) = &mut self.pending {
                        if candidate.next_turn {
                            candidate.turn = Some(turn.clone());
                            candidate.next_turn = false;
                        } else if candidate.turn.as_ref() != Some(&turn) {
                            self.pending = None;
                        }
                    }
                    self.turn = Some(turn);
                    self.completed_user = false;
                    return None;
                }
                Kind::TaskComplete => {
                    if self.turn.as_ref() != payload.turn_id.as_ref() {
                        return None;
                    }
                    if let Some(candidate) = &self.pending
                        && !candidate.next_turn
                    {
                        self.pending = None;
                    }
                    self.turn = None;
                    self.completed_user = false;
                    return None;
                }
                _ => {}
            }
        }
        let (origin, message) = if raw.kind == Kind::ResponseItem {
            (Origin::Raw, extract_codex_response_item(raw)?)
        } else if raw.kind == Kind::EventMsg && payload.kind == Kind::ItemCompleted {
            (Origin::Completed, completed(raw)?)
        } else {
            return None;
        };
        // Tool/context records do not take the place of a dialogue counterpart.
        if message.content.trim().is_empty()
            || !matches!(message.role, Role::User | Role::Assistant)
            || message.dialogue_text.as_deref() == Some("")
        {
            return Some(message);
        }
        let turn = if origin == Origin::Completed {
            payload.turn_id.clone()
        } else {
            self.turn.clone()
        };
        let next_turn = origin == Origin::Raw
            && message.role == Role::User
            && (self.turn.is_none() || self.completed_user);
        let digest = blake3::hash(message.content.as_bytes());
        let counterpart = self.pending.as_ref().is_some_and(|candidate| {
            candidate.origin != origin
                && candidate.role == message.role
                && candidate.digest == digest
                && (candidate.turn == turn || (candidate.next_turn && origin == Origin::Completed))
        });
        if origin == Origin::Completed && message.role == Role::User {
            self.completed_user = true;
        }
        if counterpart {
            // Consume exactly one counterpart. A subsequent identical request is new.
            self.pending = None;
            return None;
        }
        self.pending = Some(Candidate {
            origin,
            role: message.role.clone(),
            digest,
            turn,
            next_turn,
        });
        Some(message)
    }
}

fn completed(raw: &Record) -> Option<ExtractedMessage> {
    let payload = raw.payload.as_ref()?;
    let attachments = match payload.item.as_ref()? {
        CompletedItem::UserMessage { content, .. } => content
            .iter()
            .filter_map(|item| match item {
                UserText::Image { image_url } => Some(image_url.as_deref().map_or_else(
                    || crate::attachments::Source::External {
                        name: "image".into(),
                        media_type: "application/octet-stream".into(),
                    },
                    |url| crate::attachments::url("image", url),
                )),
                UserText::LocalImage { path } => Some(crate::attachments::local_image(path)),
                _ => None,
            })
            .take(crate::attachments::MAX_FILES)
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let (id, role, parts) = match payload.item.as_ref()? {
        CompletedItem::UserMessage { id, content } => (
            id,
            Role::User,
            content
                .iter()
                .filter_map(|item| match item {
                    UserText::Text { text } => Some(text.as_str()),
                    UserText::Other | UserText::Image { .. } | UserText::LocalImage { .. } => None,
                })
                .collect::<Vec<_>>(),
        ),
        CompletedItem::AgentMessage { id, content } => (
            id,
            Role::Assistant,
            content
                .iter()
                .filter_map(|item| match item {
                    AgentText::Text { text } => Some(text.as_str()),
                    AgentText::Other => None,
                })
                .collect::<Vec<_>>(),
        ),
        CompletedItem::Other => return None,
    };
    let turn = payload.turn_id.as_deref()?;
    let projection = parts.join("\n");
    let dialogue_text = (!attachments.is_empty()).then(|| projection.clone());
    let content = if attachments.is_empty() {
        projection
    } else {
        std::iter::once(projection)
            .filter(|s| !s.is_empty())
            .chain(attachments.iter().map(|_| "[Attachment: image]".into()))
            .collect::<Vec<_>>()
            .join("\n")
    };
    Some(ExtractedMessage {
        attachments,
        source_id: Some(format!("completed:{turn}:{id}")),
        role,
        content,
        dialogue_text,
        created_at: parse_timestamp(raw.timestamp.as_deref()),
        session_id: payload.thread_id.clone(),
    })
}

fn extract_codex_response_item(raw: &Record) -> Option<ExtractedMessage> {
    let payload = raw.payload.as_ref()?;
    let (role, content, dialogue_text) = match payload.kind {
        Kind::Message => {
            let (content, projection) = payload.content.as_ref()?.projected()?;
            (
                payload.role.clone().unwrap_or(crate::role::Role::Assistant),
                content,
                projection,
            )
        }
        Kind::FunctionCall | Kind::CustomToolCall => {
            let name = payload.name.as_deref().unwrap_or("unknown");
            let arguments = payload
                .arguments
                .as_ref()
                .or(payload.input.as_ref())
                .map_or_else(String::new, json::stringify);
            (
                crate::role::Role::Assistant,
                format!("Tool {name}: {arguments}"),
                Some(String::new()),
            )
        }
        Kind::FunctionCallOutput | Kind::CustomToolCallOutput => {
            let output = payload.output.as_ref().map(json::stringify)?;
            (
                crate::role::Role::Tool,
                truncate_chars(&output, MAX_TOOL_OUTPUT_CHARS),
                Some(String::new()),
            )
        }
        _ => return None,
    };
    Some(ExtractedMessage {
        attachments: payload
            .content
            .as_ref()
            .map_or_else(Vec::new, json::Content::attachments),
        source_id: payload.id.clone().or_else(|| {
            payload
                .call_id
                .as_ref()
                .map(|id| format!("{}:{id}", payload.kind.as_str()))
        }),
        role,
        content,
        dialogue_text,
        created_at: parse_timestamp(raw.timestamp.as_deref()),
        session_id: None,
    })
}
