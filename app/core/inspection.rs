//! Read-only canonical views and local/shared readiness inspection.
use super::models::StatusResponse;
use super::{SearchMode, ViewResponse, publication_config, required_shared, routed_both};
use crate::AppError;
use crate::config::ResolvedConfig;
use crate::semantic::{self, Models};
use crate::shared;
use crate::storage::Storage;
use std::path::Path;

pub(super) async fn view(
    configuration: &ResolvedConfig,
    database: &Path,
    id: String,
    context: u32,
    node: Option<String>,
) -> Result<ViewResponse, AppError> {
    if let Some(node) = node {
        let messages = shared::view(required_shared(configuration)?, &node, &id, context).await?;
        Ok(ViewResponse { id, messages })
    } else {
        let database = database.to_path_buf();
        crate::blocking(move || local_view(&database, id, context)).await
    }
}

pub(super) async fn status(
    configuration: &ResolvedConfig,
    database: &Path,
    models: &Path,
    inspect_shared: bool,
) -> Result<StatusResponse, AppError> {
    let db = database.to_path_buf();
    let models = models.to_path_buf();
    let config = configuration.clone();
    let mut response = crate::blocking(move || local_status(&db, &models, &config)).await?;
    if inspect_shared {
        response.shared_index = Some(shared::freshness(required_shared(configuration)?).await?);
    }
    Ok(response)
}

fn local_view(database_path: &Path, id: String, context: u32) -> Result<ViewResponse, AppError> {
    let storage = Storage::open_existing(database_path)?;
    let messages = storage.view(&id, context)?;
    Ok(ViewResponse { id, messages })
}

fn local_status(
    database_path: &Path,
    models_dir: &Path,
    configuration: &ResolvedConfig,
) -> Result<StatusResponse, AppError> {
    let models_installed = Models::new(models_dir.to_path_buf())
        .with_remote(configuration.embeddings.clone())
        .is_installed();
    if !database_path.is_file() {
        return Ok(StatusResponse {
            gold: None,
            silver: None,
            ready: false,
            database_path: Some(database_path.to_path_buf()),
            conversations: 0,
            messages: 0,
            searchable_messages: 0,
            embeddings: 0,
            stored_embeddings: 0,
            models_installed,
            semantic_support: true,
            realized_mode: SearchMode::Unavailable,
            recommended_action: Some(if models_installed {
                crate::Action::Index
            } else {
                crate::Action::ModelsInstall
            }),
            configuration: configuration.status(),
            shared: if routed_both(configuration) {
                Some(shared::Publication::pending())
            } else {
                publication_config(configuration).map(|_| shared::Publication::pending())
            },
            shared_index: None,
        });
    }

    let model = semantic::configured_generation(configuration.embeddings.as_ref());
    let snapshot =
        Storage::status_snapshot(database_path, &configuration.gold.build.generation(&model))?;
    let counts = snapshot.counts;
    let current_embeddings = snapshot.current_embeddings;
    let manifest_matches = snapshot
        .gold
        .as_ref()
        .zip(snapshot.silver.as_ref())
        .is_some_and(|(m, s)| m.matches(&configuration.gold.build, &model, s));
    let hybrid_ready = models_installed
        && snapshot.derived_clean
        && snapshot.exact_semantic_coverage
        && manifest_matches;
    Ok(StatusResponse {
        gold: snapshot.gold,
        silver: snapshot.silver,
        ready: hybrid_ready,
        database_path: Some(database_path.to_path_buf()),
        conversations: counts.conversations,
        messages: counts.messages,
        searchable_messages: counts.searchable_messages,
        embeddings: current_embeddings,
        stored_embeddings: counts.embeddings,
        models_installed,
        semantic_support: true,
        realized_mode: if hybrid_ready {
            SearchMode::Hybrid
        } else {
            SearchMode::Unavailable
        },
        recommended_action: if !models_installed {
            Some(crate::Action::ModelsInstall)
        } else if hybrid_ready {
            None
        } else {
            Some(crate::Action::Index)
        },
        configuration: configuration.status(),
        shared: if routed_both(configuration) {
            Some(shared::routed_status(database_path, configuration)?)
        } else {
            shared::status(database_path, publication_config(configuration))?
        },
        shared_index: None,
    })
}
