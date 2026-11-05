//! Projection evidence for dependency scopes. A native chat is one dependency
//! scope because its recipe may use title and branch context; a connector unit
//! already carries its enclosing-object evidence. Both record the same facts.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectionFamily {
    Native,
    Connector,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(into = "String", try_from = "String")]
pub(crate) enum ProjectionScope {
    NativeChat(String),
    ConnectorUnit(String),
}

impl ProjectionScope {
    fn family(&self) -> ProjectionFamily {
        match self {
            Self::NativeChat(_) => ProjectionFamily::Native,
            Self::ConnectorUnit(_) => ProjectionFamily::Connector,
        }
    }
}

impl From<ProjectionScope> for String {
    fn from(scope: ProjectionScope) -> Self {
        match scope {
            ProjectionScope::NativeChat(id) => format!("native-chat:{id}"),
            ProjectionScope::ConnectorUnit(id) => format!("connector-unit:{id}"),
        }
    }
}

impl TryFrom<String> for ProjectionScope {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if let Some(id) = value.strip_prefix("native-chat:") {
            Ok(Self::NativeChat(id.into()))
        } else if let Some(id) = value.strip_prefix("connector-unit:") {
            Ok(Self::ConnectorUnit(id.into()))
        } else {
            Err("unknown projection scope")
        }
    }
}

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
pub(crate) struct Projections(BTreeMap<ProjectionScope, ProjectionEvidence>);

impl Projections {
    pub(crate) fn get(&self, scope: &ProjectionScope) -> Option<&ProjectionEvidence> {
        self.0.get(scope)
    }

    pub(crate) fn insert(&mut self, scope: ProjectionScope, evidence: ProjectionEvidence) {
        self.0.insert(scope, evidence);
    }

    pub(crate) fn remove(&mut self, scope: &ProjectionScope) {
        self.0.remove(scope);
    }

    pub(crate) fn complete(&self) -> bool {
        self.0.values().all(|evidence| evidence.complete)
    }

    pub(crate) fn embeddings(&self) -> u64 {
        self.0.values().map(|evidence| evidence.embeddings).sum()
    }

    pub(crate) fn native(&self) -> impl Iterator<Item = (&String, &ProjectionEvidence)> {
        self.0.iter().filter_map(|(scope, evidence)| match scope {
            ProjectionScope::NativeChat(id) => Some((id, evidence)),
            ProjectionScope::ConnectorUnit(_) => None,
        })
    }

    pub(crate) fn connector_digests(&self) -> BTreeMap<String, String> {
        self.0
            .iter()
            .filter_map(|(scope, evidence)| match scope {
                ProjectionScope::ConnectorUnit(id) => {
                    Some((id.clone(), evidence.fingerprint.clone()))
                }
                ProjectionScope::NativeChat(_) => None,
            })
            .collect()
    }

    /// Remove outdated evidence before writing points, preserving the other
    /// destination-owned scope family. This also makes failed-write reversion safe.
    pub(crate) fn retain_matching(
        &mut self,
        current: &BTreeMap<ProjectionScope, String>,
        family: ProjectionFamily,
    ) {
        self.0.retain(|scope, evidence| {
            scope.family() != family || current.get(scope) == Some(&evidence.fingerprint)
        });
    }

    pub(crate) fn merge_native(&mut self, other: &Self, current: &BTreeMap<String, String>) {
        for (id, evidence) in other.native() {
            let scope = ProjectionScope::NativeChat(id.clone());
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
