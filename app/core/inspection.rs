//! Read-only canonical views and local/shared readiness inspection.
use super::models::{ArtifactState, LocalArtifacts, ModelAvailability, StatusResponse};
use super::{SearchMode, ViewResponse, publication_config, required_shared};
use crate::AppError;
use crate::config::ResolvedConfig;
use crate::semantic::{self, Models};
use crate::shared;
use crate::storage::Storage;
use std::path::Path;

pub(super) async fn list_attachments(
    configuration: &ResolvedConfig,
    database: &Path,
    message: String,
    node: Option<String>,
) -> Result<crate::attachments::Listing, AppError> {
    if let Some(node) = node {
        shared::attachments::list(required_shared(configuration)?, &node, &message).await
    } else {
        let db = database.to_path_buf();
        crate::blocking(move || Storage::open_catalog(&db)?.list_attachments(&message)).await
    }
}

pub(super) async fn get_attachment(
    configuration: &ResolvedConfig,
    database: &Path,
    message: String,
    attachment: String,
    node: Option<String>,
) -> Result<crate::attachments::Retrieved, AppError> {
    if let Some(node) = node {
        shared::attachments::get(
            required_shared(configuration)?,
            &node,
            &message,
            &attachment,
        )
        .await
    } else {
        let db = database.to_path_buf();
        crate::blocking(move || Storage::open_catalog(&db)?.get_attachment(&message, &attachment))
            .await
    }
}

pub(super) async fn view(
    configuration: &ResolvedConfig,
    database: &Path,
    id: String,
    context: u32,
    node: Option<String>,
) -> Result<ViewResponse, AppError> {
    let output = configuration.redaction.output();
    if let Some(node) = node {
        let mut messages =
            shared::view(required_shared(configuration)?, &node, &id, context).await?;
        for message in &mut messages {
            message.content = output.text(&message.content);
            output.tools(&mut message.tools);
        }
        Ok(ViewResponse { id, messages })
    } else {
        let database = database.to_path_buf();
        crate::blocking(move || local_view(&database, id, context, output)).await
    }
}

pub(super) async fn status(
    configuration: &ResolvedConfig,
    database: &Path,
    models: &Path,
    inspect_shared: bool,
    coverage: Option<super::coverage::Scope>,
) -> Result<StatusResponse, AppError> {
    if let Some(scope) = &coverage {
        scope.validate()?;
    }
    let db = database.to_path_buf();
    let models = models.to_path_buf();
    let config = configuration.clone();
    let mut response = crate::blocking(move || local_status(&db, &models, &config)).await?;
    if let Some(scope) = coverage {
        response.coverage = Some(super::coverage::local(database, configuration, scope).await?);
    }
    if inspect_shared {
        match shared::freshness(required_shared(configuration)?).await {
            Ok(freshness) => {
                if let Some(report) = &mut response.coverage {
                    report.add_shared(&freshness)?;
                }
                response.shared_index = Some(freshness);
            }
            Err(error) => response.shared_error = Some(error.error.message),
        }
    }
    Ok(response)
}

fn local_view(
    database_path: &Path,
    id: String,
    context: u32,
    output: crate::redaction::Policy,
) -> Result<ViewResponse, AppError> {
    let storage = Storage::open_existing(database_path)?;
    let mut messages = storage.view(&id, context)?;
    for message in &mut messages {
        message.content = output.text(&message.content);
        output.tools(&mut message.tools);
        // Connector attachments are source references, not captured files.
        if !message
            .id
            .starts_with(crate::storage::connector_views::ANCHOR_PREFIX)
        {
            message.attachments = storage.list_attachments(&message.id)?.attachments;
        }
    }
    Ok(ViewResponse { id, messages })
}

fn local_status(
    database_path: &Path,
    models_dir: &Path,
    configuration: &ResolvedConfig,
) -> Result<StatusResponse, AppError> {
    let models =
        Models::new(models_dir.to_path_buf()).with_remote(configuration.embeddings.clone());
    let models_installed = models.is_installed();
    let dense_enabled = configuration.views.build.dense_enabled();
    let exact = configuration.views.query.intent == crate::views::exact::Intent::ExactMention;
    let query_models = query_model_availability(&models, &configuration.views.query)?;
    let query_models_available = !matches!(query_models, ModelAvailability::Unavailable);
    if !database_path.is_file() {
        return Ok(StatusResponse {
            coverage: None,
            shared_error: None,
            local_artifacts: local_artifacts(None, dense_enabled, false, false),
            lexical: None,
            views: None,
            silver: None,
            ready: false,
            database_path: Some(database_path.to_path_buf()),
            conversations: 0,
            messages: 0,
            searchable_messages: 0,
            embeddings: 0,
            stored_embeddings: 0,
            models_installed,
            query_models,
            semantic_support: dense_enabled,
            realized_mode: SearchMode::Unavailable,
            recommended_action: Some(if query_models_available {
                crate::Action::Index
            } else {
                crate::Action::ModelsInstall
            }),
            configuration: configuration.status(),
            sources: native_sources(configuration),
            shared: publication_config(configuration).map(|_| shared::Publication::pending()),
            shared_index: None,
        });
    }

    let model = semantic::configured_generation(configuration.embeddings.as_ref());
    let snapshot =
        Storage::status_snapshot(database_path, &configuration.views.build.generation(&model))?;
    let counts = &snapshot.counts;
    let current_embeddings = snapshot.current_embeddings;
    let manifest_matches = snapshot
        .views
        .as_ref()
        .zip(snapshot.silver.as_ref())
        .is_some_and(|(m, s)| m.matches(&configuration.views.build, &model, s));
    let dense_current =
        snapshot.derived_clean && snapshot.exact_semantic_coverage && manifest_matches;
    let hybrid_ready = query_models_available && dense_current;
    let lexical_ready = snapshot.derived_clean
        && snapshot
            .lexical
            .as_ref()
            .zip(snapshot.silver.as_ref())
            .is_some_and(|(m, s)| m.matches(s, counts.searchable_messages));
    let ready = if exact { lexical_ready } else { hybrid_ready };
    Ok(StatusResponse {
        coverage: None,
        shared_error: None,
        local_artifacts: local_artifacts(
            Some(&snapshot),
            dense_enabled,
            lexical_ready,
            dense_current,
        ),
        lexical: snapshot.lexical,
        views: snapshot.views,
        silver: snapshot.silver,
        ready,
        database_path: Some(database_path.to_path_buf()),
        conversations: counts.conversations,
        messages: counts.messages,
        searchable_messages: counts.searchable_messages,
        embeddings: current_embeddings,
        stored_embeddings: counts.embeddings,
        models_installed,
        query_models,
        semantic_support: dense_enabled,
        realized_mode: if ready && exact {
            SearchMode::Lexical
        } else if ready {
            SearchMode::Hybrid
        } else {
            SearchMode::Unavailable
        },
        recommended_action: if !query_models_available {
            Some(crate::Action::ModelsInstall)
        } else if ready {
            None
        } else {
            Some(crate::Action::Index)
        },
        configuration: configuration.status(),
        sources: [native_sources(configuration), snapshot.connectors].concat(),
        shared: shared::status(database_path, publication_config(configuration))?,
        shared_index: None,
    })
}

fn native_sources(configuration: &ResolvedConfig) -> Vec<crate::source::SourceStatus> {
    configuration
        .providers
        .keys()
        .map(|&provider| crate::source::SourceStatus::native(provider))
        .collect()
}

fn local_artifacts(
    snapshot: Option<&crate::storage::StatusSnapshot>,
    dense_enabled: bool,
    lexical_current: bool,
    dense_current: bool,
) -> LocalArtifacts {
    LocalArtifacts {
        // Current-schema metadata views need no complete provider-scan certificate.
        metadata: ArtifactState::publication(snapshot.is_some_and(|s| s.silver.is_some()), true),
        lexical: ArtifactState::publication(
            snapshot.is_some_and(|s| s.lexical.is_some()),
            lexical_current,
        ),
        dense: if dense_enabled {
            ArtifactState::publication(snapshot.is_some_and(|s| s.views.is_some()), dense_current)
        } else {
            ArtifactState::Disabled
        },
    }
}

fn query_model_availability(
    models: &Models,
    query: &crate::views::config::Query,
) -> Result<ModelAvailability, AppError> {
    if query.intent == crate::views::exact::Intent::ExactMention {
        return Ok(ModelAvailability::NotRequired);
    }
    match models.require_search(query.ranking) {
        Ok(()) => Ok(ModelAvailability::Available),
        Err(error) if matches!(error.error.kind, crate::ErrorKind::Configuration) => Err(error),
        Err(_) => Ok(ModelAvailability::Unavailable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_distinguishes_remote_fusion_requirements_from_installed_assets() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("node.toml");
        let models = root.path().join("missing-models");
        let database = root.path().join("missing.sqlite3");
        for (ranking, available) in [("fusion", true), ("contextual-rerank", false)] {
            std::fs::write(&config, format!("version=1\nproviders={{}}\n[embeddings]\nurl='http://127.0.0.1:1/v1/embeddings'\nmodel='test'\ndimensions=384\n[views.query]\nranking='{ranking}'\n")).unwrap();
            let configuration = crate::config::load(Some(&config)).unwrap();
            let status = local_status(&database, &models, &configuration).unwrap();
            assert!(!status.models_installed);
            assert_eq!(
                matches!(status.query_models, ModelAvailability::Available),
                available
            );
            assert!(!status.ready);
            if available {
                assert!(matches!(
                    status.recommended_action,
                    Some(crate::Action::Index)
                ));
            } else {
                assert!(matches!(
                    status.recommended_action,
                    Some(crate::Action::ModelsInstall)
                ));
            }
        }
        assert!(!database.exists() && !models.exists());
    }
}
