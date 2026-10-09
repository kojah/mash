//! User/assistant turns derived from a chat's canonical messages.
//!
//! A turn groups consecutive prompts with the answers that follow them. It is
//! derived from silver alone, so every silver store computes the same turns.
//! Ported from the message-clustering research extractor (`turns.py`):
//! identities and revisions match it byte for byte.
use crate::role::Role;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

/// One canonical message, in ordinal order.
pub(crate) struct Source<'a> {
    pub(crate) id: &'a str,
    pub(crate) ordinal: i64,
    pub(crate) role: &'a Role,
    /// Visible dialogue, falling back to canonical content.
    pub(crate) projection: &'a str,
    pub(crate) context_parent: Option<i64>,
}

/// Why a turn began.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Boundary {
    Start,
    UserReply,
    Branch,
    /// Answers with no prompt before them.
    Orphan,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Turn {
    /// Stable across edits: it depends only on the chat and first message.
    pub(crate) id: String,
    /// Changes whenever a member's text, role or ancestry does.
    pub(crate) revision: String,
    /// Indices into the sources: prompts first, then answers.
    pub(crate) prompts: Vec<usize>,
    pub(crate) answers: Vec<usize>,
    pub(crate) boundary: Boundary,
    /// A later turn followed it; the last turn may still grow.
    pub(crate) closed: bool,
}

impl Turn {
    pub(crate) fn members(&self) -> impl Iterator<Item = usize> + '_ {
        self.prompts.iter().chain(&self.answers).copied()
    }
}

/// Harness and instruction records that are not a semantic prompt.
fn injected(projection: &str) -> bool {
    const PREFIXES: [&str; 5] = [
        "# AGENTS.md",
        "<skill>",
        "[gas-city]",
        "<hook_prompt",
        "<task-notification>",
    ];
    let text = projection.trim_start();
    PREFIXES.iter().any(|prefix| text.starts_with(prefix))
}

fn visible(source: &Source<'_>) -> bool {
    !source.projection.trim().is_empty()
}

struct Activity {
    ordinal: i64,
    parent: Option<i64>,
}

/// Group a chat's messages into turns.
///
/// A user message after any response starts a new turn, as does a native
/// root or a parent outside the current turn. Skipped records (empty or
/// injected) belong to no turn, and a branch parent that points at one
/// resolves to its nearest eligible ancestor. Assistant activity with no
/// visible text still counts as a response on the active branch.
pub(crate) fn extract(chat_id: &str, sources: &[Source<'_>]) -> Vec<Turn> {
    let eligible: HashSet<i64> = sources
        .iter()
        .filter(|source| visible(source) && !injected(source.projection))
        .map(|source| source.ordinal)
        .collect();
    // Linear records follow the previous record; explicit roots stay roots.
    // Silver keeps parents earlier, so an unknown one can only be missing:
    // treat it as a root rather than guess.
    let mut collapsed = HashMap::new();
    let mut previous = -1;
    for source in sources {
        let parent = source.context_parent.unwrap_or(previous);
        let resolved = if parent == -1 || eligible.contains(&parent) {
            parent
        } else {
            collapsed.get(&parent).copied().unwrap_or(-1)
        };
        collapsed.insert(source.ordinal, resolved);
        previous = source.ordinal;
    }
    let parent = |source: &Source<'_>| {
        source
            .context_parent
            .map(|_| collapsed.get(&source.ordinal).copied().unwrap_or(-1))
    };
    let activities: Vec<Activity> = sources
        .iter()
        .filter(|source| *source.role == Role::Assistant && !eligible.contains(&source.ordinal))
        .map(|source| Activity {
            ordinal: source.ordinal,
            parent: parent(source),
        })
        .collect();

    let mut builder = Builder {
        chat_id,
        sources,
        parents: sources.iter().map(parent).collect(),
        turns: Vec::new(),
        prompts: Vec::new(),
        answers: Vec::new(),
        boundary: Boundary::Start,
        responded: false,
    };
    let mut activity = activities.iter().peekable();
    for (index, source) in sources.iter().enumerate() {
        if !visible(source) {
            continue;
        }
        while let Some(next) = activity.next_if(|next| next.ordinal < source.ordinal) {
            let active = builder.active_ordinals();
            if !active.is_empty() && next.parent.is_none_or(|parent| active.contains(&parent)) {
                builder.responded = true;
            }
        }
        if injected(source.projection) {
            continue;
        }
        let active = builder.active_ordinals();
        let native_split = !active.is_empty()
            && builder.parents[index]
                .is_some_and(|parent| parent == -1 || !active.contains(&parent));
        let user = *source.role == Role::User;
        if native_split || (user && (!builder.answers.is_empty() || builder.responded)) {
            builder.finish(true);
            builder.boundary = if native_split {
                Boundary::Branch
            } else {
                Boundary::UserReply
            };
        }
        if user {
            builder.prompts.push(index);
        } else {
            if builder.prompts.is_empty() && builder.answers.is_empty() && !native_split {
                builder.boundary = Boundary::Orphan;
            }
            builder.answers.push(index);
        }
    }
    builder.finish(false);
    builder.turns
}

struct Builder<'s, 'a> {
    chat_id: &'s str,
    sources: &'s [Source<'a>],
    /// Each source's parent after collapsing skipped records.
    parents: Vec<Option<i64>>,
    turns: Vec<Turn>,
    prompts: Vec<usize>,
    answers: Vec<usize>,
    boundary: Boundary,
    responded: bool,
}

impl Builder<'_, '_> {
    fn active_ordinals(&self) -> HashSet<i64> {
        self.prompts
            .iter()
            .chain(&self.answers)
            .map(|&index| self.sources[index].ordinal)
            .collect()
    }

    fn finish(&mut self, closed: bool) {
        let Some(&first) = self.prompts.first().or(self.answers.first()) else {
            return;
        };
        let identity = format!("{}\0{}", self.chat_id, self.sources[first].id);
        let mut revision = Sha256::new();
        for (position, index) in self.prompts.iter().chain(&self.answers).enumerate() {
            let source = &self.sources[*index];
            if position > 0 {
                revision.update(b"\0");
            }
            // The research format prints an absent parent as Python's None.
            let parent = self.parents[*index].map_or_else(|| "None".to_owned(), |p| p.to_string());
            revision.update(
                format!(
                    "{}:{}:{parent}:{}",
                    source.id,
                    source.role.as_str(),
                    source.projection
                )
                .as_bytes(),
            );
        }
        self.turns.push(Turn {
            id: hex::encode(Sha256::digest(identity.as_bytes())),
            revision: hex::encode(revision.finalize()),
            prompts: std::mem::take(&mut self.prompts),
            answers: std::mem::take(&mut self.answers),
            boundary: self.boundary,
            closed,
        });
        self.responded = false;
    }
}

#[cfg(test)]
mod tests;
