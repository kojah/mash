//! Durable group authority. Activation is explicit; stale processes fail closed.
use super::{AppError, Identity, PointKind, Store, api, collection, failure, point_id};
use crate::gold::config::Build;
use crate::shared::validation::conflict;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Configuration {
    kind: PointKind,
    schema_version: u32,
    version: String,
    build: Build,
    generation: String,
    dimensions: usize,
}

impl Configuration {
    fn matches(&self, store: &Store, identity: &Identity) -> bool {
        self.build == store.build
            && self.generation == identity.generation
            && self.dimensions == identity.dimensions
    }

    fn validate(&self) -> Result<(), AppError> {
        if self.kind != PointKind::Configuration
            || self.schema_version != 1
            || self.version.len() != 64
            || !self.version.bytes().all(|b| b.is_ascii_hexdigit())
            || self.generation.is_empty()
            || self.generation.len() > 1024
            || self.generation.chars().any(char::is_control)
            || !(1..=2048).contains(&self.dimensions)
        {
            return Err(failure());
        }
        Ok(())
    }
}

impl Store {
    async fn group_configuration(
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
            schema_version: 1,
            version: super::publish::staging_version()?,
            build: self.build.clone(),
            generation: identity.generation.clone(),
            dimensions: identity.dimensions,
        };
        configuration.validate()?;
        let input = api::Upsert {
            points: vec![api::Point {
                id: point_id(&["gold-configuration"]),
                vector: api::Vectors::default(),
                payload: configuration,
            }],
            update_filter: previous.as_ref().map(|previous| api::Filter {
                must: vec![api::matches("version", vec![previous.version.clone()])],
                ..api::Filter::default()
            }),
            update_mode: Some(if previous.is_some() {
                api::UpdateMode::UpdateOnly
            } else {
                api::UpdateMode::InsertOnly
            }),
        };
        let result = self
            .api
            .upsert(&collection(&identity.group_id, true), &input)
            .await;
        // An HTTP failure can follow a committed write. Observe durable state;
        // accepting the same winning configuration is safe and idempotent.
        let current = self
            .group_configuration(identity)
            .await?
            .ok_or_else(failure)?;
        if current.matches(self, identity) {
            return Ok(current.version);
        }
        result?;
        Err(conflict("concurrent shared gold activation; retry startup"))
    }

    pub(super) async fn gold_configuration(
        &self,
        identity: &Identity,
    ) -> Result<crate::shared::GroupGold, AppError> {
        let current = self.group_configuration(identity).await?.ok_or_else(|| {
            conflict("shared group gold configuration is missing; restart the API")
        })?;
        if !current.matches(self, identity) {
            return Err(conflict(
                "shared group gold configuration changed; restart this API with the active settings",
            ));
        }
        Ok(crate::shared::GroupGold {
            version: current.version,
            build: current.build,
            generation: current.generation,
            dimensions: current.dimensions,
        })
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
