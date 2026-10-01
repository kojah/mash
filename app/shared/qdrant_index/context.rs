//! Validate and select canonical ordinal or tree context from one immutable thread.
use super::{AppError, Document, NodeHead, api, failure, point_id};
use crate::shared::protocol;
use crate::storage::Message;
use std::collections::{BTreeMap, BTreeSet};
pub(super) fn messages(
    anchor: &protocol::MessageRecord,
    records: Vec<api::Record<Document>>,
    head: &NodeHead,
    input: &protocol::View,
) -> Result<Vec<Message>, AppError> {
    let mut rows = BTreeMap::new();
    for point in records {
        let d = point.payload;
        let m = d.message;
        if d.version != head.version
            || d.node_id != input.node_id
            || d.conversation_id != anchor.conversation_id
            || m.conversation_id != anchor.conversation_id
            || !m.context.valid(m.ordinal)
            || point.id != point_id(&["message", &head.version, &m.id])
            || rows.insert(m.ordinal, m).is_some()
        {
            return Err(failure());
        }
    }
    if rows.get(&anchor.ordinal).is_none_or(|m| m.id != input.id) {
        return Err(failure());
    }
    let mut selected = BTreeSet::new();
    if anchor.context.is_linear() {
        selected.extend(
            rows.range(
                anchor.ordinal.saturating_sub(i64::from(input.context))
                    ..=anchor.ordinal.saturating_add(i64::from(input.context)),
            )
            .map(|(n, _)| *n),
        );
    } else {
        let mut ordinal = anchor.ordinal;
        for _ in 0..=input.context {
            selected.insert(ordinal);
            let Some(parent) = rows
                .get(&ordinal)
                .and_then(|m| m.context.parent())
                .filter(|p| *p >= 0)
            else {
                break;
            };
            if parent >= ordinal || !rows.contains_key(&parent) {
                return Err(failure());
            }
            ordinal = parent;
        }
        ordinal = anchor.ordinal;
        for _ in 0..input.context {
            let Some(child) = rows.values().find(|m| m.context.parent() == Some(ordinal)) else {
                break;
            };
            ordinal = child.ordinal;
            selected.insert(ordinal);
        }
    }
    Ok(selected
        .into_iter()
        .filter_map(|n| rows.get(&n))
        .map(|m| Message {
            id: m.id.clone(),
            ordinal: m.ordinal,
            role: m.role.clone(),
            content: match input.content {
                crate::storage::MessageContent::Canonical => m.content.clone(),
                crate::storage::MessageContent::SearchProjection => m.projection.clone(),
            },
            created_at: m.created_at,
        })
        .collect())
}
