//! Translate native UUID ancestry, including non-message intermediate events.
use std::collections::{BTreeMap, BTreeSet};

use super::super::claude::{Lineage, reconcile};
use super::super::json::ParentLink;
use super::super::messages::Messages;
use super::stable_id;
use crate::AppError;

mod replay;

#[derive(Default)]
pub(super) struct Identity {
    parents: BTreeMap<String, Ancestry>,
    pub(super) keys: Vec<Option<String>>,
    agent: Option<String>,
    session: Option<String>,
    sessions: BTreeSet<String>,
    transitions: BTreeSet<usize>,
    replay: replay::Replay,
}

#[derive(PartialEq, Eq)]
enum Ancestry {
    Known(ParentLink),
    Ambiguous,
}

#[derive(Clone)]
struct MessageIdentity {
    id: String,
    // Unknown external ancestors have no position in this physical transcript.
    position: Option<usize>,
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
        self.replay.observe(record, &mut self.parents);
        Ok(())
    }

    pub(super) fn capture_session(&mut self, next: Option<String>) -> Result<(), AppError> {
        let Some(next) = next else {
            return Ok(());
        };
        if self.session.as_ref() == Some(&next) {
            return Ok(());
        }
        if !self.sessions.insert(next.clone()) {
            return Err(AppError::configuration(
                "Claude transcript interleaves session identities",
            ));
        }
        if self.session.is_some() {
            self.transitions.insert(self.keys.len());
        }
        self.session = Some(next);
        Ok(())
    }

    pub(super) fn thread_id(&self, fallback: &str) -> String {
        let session = self.session.as_deref().unwrap_or(fallback);
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
        normalize(
            thread,
            messages,
            self.keys,
            &self.parents,
            &self.transitions,
        )
    }
}

fn normalize(
    thread: &str,
    messages: Messages,
    keys: Vec<Option<String>>,
    parents: &BTreeMap<String, Ancestry>,
    transitions: &BTreeSet<usize>,
) -> Result<(Messages, Lineage), AppError> {
    if messages.len() != keys.len() {
        return Err(AppError::internal(
            "Claude message identities are incomplete",
        ));
    }
    let mut identities = BTreeMap::new();
    for (position, key) in keys.iter().enumerate() {
        if let Some(key) = key {
            identities
                .entry(key.clone())
                .or_insert_with(|| MessageIdentity {
                    id: stable_id("claude-message", &[thread, key]),
                    position: Some(position),
                });
        }
    }
    let mut previous = None;
    let contributions =
        messages
            .into_rows()?
            .zip(keys)
            .enumerate()
            .map(|(ordinal, (message, key))| {
                let mut message = message?;
                let transition = transitions.contains(&ordinal);
                let explicit_parent = key.as_ref().is_some_and(|key| {
                    matches!(
                        parents.get(key),
                        Some(Ancestry::Known(ParentLink::Parent(_)))
                    )
                });
                let parent = if let Some(key) = key {
                    message.id.clone_from(&identities[&key].id);
                    match parents.get(&key) {
                        None | Some(Ancestry::Known(ParentLink::Missing)) => previous.clone(),
                        Some(Ancestry::Known(ParentLink::Root)) => None,
                        Some(Ancestry::Known(ParentLink::Parent(parent))) => {
                            resolve_parent(thread, parent, &identities, parents)?
                        }
                        Some(Ancestry::Ambiguous) => return Err(ambiguous_ancestry(&key)),
                    }
                } else {
                    previous.clone()
                };
                // A native fork copies history under its original session ID before
                // writing the new session's dialogue. Only explicit ancestry into that
                // retained prefix proves this is a fork, rather than concatenated logs.
                if transition
                    && (!explicit_parent
                        || parent
                            .as_ref()
                            .and_then(|parent| parent.position)
                            .is_none_or(|position| position >= ordinal))
                {
                    return Err(AppError::configuration(
                        "Claude session transition is not linked to inherited dialogue",
                    ));
                }
                previous = Some(MessageIdentity {
                    id: message.id.clone(),
                    position: Some(ordinal),
                });
                Ok((message, parent.map(|parent| parent.id)))
            });
    reconcile(contributions)
}

fn resolve_parent(
    thread: &str,
    parent: &str,
    identities: &BTreeMap<String, MessageIdentity>,
    parents: &BTreeMap<String, Ancestry>,
) -> Result<Option<MessageIdentity>, AppError> {
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
            Some(Ancestry::Ambiguous) => return Err(ambiguous_ancestry(current)),
            None => {
                return Ok(Some(MessageIdentity {
                    id: stable_id("claude-message", &[thread, current]),
                    position: None,
                }));
            }
        }
    }
}

fn ambiguous_ancestry(uuid: &str) -> AppError {
    // Native metadata events can be repeated and reparented without being part
    // of dialogue ancestry. Reject ambiguity only if a retained message or its
    // parent chain depends on it; never choose an arbitrary first/last parent.
    AppError::configuration(format!(
        "Claude native message {uuid} has conflicting ancestry"
    ))
}
