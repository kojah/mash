//! Native Claude identities and deterministic reconciliation of source contributions.
use std::collections::{BTreeMap, BTreeSet};

use super::NormalizedMessage;
use crate::AppError;
use crate::message_context::MessageContext;

pub(crate) type Lineage = BTreeMap<String, Option<String>>;

/// Reconcile matching native identities and order every retained branch after
/// its parent. Missing ancestors remain roots until another contribution supplies
/// them. Timestamp and identity break ties; source arrival order never does.
pub(crate) fn reconcile(
    contributions: impl IntoIterator<Item = (NormalizedMessage, Option<String>)>,
) -> Result<(Vec<NormalizedMessage>, Lineage), AppError> {
    let mut nodes = BTreeMap::<String, (NormalizedMessage, Option<String>)>::new();
    for (message, parent) in contributions {
        if let Some((previous, previous_parent)) = nodes.get(&message.id) {
            if previous.role != message.role
                || previous.content != message.content
                || previous.search_projection != message.search_projection
                || previous.created_at != message.created_at
                || previous_parent != &parent
            {
                return Err(AppError::configuration(
                    "Claude native message identity has conflicting content or ancestry",
                ));
            }
        } else {
            nodes.insert(message.id.clone(), (message, parent));
        }
    }
    let mut children = BTreeMap::<String, Vec<String>>::new();
    let mut ready = BTreeSet::new();
    for (id, (message, parent)) in &nodes {
        if let Some(parent) = parent.as_ref().filter(|parent| nodes.contains_key(*parent)) {
            children.entry(parent.clone()).or_default().push(id.clone());
        } else {
            ready.insert((message.created_at, id.clone()));
        }
    }
    let mut positions = BTreeMap::new();
    let mut lineage = Lineage::new();
    let mut messages = Vec::with_capacity(nodes.len());
    while let Some((_, id)) = ready.pop_first() {
        let (mut message, parent) = nodes.remove(&id).ok_or_else(|| {
            AppError::internal("Claude reconciliation selected an unknown message")
        })?;
        message.ordinal = i64::try_from(messages.len())
            .map_err(|_| AppError::internal("Claude conversation contains too many messages"))?;
        message.context = parent
            .as_ref()
            .and_then(|id| positions.get(id))
            .map_or(MessageContext::Root, |&ordinal| MessageContext::Parent {
                ordinal,
            });
        positions.insert(id.clone(), message.ordinal);
        lineage.insert(id.clone(), parent);
        messages.push(message);
        for child in children.remove(&id).unwrap_or_default() {
            let (message, _) = nodes
                .get(&child)
                .ok_or_else(|| AppError::internal("Claude reconciliation lost a child message"))?;
            ready.insert((message.created_at, child));
        }
    }
    if !nodes.is_empty() {
        return Err(AppError::configuration(
            "Claude message ancestry contains a cycle",
        ));
    }
    Ok((messages, lineage))
}
