//! Explicit model installation and local/shared forgetting.
use super::models::{ForgetResponse, ModelsInstallResponse};
use super::publication_config;
use crate::AppError;
use crate::config::ResolvedConfig;
use crate::semantic::Models;
use crate::shared;
use crate::storage::Storage;
use std::path::Path;

pub(super) async fn forget(
    database: &Path,
    models: &Path,
    configuration: &ResolvedConfig,
    id: String,
) -> Result<ForgetResponse, AppError> {
    let _progress = crate::progress::Session;
    let db = database.to_path_buf();
    let local_id = id.clone();
    let mut result = crate::blocking(move || local_forget(&db, local_id)).await?;
    if result.forgotten
        && let Err(error) = crate::views::local::build(database, models, configuration, false).await
    {
        // Silver and its tombstone are already committed. A later index retries
        // projection; readers reject the old gold certificate in the meantime.
        result.local_views_error = Some(error.error.message);
    }
    if result.forgotten {
        // A search that began before the forget may have kept its words again.
        let _ = std::fs::remove_file(crate::views::qdrant::vocabulary_file(database));
    }
    if let Some(config) = publication_config(configuration) {
        result.shared = Some(
            shared::publish(
                database,
                config,
                Some(shared::publication::Inference {
                    local: Some(configuration),
                    models_dir: models,
                }),
            )
            .await,
        );
    }
    Ok(result)
}

pub(super) async fn install_models(models: &Path) -> Result<ModelsInstallResponse, AppError> {
    let models = models.to_path_buf();
    crate::blocking(move || local_install_models(&models)).await
}

fn local_install_models(models_dir: &Path) -> Result<ModelsInstallResponse, AppError> {
    let summary = Models::new(models_dir.to_path_buf()).install()?;
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
    if forgotten {
        // Kept matching words may include the forgotten text's; searches read
        // them again rather than keep them on disk. The history is already
        // forgotten, so a failure here only leaves words that match nothing.
        let _ = std::fs::remove_file(crate::views::qdrant::vocabulary_file(database_path));
    }
    Ok(ForgetResponse {
        id,
        forgotten,
        local_views_error: None,
        shared: None,
    })
}
