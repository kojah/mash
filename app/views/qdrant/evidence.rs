//! Per-session evidence that retrieval views match canonical agent history.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectionEvidence {
    /// Fingerprint of every silver field on which this scope depends.
    pub(crate) fingerprint: String,
    pub(crate) embeddings: u64,
    /// Every dense-eligible unit in this scope has its required vector.
    pub(crate) complete: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub(crate) struct Projections(BTreeMap<String, ProjectionEvidence>);

impl Projections {
    pub(crate) fn get(&self, scope: &str) -> Option<&ProjectionEvidence> {
        self.0.get(scope)
    }

    pub(crate) fn insert(&mut self, scope: String, evidence: ProjectionEvidence) {
        self.0.insert(scope, evidence);
    }

    pub(crate) fn remove(&mut self, scope: &str) {
        self.0.remove(scope);
    }

    pub(crate) fn complete(&self) -> bool {
        self.0.values().all(|evidence| evidence.complete)
    }

    pub(crate) fn embeddings(&self) -> u64 {
        self.0.values().map(|evidence| evidence.embeddings).sum()
    }

    pub(crate) fn sessions(&self) -> impl Iterator<Item = (&String, &ProjectionEvidence)> {
        self.0.iter()
    }

    /// Discard evidence that no longer describes the committed session.
    pub(crate) fn retain_matching(&mut self, current: &BTreeMap<String, String>) {
        self.0
            .retain(|id, evidence| current.get(id) == Some(&evidence.fingerprint));
    }

    pub(crate) fn merge_matching(&mut self, other: &Self, current: &BTreeMap<String, String>) {
        for (id, evidence) in other.sessions() {
            let scope = id.clone();
            if self
                .get(&scope)
                .is_none_or(|existing| current.get(id) != Some(&existing.fingerprint))
                && current.get(id) == Some(&evidence.fingerprint)
            {
                self.insert(scope, evidence.clone());
            }
        }
    }
}
