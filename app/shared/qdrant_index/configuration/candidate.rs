//! Durable replacement ownership; candidate metadata never selects search data.
use super::super::{AppError, Identity, NodeHead};
use super::{Build, Configuration, Phase, Store, conflict, failure, valid_version};
use crate::shared::silver_revision;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::shared) struct Candidate {
    schema_version: u32,
    pub(in crate::shared) id: String,
    created_at: chrono::DateTime<chrono::Utc>,
    base_epoch: String,
    pub(in crate::shared) build: Build,
    generation: String,
    dimensions: usize,
    nodes: BTreeMap<String, Pin>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Pin {
    publication: String,
    silver: silver_revision::State,
}

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::shared) struct Status {
    pub(in crate::shared) candidate: Candidate,
    pub(in crate::shared) validity: Validity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(in crate::shared) enum Validity {
    Pinned,
    Invalidated,
}

fn pins(nodes: &BTreeMap<String, NodeHead>) -> Result<BTreeMap<String, Pin>, AppError> {
    nodes
        .iter()
        .map(|(id, node)| {
            let silver = node
                .head
                .as_ref()
                .and_then(|h| h.silver_revision.clone())
                .ok_or_else(|| {
                    AppError::configuration(
                        "candidate staging requires recorded silver revisions for every node",
                    )
                })?;
            silver.validate()?;
            Ok((
                id.clone(),
                Pin {
                    publication: node.version.clone(),
                    silver,
                },
            ))
        })
        .collect()
}

impl Candidate {
    pub(super) fn validate(&self, active: &Configuration) -> Result<(), AppError> {
        if self.schema_version != 1
            || !valid_version(&self.id)
            || !valid_version(&self.base_epoch)
            || active.schema_version != 2
            || active.phase != Some(Phase::Ready)
            || self.generation != active.generation
            || self.dimensions != active.dimensions
            || self.build == active.build
            || self.nodes.len() > 512
        {
            return Err(failure());
        }
        for (id, pin) in &self.nodes {
            crate::shared::validation::valid_id(id).map_err(|_| failure())?;
            if pin.publication.is_empty() || pin.publication.len() > 4096 {
                return Err(failure());
            }
            pin.silver.validate()?;
        }
        Ok(())
    }

    fn current(&self, active: &Configuration) -> Result<bool, AppError> {
        let nodes = active.nodes.as_ref().ok_or_else(failure)?;
        Ok(self.base_epoch == active.version
            && self.nodes.len() == nodes.len()
            && self.nodes.iter().all(|(id, pin)| {
                nodes.get(id).is_some_and(|node| {
                    node.version == pin.publication
                        && node.head.as_ref().and_then(|h| h.silver_revision.as_ref())
                            == Some(&pin.silver)
                })
            }))
    }
}

impl Store {
    /// Read-only validation before staging any configured group.
    pub(in crate::shared) async fn stage_base(
        &self,
        identity: &Identity,
    ) -> Result<Build, AppError> {
        Ok(self.candidate_base(identity).await?.build)
    }

    async fn candidate_base(&self, identity: &Identity) -> Result<Configuration, AppError> {
        let previous = self.group_configuration(identity).await?.ok_or_else(|| {
            AppError::configuration("stage-gold requires an existing active group")
        })?;
        if previous.schema_version != 2
            || previous.phase != Some(Phase::Ready)
            || previous.generation != identity.generation
            || previous.dimensions != identity.dimensions
        {
            return Err(AppError::configuration(
                "stage-gold requires a ready group with unchanged model space; migrate or rebuild it first",
            ));
        }
        if previous.build == self.build {
            return Err(AppError::configuration(
                "the requested gold build is already active",
            ));
        }
        let _ = pins(previous.nodes.as_ref().ok_or_else(failure)?)?;
        if let Some(candidate) = &previous.candidate
            && (candidate.build != self.build || !candidate.current(&previous)?)
        {
            return Err(conflict(
                "a different or invalidated gold candidate exists; explicitly discard it first",
            ));
        }
        Ok(previous)
    }
    /// Explicit operator staging: pin metadata only, preserve the active recipe.
    pub(in crate::shared) async fn stage_gold(
        &self,
        identity: &Identity,
    ) -> Result<Build, AppError> {
        let previous = self.candidate_base(identity).await?;
        if previous.candidate.is_some() {
            return Ok(previous.build);
        }
        let candidate = Candidate {
            schema_version: 1,
            id: super::super::publish::staging_version()?,
            created_at: chrono::Utc::now(),
            base_epoch: previous.version.clone(),
            build: self.build.clone(),
            generation: identity.generation.clone(),
            dimensions: identity.dimensions,
            nodes: pins(previous.nodes.as_ref().ok_or_else(failure)?)?,
        };
        let id = candidate.id.clone();
        let mut proposed = previous.clone();
        proposed.candidate = Some(candidate);
        proposed.publication = Some(super::super::publish::staging_version()?);
        let result = self
            .replace_configuration(identity, proposed, Some(&previous))
            .await;
        let observed = self
            .group_configuration(identity)
            .await?
            .ok_or_else(failure)?;
        if observed.version == previous.version
            && observed.candidate.as_ref().is_some_and(|c| c.id == id)
        {
            // An acknowledgement may have been lost after a committed metadata write.
            return Ok(observed.build);
        }
        result?;
        Err(conflict(
            "shared catalogue changed while staging gold; retry explicitly",
        ))
    }

    pub(in crate::shared) async fn candidate_status(
        &self,
        identity: &Identity,
    ) -> Result<Option<Status>, AppError> {
        let active = self.active_configuration(identity).await?;
        active
            .candidate
            .as_ref()
            .map(|candidate| {
                Ok(Status {
                    candidate: candidate.clone(),
                    validity: if candidate.current(&active)? {
                        Validity::Pinned
                    } else {
                        Validity::Invalidated
                    },
                })
            })
            .transpose()
    }

    /// Discard only the observed candidate. Competing ownership is never erased.
    pub(in crate::shared) async fn discard_candidate(
        &self,
        identity: &Identity,
    ) -> Result<(), AppError> {
        let previous = self.active_configuration(identity).await?;
        let Some(candidate) = &previous.candidate else {
            return Ok(());
        };
        let mut proposed = previous.clone();
        proposed.candidate = None;
        proposed.publication = Some(super::super::publish::staging_version()?);
        let result = self
            .replace_configuration(identity, proposed, Some(&previous))
            .await;
        let observed = self.active_configuration(identity).await?;
        if observed
            .candidate
            .as_ref()
            .is_none_or(|c| c.id != candidate.id)
        {
            return Ok(());
        }
        result?;
        Err(conflict(
            "gold candidate discard was not observed; retry explicitly",
        ))
    }
}
