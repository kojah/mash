use super::{Storage, generated};
use crate::{
    AppError,
    views::{config::Build, manifest::Manifest},
};

impl Storage {
    pub(crate) fn lexical_manifest(
        &self,
    ) -> Result<Option<crate::views::lexical_manifest::Manifest>, AppError> {
        generated::LexicalManifest
            .query_one(&self.connection)
            .map_err(AppError::database)?
            .manifest
            .map(|json| {
                serde_json::from_str(&json)
                    .map_err(|_| AppError::database_data("invalid lexical build manifest"))
            })
            .transpose()
    }

    pub(crate) fn publish_lexical_manifest(&self) -> Result<(), AppError> {
        self.require_writer()?;
        let silver = self.silver_certificate()?;
        if !silver.is_complete() || self.derived_search_is_dirty()? {
            return Err(AppError::search_not_ready(
                "lexical publication requires complete silver and clean derived data",
            ));
        }
        let manifest = crate::views::lexical_manifest::Manifest::new(
            silver,
            self.counts()?.searchable_messages,
        );
        let json = serde_json::to_string(&manifest)
            .map_err(|_| AppError::internal("serialize lexical build manifest"))?;
        self.connection
            .execute(
                "UPDATE lexical_manifest SET manifest=?1 WHERE singleton=1",
                [json],
            )
            .map_err(AppError::database)?;
        Ok(())
    }

    pub(crate) fn views_manifest(&self) -> Result<Option<Manifest>, AppError> {
        let row = generated::ViewManifest
            .query_one(&self.connection)
            .map_err(AppError::database)?;
        row.manifest
            .map(|json| {
                serde_json::from_str(&json)
                    .map_err(|_| AppError::database_data("invalid views build manifest"))
            })
            .transpose()
    }

    pub(crate) fn publish_views_manifest(&self, manifest: &Manifest) -> Result<(), AppError> {
        self.require_writer()?;
        let json = serde_json::to_string(manifest)
            .map_err(|_| AppError::internal("serialize views build manifest"))?;
        self.connection
            .execute(
                "UPDATE views_manifest SET manifest=?1 WHERE singleton=1",
                [json],
            )
            .map_err(AppError::database)?;
        Ok(())
    }

    pub(crate) fn require_views_build(&self, build: &Build, model: &str) -> Result<(), AppError> {
        let silver = self.silver_certificate()?;
        if !self
            .views_manifest()?
            .is_some_and(|m| m.matches(build, model, &silver))
        {
            return Err(AppError::search_not_ready(
                "views build configuration or silver revision is stale; run `mash index --rebuild-local-views`",
            ));
        }
        Ok(())
    }
}
