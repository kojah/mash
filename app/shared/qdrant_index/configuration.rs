//! Durable group authority. Activation is explicit; stale processes fail closed.
use super::{AppError, Identity, PointKind, Store, api, collection, failure, point_id};
use crate::gold::config::Build;
use crate::shared::validation::conflict;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod catalogue;
mod migration;
pub(super) use catalogue::ready_nodes as catalogue_nodes;

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(super) enum Phase {
    Migrating,
    Ready,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Configuration {
    pub(super) kind: PointKind,
    pub(super) schema_version: u32,
    pub(super) version: String,
    pub(super) build: Build,
    pub(super) generation: String,
    pub(super) dimensions: usize,
    #[serde(default)]
    pub(super) vector_search: super::VectorIndex,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) publication: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) phase: Option<Phase>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) nodes: Option<BTreeMap<String, super::NodeHead>>,
}

impl Configuration {
    pub(super) fn matches(&self, store: &Store, identity: &Identity) -> bool {
        self.build == store.build
            && self.generation == identity.generation
            && self.dimensions == identity.dimensions
    }

    fn validate(&self) -> Result<(), AppError> {
        self.vector_search.validate().map_err(|_| failure())?;
        if self.kind != PointKind::Configuration
            || !valid_version(&self.version)
            || self.generation.is_empty()
            || self.generation.len() > 1024
            || self.generation.chars().any(char::is_control)
            || !(1..=2048).contains(&self.dimensions)
        {
            return Err(failure());
        }
        match self.schema_version {
            1 if self.publication.is_none() && self.phase.is_none() && self.nodes.is_none() => {}
            2 if self.publication.as_deref().is_some_and(valid_version)
                && self.phase.is_some()
                && self.nodes.is_some() =>
            {
                let nodes = self.nodes.as_ref().ok_or_else(failure)?;
                if nodes.len() > 512 || (self.phase == Some(Phase::Migrating) && !nodes.is_empty())
                {
                    return Err(failure());
                }
                for (id, node) in nodes {
                    catalogue::validate_node(id, node)?;
                }
            }
            _ => return Err(failure()),
        }
        Ok(())
    }
}

fn valid_version(version: &str) -> bool {
    version.len() == 64 && version.bytes().all(|b| b.is_ascii_hexdigit())
}

impl Store {
    pub(in crate::shared) fn with_query(mut self, query: super::VectorIndex) -> Self {
        self.vector_index = query;
        self
    }

    /// Change only the persisted retrieval default. Keep the build identity and
    /// immutable node references; concurrent publishers participate in the same CAS.
    pub(in crate::shared) async fn select_query(
        &self,
        identity: &Identity,
        activate: bool,
    ) -> Result<(), AppError> {
        self.vector_index.validate()?;
        let previous = self.active_configuration(identity).await?;
        if previous.vector_search == self.vector_index {
            return Ok(());
        }
        if !activate {
            return Err(AppError::configuration(
                "server query defaults conflict with the persisted group policy; use --activate-query for an intentional change",
            ));
        }
        let mut proposed = previous.clone();
        proposed.vector_search = self.vector_index;
        proposed.publication = Some(super::publish::staging_version()?);
        let result = self
            .replace_configuration(identity, proposed, Some(&previous))
            .await;
        let observed = self.active_configuration(identity).await?;
        if observed.version == previous.version && observed.vector_search == self.vector_index {
            return Ok(());
        }
        result?;
        Err(conflict(
            "concurrent shared query activation; retry startup",
        ))
    }

    pub(super) async fn group_configuration(
        &self,
        identity: &Identity,
    ) -> Result<Option<Configuration>, AppError> {
        let id = point_id(&["gold-configuration"]);
        let mut points = self
            .api
            .retrieve::<Configuration>(
                &collection(&identity.group_id, true),
                std::slice::from_ref(&id),
            )
            .await?;
        if points.len() > 1 {
            return Err(failure());
        }
        points
            .pop()
            .map(|point| {
                if point.id != id {
                    return Err(failure());
                }
                point.payload.validate()?;
                Ok(point.payload)
            })
            .transpose()
    }

    /// Bootstrap once, or explicitly replace the observed group configuration.
    /// A competing selection is never overwritten by an unconditional upsert.
    pub(in crate::shared) async fn select_gold(
        &self,
        identity: &Identity,
        activate: bool,
    ) -> Result<String, AppError> {
        let previous = self.group_configuration(identity).await?;
        if let Some(previous) = &previous {
            if previous.schema_version == 1 || previous.phase == Some(Phase::Migrating) {
                return self
                    .migrate_catalogue(identity, previous.clone(), activate)
                    .await;
            }
            if previous.matches(self, identity) {
                return Ok(previous.version.clone());
            }
            if !activate {
                return Err(AppError::configuration(
                    "server gold settings conflict with the persisted group configuration; use --activate-gold for an intentional switch",
                ));
            }
        }
        let configuration = Configuration {
            kind: PointKind::Configuration,
            schema_version: 2,
            version: super::publish::staging_version()?,
            build: self.build.clone(),
            generation: identity.generation.clone(),
            dimensions: identity.dimensions,
            vector_search: previous
                .as_ref()
                .map_or(self.vector_index, |p| p.vector_search),
            publication: Some(super::publish::staging_version()?),
            phase: Some(Phase::Ready),
            nodes: Some(
                previous
                    .as_ref()
                    .and_then(|p| p.nodes.clone())
                    .unwrap_or_default(),
            ),
        };
        configuration.validate()?;
        let result = self
            .replace_configuration(identity, configuration, previous.as_ref())
            .await;
        // An HTTP failure can follow a committed write. Observe durable state;
        // accepting the same winning configuration is safe and idempotent.
        let current = self
            .group_configuration(identity)
            .await?
            .ok_or_else(failure)?;
        if current.matches(self, identity) && current.phase == Some(Phase::Ready) {
            return Ok(current.version);
        }
        result?;
        Err(conflict("concurrent shared gold activation; retry startup"))
    }

    pub(super) async fn replace_configuration(
        &self,
        identity: &Identity,
        configuration: Configuration,
        previous: Option<&Configuration>,
    ) -> Result<(), AppError> {
        configuration.validate()?;
        let input = api::Upsert {
            points: vec![api::Point {
                id: point_id(&["gold-configuration"]),
                vector: api::Vectors::default(),
                payload: configuration,
            }],
            update_filter: previous.map(|previous| api::Filter {
                must: if let Some(publication) = &previous.publication {
                    vec![
                        api::matches("version", vec![previous.version.clone()]),
                        api::matches("publication", vec![publication.clone()]),
                    ]
                } else {
                    vec![api::matches("version", vec![previous.version.clone()])]
                },
                ..api::Filter::default()
            }),
            update_mode: Some(if previous.is_some() {
                api::UpdateMode::UpdateOnly
            } else {
                api::UpdateMode::InsertOnly
            }),
        };
        self.api
            .upsert(&collection(&identity.group_id, true), &input)
            .await
    }

    pub(super) async fn gold_configuration(
        &self,
        identity: &Identity,
    ) -> Result<crate::shared::GroupGold, AppError> {
        let current = self.active_configuration(identity).await?;
        Ok(crate::shared::GroupGold {
            version: current.version,
            build: current.build,
            generation: current.generation,
            dimensions: current.dimensions,
            vector_search: current.vector_search,
        })
    }

    pub(super) async fn active_configuration(
        &self,
        identity: &Identity,
    ) -> Result<Configuration, AppError> {
        let current = self.group_configuration(identity).await?.ok_or_else(|| {
            conflict("shared group gold configuration is missing; restart the API")
        })?;
        if !current.matches(self, identity) {
            return Err(conflict(
                "shared group gold configuration changed; restart this API with the active settings",
            ));
        }
        if current.schema_version != 2 || current.phase != Some(Phase::Ready) {
            return Err(conflict(
                "shared catalogue migration is incomplete; restart with --activate-gold",
            ));
        }
        Ok(current)
    }

    pub(super) async fn gold_version(&self, identity: &Identity) -> Result<String, AppError> {
        Ok(self.gold_configuration(identity).await?.version)
    }

    pub(super) async fn unchanged_gold(
        &self,
        identity: &Identity,
        version: &str,
    ) -> Result<(), AppError> {
        if self.gold_version(identity).await? != version {
            return Err(conflict(
                "shared gold activation changed during publication; retry indexing",
            ));
        }
        Ok(())
    }
}
