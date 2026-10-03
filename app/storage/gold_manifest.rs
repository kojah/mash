use super::{Storage, generated};
use crate::{
    AppError,
    gold::{config::Build, manifest::Manifest},
};

impl Storage {
    pub(crate) fn gold_manifest(&self) -> Result<Option<Manifest>, AppError> {
        let row = generated::GoldManifest
            .query_one(&self.connection)
            .map_err(AppError::database)?;
        row.manifest
            .map(|json| {
                serde_json::from_str(&json)
                    .map_err(|_| AppError::database_data("invalid gold build manifest"))
            })
            .transpose()
    }

    pub(crate) fn publish_gold_manifest(&self, manifest: &Manifest) -> Result<(), AppError> {
        self.require_writer()?;
        let json = serde_json::to_string(manifest)
            .map_err(|_| AppError::internal("serialize gold build manifest"))?;
        self.connection
            .execute(
                "UPDATE gold_manifest SET manifest=?1 WHERE singleton=1",
                [json],
            )
            .map_err(AppError::database)?;
        Ok(())
    }

    pub(crate) fn require_gold_build(&self, build: &Build, model: &str) -> Result<(), AppError> {
        let silver = self.silver_certificate()?;
        if !self
            .gold_manifest()?
            .is_some_and(|m| m.matches(build, model, &silver))
        {
            return Err(AppError::search_not_ready(
                "gold build configuration or silver revision is stale; run `mash index --rebuild-local-gold`",
            ));
        }
        Ok(())
    }
}
