//! Translate native UUID ancestry, including non-message intermediate events.
use std::collections::BTreeSet;

use super::super::claude::{Lineage, reconcile};
use super::super::json::ParentLink;
use super::super::messages::Messages;
use super::stable_id;
use crate::AppError;

mod keys;
mod parents;
mod replay;
use keys::Keys;
use parents::Parents;

#[derive(Default)]
pub struct Identity {
    parents: Parents,
    keys: Keys,
    agent: Option<String>,
    conflicting_agents: bool,
    session: Option<String>,
    sessions: BTreeSet<String>,
    transitions: BTreeSet<usize>,
    replay: replay::Replay,
}

#[derive(Clone, Debug, PartialEq, Eq)]
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
    pub fn push_key(&mut self, key: Option<String>) -> Result<(), AppError> {
        self.keys.push(key)
    }

    /// An unreadable record breaks any replay in progress.
    pub fn interrupt(&mut self) {
        self.replay.clear();
    }

    pub fn observe(&mut self, record: &super::Record) -> Result<(), AppError> {
        if let Some(agent) = record.agent_id.as_ref().filter(|agent| !agent.is_empty()) {
            if self
                .agent
                .as_ref()
                .is_some_and(|previous| previous != agent)
            {
                self.conflicting_agents = true;
            }
            self.agent.get_or_insert_with(|| agent.clone());
        }
        self.replay.observe(record, &mut self.parents)?;
        Ok(())
    }

    pub fn capture_session(&mut self, next: Option<String>) -> Result<(), AppError> {
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

    pub fn thread_id(&self, fallback: &str) -> String {
        let session = self.session.as_deref().unwrap_or(fallback);
        self.agent.as_ref().map_or_else(
            || session.to_owned(),
            |agent| stable_id("claude-agent", &[session, agent]),
        )
    }

    pub fn normalize(
        self,
        thread: &str,
        messages: Messages,
    ) -> Result<(Messages, Lineage), AppError> {
        // Event-only workflow journals can name many agents without containing
        // a chat. Keep transcript identity rules once dialogue is retained.
        if self.conflicting_agents {
            return Err(AppError::configuration(
                "Claude transcript contains multiple agent identities",
            ));
        }
        normalize(
            thread,
            messages,
            &self.keys,
            &self.parents,
            &self.transitions,
        )
    }
}

fn normalize(
    thread: &str,
    messages: Messages,
    keys: &Keys,
    parents: &Parents,
    transitions: &BTreeSet<usize>,
) -> Result<(Messages, Lineage), AppError> {
    if messages.len() != keys.len() {
        return Err(AppError::internal(
            "Claude message identities are incomplete",
        ));
    }
    let mut previous = None;
    let contributions = messages.into_rows()?.enumerate().map(|(ordinal, message)| {
        let mut message = message?;
        let key = keys.get(ordinal)?;
        let transition = transitions.contains(&ordinal);
        let ancestry = key
            .as_ref()
            .map(|key| parents.get(key))
            .transpose()?
            .flatten();
        let explicit_parent =
            key.is_some() && matches!(ancestry, Some(Ancestry::Known(ParentLink::Parent(_))));
        let parent = if let Some(key) = key {
            message.id = stable_id("claude-message", &[thread, &key]);
            match ancestry {
                None | Some(Ancestry::Known(ParentLink::Missing)) => previous.clone(),
                Some(Ancestry::Known(ParentLink::Root)) => None,
                Some(Ancestry::Known(ParentLink::Parent(parent))) => {
                    resolve_parent(thread, &parent, keys, parents)?
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
    identities: &Keys,
    parents: &Parents,
) -> Result<Option<MessageIdentity>, AppError> {
    let mut current = parent.to_owned();
    // Brent cycle detection: retain only the current UUID and a checkpoint,
    // rather than every operational event traversed. Walk the same parent path
    // in order, still stopping on dialogue, unknown, root or ambiguous ancestry.
    let mut checkpoint = current.clone();
    let mut power = 1_usize;
    let mut distance = 0_usize;
    loop {
        if let Some(position) = identities.first(&current)? {
            return Ok(Some(MessageIdentity {
                id: stable_id("claude-message", &[thread, &current]),
                position: Some(position),
            }));
        }
        match parents.get(&current)? {
            Some(Ancestry::Known(ParentLink::Parent(parent))) => current = parent,
            Some(Ancestry::Known(ParentLink::Root | ParentLink::Missing)) => return Ok(None),
            Some(Ancestry::Ambiguous) => return Err(ambiguous_ancestry(&current)),
            None => {
                return Ok(Some(MessageIdentity {
                    id: stable_id("claude-message", &[thread, &current]),
                    position: None,
                }));
            }
        }
        distance = distance.saturating_add(1);
        if current == checkpoint {
            return Err(AppError::configuration(
                "Claude message ancestry contains a cycle",
            ));
        }
        if distance == power {
            checkpoint.clone_from(&current);
            power = power.saturating_mul(2);
            distance = 0;
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

#[cfg(test)]
mod traversal_tests;
