//! Explicit model installation and local/shared forgetting.
use super::models::{ForgetResponse, ModelsInstallResponse};
use super::{publication_config, routed_both};
use crate::AppError;
use crate::config::ResolvedConfig;
use crate::semantic::{self, Models};
use crate::shared;
use crate::storage::Storage;
use std::path::Path;

pub(super) async fn forget(
    database: &Path,
    configuration: &ResolvedConfig,
    id: String,
) -> Result<ForgetResponse, AppError> {
    let _progress = crate::progress::Session;
    let db = database.to_path_buf();
    let local_id = id.clone();
    let mut result = crate::blocking(move || local_forget(&db, local_id)).await?;
    if routed_both(configuration) {
        let (publication, forgotten) = shared::forget_routed(database, configuration, &id).await?;
        result.forgotten |= forgotten;
        result.shared = Some(publication);
    } else if let Some(config) = publication_config(configuration) {
        result.shared = Some(shared::publish(database, config).await);
    }
    Ok(result)
}

pub(super) async fn install_models(
    models: &Path,
    embeddings: Option<semantic::remote::Configuration>,
) -> Result<ModelsInstallResponse, AppError> {
    let models = models.to_path_buf();
    crate::blocking(move || local_install_models(&models, embeddings)).await
}

fn local_install_models(
    models_dir: &Path,
    embeddings: Option<semantic::remote::Configuration>,
) -> Result<ModelsInstallResponse, AppError> {
    let summary = Models::new(models_dir.to_path_buf())
        .with_remote(embeddings)
        .install()?;
    Ok(ModelsInstallResponse {
        installed: true,
        model_directory: models_dir.to_path_buf(),
        summary,
        recommended_action: crate::Action::Index,
    })
}

fn local_forget(database_path: &Path, id: String) -> Result<ForgetResponse, AppError> {
    let mut storage = Storage::open_existing(database_path)?;
    let forgotten = storage.forget(&id)?;
    Ok(ForgetResponse {
        id,
        forgotten,
        shared: None,
    })
}
