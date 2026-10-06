//! Select native context from projected thread topology before fetching bodies.
use super::{AppError, Document, NodeHead, api, failure, point_id};
use crate::message_context::MessageContext;
use crate::records::Message;
use crate::shared::protocol;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
mod tests;

pub(super) const FIELDS: &[&str] = &[
    "version",
    "node_id",
    "conversation_id",
    "message.id",
    "message.conversation_id",
    "message.ordinal",
    "message.context",
];
#[derive(Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Outline {
    version: String,
    node_id: String,
    conversation_id: String,
    message: Entry,
}
#[derive(Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Entry {
    id: String,
    conversation_id: String,
    ordinal: i64,
    #[serde(default)]
    context: MessageContext,
}
impl Entry {
    fn matches(&self, message: &protocol::MessageRecord) -> bool {
        self.id == message.id
            && self.conversation_id == message.conversation_id
            && self.ordinal == message.ordinal
            && self.context == message.context
    }
}

pub(super) fn anchor(
    records: Vec<api::Record<Document>>,
    head: &NodeHead,
    input: &protocol::View,
) -> Result<Option<protocol::MessageRecord>, AppError> {
    if records.len() > 1 {
        return Err(failure());
    }
    let Some(point) = records.into_iter().next() else {
        return Ok(None);
    };
    let d = point.payload;
    let m = d.message;
    let version = head
        .thread_version(&m.conversation_id)
        .ok_or_else(failure)?;
    if point.id != point_id(&["message", version, &input.id])
        || m.id != input.id
        || d.version != version
        || d.node_id != input.node_id
        || d.conversation_id != m.conversation_id
        || !m.context.valid(m.ordinal)
    {
        return Err(failure());
    }
    Ok(Some(m))
}

pub(super) struct Selection {
    entries: BTreeMap<i64, Entry>,
}

// Search asks for radius two. Read its native neighborhood rather than every
// outline in a long thread. Large explicit views retain the full topology path.
// Publication validates the complete thread; reads validate the fetched scope.
pub(super) async fn neighborhood(
    client: &api::Client,
    name: &str,
    filter: api::Filter,
    anchor: &protocol::MessageRecord,
    radius: u32,
) -> Result<Vec<api::Record<Outline>>, AppError> {
    async fn fetch(
        client: &api::Client,
        name: &str,
        mut filter: api::Filter,
        key: &str,
        ordinals: Vec<i64>,
    ) -> Result<Vec<api::Record<Outline>>, AppError> {
        filter.must.push(api::integers(key, ordinals));
        client.scroll_fields(name, filter, FIELDS).await
    }
    if radius > 2 {
        return client.scroll_fields(name, filter, FIELDS).await;
    }
    if anchor.context.is_linear() {
        return fetch(
            client,
            name,
            filter,
            "message.ordinal",
            (anchor.ordinal.saturating_sub(i64::from(radius))
                ..=anchor.ordinal.saturating_add(i64::from(radius)))
                .collect(),
        )
        .await;
    }
    let mut records = fetch(
        client,
        name,
        filter.clone(),
        "message.ordinal",
        vec![anchor.ordinal],
    )
    .await?;
    let mut context = anchor.context;
    // The existing selector also checks the last selected ancestor's parent.
    for _ in 0..=radius {
        let Some(parent) = context.parent().filter(|p| *p >= 0) else {
            break;
        };
        let parents = fetch(
            client,
            name,
            filter.clone(),
            "message.ordinal",
            vec![parent],
        )
        .await?;
        context = parents
            .first()
            .map_or(MessageContext::Root, |p| p.payload.message.context);
        records.extend(parents);
    }
    let mut ordinal = anchor.ordinal;
    for _ in 0..radius {
        let children = fetch(
            client,
            name,
            filter.clone(),
            "message.context.ordinal",
            vec![ordinal],
        )
        .await?;
        let child = children.iter().map(|p| p.payload.message.ordinal).min();
        records.extend(children);
        let Some(child) = child else { break };
        ordinal = child;
    }
    // Ancestor and child reads can overlap. Do not hide inconsistent payloads.
    let mut unique = BTreeMap::new();
    for point in records {
        if let Some(previous) = unique.insert(point.id.clone(), point.payload)
            && unique.get(&point.id) != Some(&previous)
        {
            return Err(failure());
        }
    }
    Ok(unique
        .into_iter()
        .map(|(id, payload)| api::Record { id, payload })
        .collect())
}

#[tracing::instrument(name = "select_shared_context", skip_all, fields(thread_messages = records.len(), selected_messages = tracing::field::Empty))]
pub(super) fn select(
    anchor: &protocol::MessageRecord,
    records: Vec<api::Record<Outline>>,
    head: &NodeHead,
    input: &protocol::View,
) -> Result<Selection, AppError> {
    let mut rows = BTreeMap::new();
    let mut ids = BTreeSet::new();
    for point in records {
        let d = point.payload;
        let m = d.message;
        let version = head
            .thread_version(&m.conversation_id)
            .ok_or_else(failure)?;
        if d.version != version
            || d.node_id != input.node_id
            || d.conversation_id != anchor.conversation_id
            || m.conversation_id != anchor.conversation_id
            || !m.context.valid(m.ordinal)
            || point.id != point_id(&["message", version, &m.id])
            || !ids.insert(point.id)
            || rows.insert(m.ordinal, m).is_some()
        {
            return Err(failure());
        }
    }
    if rows.get(&anchor.ordinal).is_none_or(|m| !m.matches(anchor)) {
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
    let entries = selected
        .into_iter()
        .map(|n| rows.remove(&n).map(|entry| (n, entry)).ok_or_else(failure))
        .collect::<Result<_, _>>()?;
    let result = Selection { entries };
    tracing::Span::current().record("selected_messages", result.entries.len());
    Ok(result)
}

impl Selection {
    pub(super) fn ids(&self, version: &str) -> Vec<String> {
        self.entries
            .values()
            .map(|m| point_id(&["message", version, &m.id]))
            .collect()
    }

    pub(super) fn messages(
        self,
        records: Vec<api::Record<Document>>,
        head: &NodeHead,
        input: &protocol::View,
    ) -> Result<Vec<Message>, AppError> {
        if records.len() != self.entries.len() {
            return Err(failure());
        }
        let mut output = BTreeMap::new();
        for point in records {
            let d = point.payload;
            let m = d.message;
            let version = head
                .thread_version(&m.conversation_id)
                .ok_or_else(failure)?;
            if d.version != version
                || d.node_id != input.node_id
                || d.conversation_id != m.conversation_id
                || point.id != point_id(&["message", version, &m.id])
                || self.entries.get(&m.ordinal).is_none_or(|e| !e.matches(&m))
                || output.contains_key(&m.ordinal)
            {
                return Err(failure());
            }
            output.insert(
                m.ordinal,
                Message {
                    attachments: m.attachments,
                    id: m.id,
                    ordinal: m.ordinal,
                    role: m.role,
                    content: match input.content {
                        crate::records::MessageContent::Canonical => m.content,
                        crate::records::MessageContent::SearchProjection => m.projection,
                    },
                    created_at: m.created_at,
                },
            );
        }
        Ok(output.into_values().collect())
    }
}
