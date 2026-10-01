//! Translate native UUID ancestry, including non-message intermediate events.
use std::collections::{BTreeMap, BTreeSet};

use super::super::claude::{Lineage, reconcile};
use super::super::json::ParentLink;
use super::super::messages::Messages;
use super::stable_id;
use crate::AppError;

#[derive(Default)]
pub(super) struct Identity {
    parents: BTreeMap<String, Ancestry>,
    pub(super) keys: Vec<Option<String>>,
    agent: Option<String>,
}

#[derive(PartialEq, Eq)]
enum Ancestry {
    Known(ParentLink),
    Ambiguous,
}

impl Identity {
    pub(super) fn observe(&mut self, record: &super::Record) -> Result<(), AppError> {
        if let Some(agent) = record.agent_id.as_ref().filter(|agent| !agent.is_empty()) {
            if self
                .agent
                .as_ref()
                .is_some_and(|previous| previous != agent)
            {
                return Err(AppError::configuration(
                    "Claude transcript contains multiple agent identities",
                ));
            }
            self.agent = Some(agent.clone());
        }
        if let Some(uuid) = record.uuid.as_ref() {
            let parent = Ancestry::Known(record.parent.clone());
            self.parents
                .entry(uuid.clone())
                .and_modify(|known| {
                    if *known != parent {
                        *known = Ancestry::Ambiguous;
                    }
                })
                .or_insert(parent);
        }
        Ok(())
    }

    pub(super) fn thread_id(&self, session: &str) -> String {
        self.agent.as_ref().map_or_else(
            || session.to_owned(),
            |agent| stable_id("claude-agent", &[session, agent]),
        )
    }

    pub(super) fn normalize(
        self,
        thread: &str,
        messages: Messages,
    ) -> Result<(Messages, Lineage), AppError> {
        normalize(thread, messages, self.keys, &self.parents)
    }
}

fn normalize(
    thread: &str,
    messages: Messages,
    keys: Vec<Option<String>>,
    parents: &BTreeMap<String, Ancestry>,
) -> Result<(Messages, Lineage), AppError> {
    if messages.len() != keys.len() {
        return Err(AppError::internal(
            "Claude message identities are incomplete",
        ));
    }
    let identities = keys
        .iter()
        .flatten()
        .map(|key| (key.clone(), stable_id("claude-message", &[thread, key])))
        .collect::<BTreeMap<_, _>>();
    let mut previous = None;
    let contributions = messages.into_rows()?.zip(keys).map(|(message, key)| {
        let mut message = message?;
        let parent = if let Some(key) = key {
            message.id.clone_from(&identities[&key]);
            match parents.get(&key) {
                None | Some(Ancestry::Known(ParentLink::Missing)) => previous.clone(),
                Some(Ancestry::Known(ParentLink::Root)) => None,
                Some(Ancestry::Known(ParentLink::Parent(parent))) => {
                    resolve_parent(thread, parent, &identities, parents)?
                }
                Some(Ancestry::Ambiguous) => return Err(ambiguous_ancestry()),
            }
        } else {
            previous.clone()
        };
        previous = Some(message.id.clone());
        Ok((message, parent))
    });
    reconcile(contributions)
}

fn resolve_parent(
    thread: &str,
    parent: &str,
    identities: &BTreeMap<String, String>,
    parents: &BTreeMap<String, Ancestry>,
) -> Result<Option<String>, AppError> {
    let mut current = parent;
    let mut visited = BTreeSet::new();
    loop {
        if let Some(id) = identities.get(current) {
            return Ok(Some(id.clone()));
        }
        if !visited.insert(current) {
            return Err(AppError::configuration(
                "Claude message ancestry contains a cycle",
            ));
        }
        match parents.get(current) {
            Some(Ancestry::Known(ParentLink::Parent(parent))) => current = parent,
            Some(Ancestry::Known(ParentLink::Root | ParentLink::Missing)) => return Ok(None),
            Some(Ancestry::Ambiguous) => return Err(ambiguous_ancestry()),
            None => return Ok(Some(stable_id("claude-message", &[thread, current]))),
        }
    }
}

fn ambiguous_ancestry() -> AppError {
    // Native metadata events can be repeated and reparented without being part
    // of dialogue ancestry. Reject ambiguity only if a retained message or its
    // parent chain depends on it; never choose an arbitrary first/last parent.
    AppError::configuration("Claude native message has conflicting ancestry")
}
