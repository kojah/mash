//! Group views settings. Changing them is an explicit operator activation.
use super::{AppError, PointKind, Scope, Store, api, collection, failure, point_id};
use crate::views::config::Build;
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u32 = 4;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Configuration {
    pub(super) kind: PointKind,
    pub(super) schema_version: u32,
    pub(super) version: String,
    pub(super) build: Build,
    pub(super) generation: String,
    pub(super) dimensions: usize,
    pub(super) vector_search: super::VectorIndex,
}

impl Configuration {
    fn matches(&self, store: &Store, identity: &Scope) -> bool {
        self.build == store.build
            && self.generation == identity.generation
            && self.dimensions == identity.dimensions
    }

    fn validate(&self) -> Result<(), AppError> {
        self.vector_search.validate().map_err(|_| failure())?;
        if self.kind != PointKind::Configuration
            || self.schema_version != SCHEMA_VERSION
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

fn version() -> Result<String, AppError> {
    let mut random = [0u8; 32];
    getrandom::fill(&mut random).map_err(|_| AppError::internal("create views identity"))?;
    Ok(blake3::hash(&random).to_hex().to_string())
}

impl Store {
    pub(crate) fn with_query(mut self, query: super::VectorIndex) -> Self {
        self.vector_index = query;
        self
    }

    async fn group_configuration(
        &self,
        identity: &Scope,
    ) -> Result<Option<Configuration>, AppError> {
        let id = point_id(&["views-configuration"]);
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

    async fn put_configuration(
        &self,
        identity: &Scope,
        configuration: &Configuration,
    ) -> Result<(), AppError> {
        configuration.validate()?;
        self.api
            .upsert(
                &collection(&identity.group_id, true),
                &api::Upsert {
                    points: vec![api::Point {
                        id: point_id(&["views-configuration"]),
                        vector: api::Vectors::default(),
                        payload: configuration,
                    }],
                },
            )
            .await
    }

    /// Bootstrap once, or explicitly replace the group's recipe and model space.
    /// Replacing them makes every node's views stale until it is republished.
    pub(crate) async fn select_views(
        &self,
        identity: &Scope,
        activate: bool,
    ) -> Result<String, AppError> {
        let previous = self.group_configuration(identity).await?;
        if let Some(previous) = &previous {
            if previous.matches(self, identity) {
                return Ok(previous.version.clone());
            }
            if !activate {
                return Err(AppError::configuration(
                    "server views settings conflict with the persisted group configuration; use --activate-views for an intentional switch",
                ));
            }
        }
        let configuration = Configuration {
            kind: PointKind::Configuration,
            schema_version: SCHEMA_VERSION,
            version: version()?,
            build: self.build.clone(),
            generation: identity.generation.clone(),
            dimensions: identity.dimensions,
            vector_search: previous
                .as_ref()
                .map_or(self.vector_index, |p| p.vector_search),
        };
        self.put_configuration(identity, &configuration).await?;
        Ok(configuration.version)
    }

    /// Select retrieval defaults and, on explicit activation, planner thresholds.
    pub(crate) async fn select_query(
        &self,
        identity: &Scope,
        activate: bool,
    ) -> Result<(), AppError> {
        self.vector_index.validate()?;
        let mut configuration = self.active_configuration(identity).await?;
        if configuration.vector_search != self.vector_index && !activate {
            return Err(AppError::configuration(
                "server query defaults conflict with the persisted group policy; use --activate-query for an intentional change",
            ));
        }
        self.api
            .prepare_query(
                &collection(&identity.group_id, false),
                self.vector_index,
                activate,
            )
            .await?;
        if configuration.vector_search != self.vector_index {
            configuration.vector_search = self.vector_index;
            self.put_configuration(identity, &configuration).await?;
        }
        Ok(())
    }

    pub(crate) async fn views_configuration(&self, identity: &Scope) -> Result<Settings, AppError> {
        let current = self.active_configuration(identity).await?;
        Ok(Settings {
            version: current.version,
            build: current.build,
            generation: current.generation,
            dimensions: current.dimensions,
            vector_search: current.vector_search,
        })
    }

    pub(super) async fn active_configuration(
        &self,
        identity: &Scope,
    ) -> Result<Configuration, AppError> {
        let current = self.group_configuration(identity).await?.ok_or_else(|| {
            super::conflict("shared group views configuration is missing; restart the API")
        })?;
        if !current.matches(self, identity) {
            return Err(super::conflict(
                "shared group views configuration changed; restart this API with the active settings",
            ));
        }
        Ok(current)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Settings {
    pub(crate) version: String,
    pub(crate) build: crate::views::config::Build,
    pub(crate) generation: String,
    pub(crate) dimensions: usize,
    #[serde(default)]
    pub(crate) vector_search: super::VectorIndex,
}
