//! Ordered native bubbles, visibility normalization and stable message identities.
use super::super::text;
use super::{
    AppError, invalid,
    models::{self, Bubble, Kind, Root},
    records,
    store::Store,
    timestamp,
};
use crate::{
    message_context::MessageContext, provider::Provider, role::Role, silver::NormalizedMessage,
};
use std::{collections::BTreeSet, fmt::Write};

fn recorded_time(bubble: &Bubble, header: &models::Header) -> Result<Option<i64>, AppError> {
    let time = bubble
        .created_at
        .as_deref()
        .or(header.created_at.as_deref());
    if let Some(time) = time.filter(|time| !time.is_empty()) {
        return timestamp(Some(
            chrono::DateTime::parse_from_rfc3339(time)
                .map_err(|_| invalid("invalid native ISO timestamp"))?
                .timestamp_millis(),
        ));
    }
    timestamp(bubble.started_at_ms.or(bubble.completed_at_ms))
}

fn normalized(bubble: &Bubble, child: bool) -> Result<(Role, text::Text), AppError> {
    let role = match bubble.kind {
        Kind::Human => Role::User,
        Kind::Assistant => Role::Assistant,
        kind => Role::Other(format!("cursor-message-type-{}", i32::from(kind))),
    };
    let mut text = text::Text {
        canonical: bubble.text.clone(),
        projection: text::projection(&role, &bubble.text),
    };
    if child
        || bubble.is_display_only
        || bubble.is_simulated_msg
        || bubble.simulated_msg_reason.is_some_and(|r| r != 0)
        || bubble
            .simulated_message_metadata
            .as_ref()
            .is_some_and(|m| m.suppress_ui_display)
        || bubble.is_placeholder
        || bubble.service_status_update.is_some()
        || bubble.event_notification.is_some()
        || bubble.capability_type.is_some()
        || bubble.tool_former_data.is_some()
        || bubble.sent_by_agent_id.is_some()
    {
        text.projection.clear();
    }
    if let Some(thinking) = &bubble.thinking {
        let _ = write!(text.canonical, "\nThinking: {}", thinking.text);
    }
    if let Some(tool) = &bubble.tool_former_data {
        let _ = write!(text.canonical, "\nTool {}", tool.id);
        for payload in [&tool.params, &tool.result].into_iter().flatten() {
            text.canonical.push('\n');
            text.canonical.push_str(&payload.to_string());
        }
    }
    if let Some(status) = &bubble.service_status_update {
        let message = match status {
            models::ProtoRecord::Object(status) => status.message.clone(),
            models::ProtoRecord::Json(json) => {
                records::decode::<models::Status>(json.as_bytes())?.message
            }
        };
        let _ = write!(text.canonical, "\nStatus: {message}");
    }
    if let Some(notification) = &bubble.event_notification {
        let _ = write!(text.canonical, "\nNotification: {}", notification.headline);
        if let Some(detail) = &notification.detail {
            let _ = write!(text.canonical, "\n{detail}");
        }
    }
    Ok((role, text))
}

pub(super) fn read(
    store: &mut Store,
    root: &Root,
    id: &str,
) -> Result<Vec<NormalizedMessage>, AppError> {
    let mut ids = BTreeSet::new();
    let mut messages = Vec::new();
    let mut bytes = 0usize;
    for header in &root.full_conversation_headers_only {
        if header.bubble_id.trim().is_empty() || !ids.insert(&header.bubble_id) {
            return Err(invalid("invalid or duplicate reachable bubble identity"));
        }
        if header.is_display_only {
            continue;
        }
        let bubble: Bubble = store.json(&format!("bubbleId:{id}:{}", header.bubble_id))?;
        if bubble.id != header.bubble_id || bubble.kind != header.kind {
            return Err(invalid("bubble and header identities or types disagree"));
        }
        if bubble.is_display_only {
            continue;
        }
        let (role, text) = normalized(&bubble, root.subagent_info.is_some())?;
        let created_at = recorded_time(&bubble, header)?;
        if text.canonical.trim().is_empty() {
            continue;
        }
        bytes = bytes
            .saturating_add(text.canonical.capacity())
            .saturating_add(text.projection.capacity())
            .saturating_add(512);
        if bytes > 32 * 1024 * 1024 || messages.len() == 65_536 {
            return Err(invalid(
                "normalized history exceeds the retained replay limit",
            ));
        }
        messages.push(NormalizedMessage {
            attachments: Vec::new(),
            id: crate::silver::parsers::stable_id(
                "message",
                &[Provider::Cursor.as_str(), "ide", id, &bubble.id],
            ),
            ordinal: i64::try_from(messages.len()).map_err(|_| invalid("too many bubbles"))?,
            context: MessageContext::Linear,
            role,
            content: text.canonical,
            dialogue_text: Some(text.projection),
            created_at,
            tools: crate::tools::Activity::default(),
        });
    }
    Ok(messages)
}
