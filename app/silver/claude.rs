//! Native Claude identities and deterministic reconciliation of source contributions.
use std::collections::{BTreeMap, BTreeSet};

use super::NormalizedMessage;
use super::messages::{Builder, Messages};
use crate::AppError;
use crate::message_context::MessageContext;

pub(crate) type Lineage = BTreeMap<String, Option<String>>;

/// Reconcile matching native identities and order every retained branch after
/// its parent. Missing ancestors remain roots until another contribution supplies
/// them. Timestamp and identity break ties; source arrival order never does.
pub(crate) fn reconcile(
    contributions: impl IntoIterator<Item = Result<(NormalizedMessage, Option<String>), AppError>>,
) -> Result<(Messages, Lineage), AppError> {
    struct Node {
        position: usize,
        created_at: Option<i64>,
        parent: Option<String>,
    }
    let mut payloads = Builder::default();
    let mut nodes = BTreeMap::<String, Node>::new();
    // Copies retained from an earlier interpretation may lack tool activity
    // that a newer copy of the same message carries; the activity is kept.
    let mut tools = BTreeMap::<String, crate::tools::Activity>::new();
    for contribution in contributions {
        let (message, parent) = contribution?;
        if let Some(node) = nodes.get(&message.id) {
            let previous = payloads
                .get(node.position)?
                .ok_or_else(|| AppError::internal("Claude reconciliation lost a payload"))?;
            if previous.role != message.role
                || previous.content != message.content
                || previous.dialogue_text != message.dialogue_text
                || previous.created_at != message.created_at
                || node.parent != parent
            {
                return Err(AppError::configuration(
                    "Claude native message identity has conflicting content or ancestry",
                ));
            }
            if previous.tools.is_empty() && !message.tools.is_empty() {
                tools.insert(message.id, message.tools);
            }
        } else {
            let id = message.id.clone();
            let created_at = message.created_at;
            let position = payloads.push(message)?;
            nodes.insert(
                id,
                Node {
                    position,
                    created_at,
                    parent,
                },
            );
        }
    }
    let mut children = BTreeMap::<String, Vec<String>>::new();
    let mut ready = BTreeSet::new();
    for (id, node) in &nodes {
        if let Some(parent) = node
            .parent
            .as_ref()
            .filter(|parent| nodes.contains_key(*parent))
        {
            children.entry(parent.clone()).or_default().push(id.clone());
        } else {
            ready.insert((node.created_at, id.clone()));
        }
    }
    let mut positions = BTreeMap::new();
    let mut lineage = Lineage::new();
    let mut messages = Builder::default();
    while let Some((_, id)) = ready.pop_first() {
        let node = nodes.remove(&id).ok_or_else(|| {
            AppError::internal("Claude reconciliation selected an unknown message")
        })?;
        let mut message = payloads
            .get(node.position)?
            .ok_or_else(|| AppError::internal("Claude reconciliation lost a payload"))?
            .into_owned();
        let parent = node.parent;
        if let Some(activity) = tools.remove(&id) {
            message.tools = activity;
        }
        message.ordinal = i64::try_from(messages.len())
            .map_err(|_| AppError::internal("Claude conversation contains too many messages"))?;
        message.context = parent
            .as_ref()
            .filter(|id| positions.contains_key(*id))
            .map_or(MessageContext::Root, |id| MessageContext::Parent {
                id: id.clone(),
            });
        positions.insert(id.clone(), message.ordinal);
        lineage.insert(id.clone(), parent);
        messages.push(message)?;
        for child in children.remove(&id).unwrap_or_default() {
            let node = nodes
                .get(&child)
                .ok_or_else(|| AppError::internal("Claude reconciliation lost a child message"))?;
            ready.insert((node.created_at, child));
        }
    }
    if !nodes.is_empty() {
        return Err(AppError::configuration(
            "Claude message ancestry contains a cycle",
        ));
    }
    Ok((messages.finish()?, lineage))
}
