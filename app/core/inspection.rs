//! Read-only canonical views and local/shared readiness inspection.
use super::models::{ArtifactState, LocalArtifacts, ModelAvailability, StatusResponse};
use super::{SearchMode, ViewResponse, publication_config, required_shared};
use crate::AppError;
use crate::config::ResolvedConfig;
use crate::semantic::Models;
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
        crate::blocking(move || Storage::open_silver(&db)?.list_attachments(&message)).await
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
        crate::blocking(move || Storage::open_silver(&db)?.get_attachment(&message, &attachment))
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

#[allow(
    clippy::too_many_lines,
    reason = "one status report assembles local, coverage and shared sections"
)]
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
    if response.silver.is_some() {
        match local_gold_status(database, configuration, &mut response).await {
            Ok(()) => {}
            Err(error) => response.local_views_error = Some(error.error.message),
        }
    }
    if let Some(scope) = coverage {
        response.coverage = Some(super::coverage::local(database, configuration, scope).await?);
    }
    if inspect_shared {
        let shared_config = required_shared(configuration)?;
        match shared::freshness(shared_config).await {
            Ok(freshness) => {
                if let Some(report) = &mut response.coverage {
                    report.add_shared(&freshness)?;
                }
                response.shared_index = Some(freshness);
                if let Ok(duplicates) = shared::fingerprints(shared_config).await {
                    response.duplicates = Some(duplicates);
                }
            }
            Err(error) => response.shared_error = Some(error.error.message),
        }
    }
    Ok(response)
}

async fn local_gold_status(
    database: &Path,
    configuration: &ResolvedConfig,
    response: &mut StatusResponse,
) -> Result<(), AppError> {
    let local = crate::views::local::Local::open_exact(database, configuration).await?;
    if response.silver.as_ref() != Some(&local.certificate) {
        response.local_views_error = Some("silver changed while reading status; retry".into());
        return Ok(());
    }
    let Some(node) = local.current().await? else {
        return Ok(());
    };
    let dense_current = node.build == configuration.views.build;
    response.lexical = Some(crate::views::lexical_manifest::Manifest::new(
        local.certificate.clone(),
        response.searchable_messages,
    ));
    response.views = Some(crate::views::manifest::Manifest::new(
        node.build.clone(),
        local.scope.generation,
        local.certificate,
    ));
    let embeddings = node.stored_vectors();
    response.stored_embeddings = embeddings;
    response.embeddings = if dense_current { embeddings } else { 0 };
    response.local_artifacts.lexical = ArtifactState::publication(true, true);
    if configuration.views.build.dense_enabled() && dense_current {
        response.local_artifacts.dense = ArtifactState::publication(true, true);
    }
    let exact = configuration.views.query.intent == crate::views::exact::Intent::ExactMention;
    response.ready = (exact || dense_current)
        && !matches!(response.query_models, ModelAvailability::Unavailable);
    if response.ready {
        response.realized_mode =
            if configuration.views.query.intent == crate::views::exact::Intent::ExactMention {
                SearchMode::Lexical
            } else {
                SearchMode::Hybrid
            };
        response.recommended_action = None;
    }
    Ok(())
}

pub(super) async fn chat_messages(
    configuration: &ResolvedConfig,
    database: &Path,
    chat_id: String,
    offset: u32,
    limit: u32,
) -> Result<ViewResponse, AppError> {
    let output = configuration.redaction.output();
    let database = database.to_path_buf();
    crate::blocking(move || {
        let storage = Storage::open_silver(&database)?;
        let mut messages = storage.chat_messages(&chat_id, i64::from(offset), i64::from(limit))?;
        for message in &mut messages {
            message.content = output.text(&message.content);
            output.tools(&mut message.tools);
            message.attachments = storage.list_attachments(&message.id)?.attachments;
        }
        Ok(ViewResponse {
            id: chat_id,
            messages,
        })
    })
    .await
}

fn local_view(
    database_path: &Path,
    id: String,
    context: u32,
    output: crate::redaction::Policy,
) -> Result<ViewResponse, AppError> {
    let storage = Storage::open_silver(database_path)?;
    let mut messages = storage.view(&id, context)?;
    for message in &mut messages {
        message.content = output.text(&message.content);
        output.tools(&mut message.tools);
        message.attachments = storage.list_attachments(&message.id)?.attachments;
    }
    Ok(ViewResponse { id, messages })
}

#[allow(
    clippy::too_many_lines,
    reason = "one local status report assembles readiness, counts and artifacts"
)]
fn local_status(
    database_path: &Path,
    models_dir: &Path,
    configuration: &ResolvedConfig,
) -> Result<StatusResponse, AppError> {
    let models =
        Models::new(models_dir.to_path_buf()).with_remote(configuration.embeddings.clone());
    let models_installed = models.is_installed();
    let dense_enabled = configuration.views.build.dense_enabled();
    let query_models = query_model_availability(&models, &configuration.views.query)?;
    let query_models_available = !matches!(query_models, ModelAvailability::Unavailable);
    if !database_path.is_file() {
        return Ok(StatusResponse {
            local_views_error: None,
            installation: super::installation::summary(),
            coverage: None,
            shared_error: None,
            local_artifacts: local_artifacts(false, dense_enabled),
            lexical: None,
            views: None,
            silver: None,
            ready: false,
            database_path: Some(database_path.to_path_buf()),
            chats: 0,
            messages: 0,
            searchable_messages: 0,
            embeddings: 0,
            stored_embeddings: 0,
            models_installed,
            query_models,
            semantic_support: dense_enabled,
            realized_mode: SearchMode::Unavailable,
            // A new machine needs setup first; init writes a configuration
            // that works without models.
            recommended_action: Some(if !configuration.loaded {
                crate::Action::Init
            } else if query_models_available {
                crate::Action::Index
            } else {
                crate::Action::ModelsInstall
            }),
            configuration: configuration.status(),
            shared: publication_config(configuration).map(|_| shared::Publication::pending()),
            shared_index: None,
            duplicates: None,
        });
    }

    let snapshot = Storage::status_snapshot(database_path)?;
    let counts = &snapshot.counts;
    Ok(StatusResponse {
        local_views_error: None,
        installation: super::installation::summary(),
        coverage: None,
        shared_error: None,
        local_artifacts: local_artifacts(snapshot.silver.is_some(), dense_enabled),
        lexical: None,
        views: None,
        silver: snapshot.silver,
        ready: false,
        database_path: Some(database_path.to_path_buf()),
        chats: counts.chats,
        messages: counts.messages,
        searchable_messages: counts.searchable_messages,
        embeddings: 0,
        stored_embeddings: counts.embeddings,
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
        shared: shared::status(database_path, publication_config(configuration))?,
        shared_index: None,
        duplicates: None,
    })
}

fn local_artifacts(has_silver: bool, dense_enabled: bool) -> LocalArtifacts {
    LocalArtifacts {
        metadata: ArtifactState::publication(has_silver, true),
        lexical: ArtifactState::publication(has_silver, false),
        dense: if dense_enabled {
            ArtifactState::publication(has_silver, false)
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

    #[tokio::test]
    #[ignore = "requires disposable MASH_TEST_QDRANT_URL and MASH_TEST_QDRANT_GRPC_URL"]
    async fn qdrant_status_rejects_a_changed_silver_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("silver.sqlite3");
        let gold = crate::views::local::test_support::Fixture::new(&database, "local").await;
        let mut configuration = gold.configuration.clone();
        configuration.views.query.intent = crate::views::exact::Intent::ExactMention;
        let models = root.path().join("missing-models");
        let mut current = local_status(&database, &models, &configuration).unwrap();
        local_gold_status(&database, &configuration, &mut current)
            .await
            .unwrap();
        assert!(current.ready);
        let mut stale = local_status(&database, &models, &configuration).unwrap();
        crate::storage::test_support::write(
            &database,
            "UPDATE silver_state SET revision=revision+1 WHERE singleton=1",
        );
        local_gold_status(&database, &configuration, &mut stale)
            .await
            .unwrap();
        assert!(!stale.ready);
        assert!(stale.lexical.is_none() && stale.views.is_none());
        assert!(stale.local_views_error.unwrap().contains("silver changed"));
        gold.cleanup().await;
    }

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
