//! Explicit indexing, publication, progress and watcher selection.
use super::models::IndexResponse;
use super::{
    IndexRequest, Operation, Request, SearchMode, default_models_path, publication_config,
};
use crate::AppError;
use crate::config::{self, ResolvedConfig};
use crate::provider::Provider;
use crate::semantic::{self, Models};
use crate::storage::Storage;
use crate::{shared, silver as history};
#[cfg(test)]
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use crate::{silver::local as silver, views::local as views};

enum LocalInput {
    ProviderFiles(history::IndexOptions, crate::attachments::Configuration),
    StoredSilver,
}

#[tracing::instrument(name = "index", skip_all)]
pub(super) async fn run(
    configuration: &ResolvedConfig,
    database: &Path,
    models: &Path,
    request: IndexRequest,
) -> Result<IndexResponse, AppError> {
    if request.mode == super::IndexMode::LocalViews {
        return rebuild_local(configuration, database, models, request).await;
    }
    let full = request.full;
    let options = resolve_index_options(
        configuration,
        &request.provider,
        request.since_days,
        request.all_history,
    )?;
    let started = Instant::now();
    let local_options = options.clone();
    let _progress = crate::progress::Session;
    let db = database.to_path_buf();
    let root = models.to_path_buf();
    let embeddings = configuration.embeddings.clone();
    let build = configuration.views.build.clone();
    let all_sources = options.clone();
    let attachments = configuration.attachments.clone();
    let mut result = crate::blocking(move || {
        history::validate_output_path(&db, &all_sources)?;
        index(
            &db,
            &root,
            embeddings,
            full,
            LocalInput::ProviderFiles(local_options, attachments),
            &build,
        )
    })
    .await?;
    if let Some(config) = publication_config(configuration) {
        result.shared = Some(if request.defer_publication {
            // Watch mode: the local pass is complete; a slow network publisher
            // must not hold the indexing task. The queue publishes the newest
            // snapshot for this database.
            match shared::publication::enqueue(
                database,
                config,
                models,
                configuration.embeddings.as_ref(),
                configuration.attachments.share_files,
            ) {
                Ok(()) => shared::Publication::pending(),
                Err(error) => {
                    let mut pending = shared::Publication::pending();
                    pending.attachment_failure(error);
                    pending
                }
            }
        } else {
            shared::publish(
                database,
                config,
                Some(shared::Inference {
                    models_dir: models,
                    embeddings: configuration.embeddings.as_ref(),
                }),
            )
            .await
        });
    }
    if configuration.attachments.share_files
        && let Some(publication) = &mut result.shared
        && !publication.is_pending()
        && let Some(config) = configuration.shared.as_ref()
        && let Err(error) = shared::attachments::publish(database, config).await
    {
        publication.attachment_failure(error);
    }
    result.total_milliseconds = elapsed_milliseconds(started);
    emit_finished(&result);
    Ok(result)
}

fn emit_finished(result: &IndexResponse) {
    let phase = if result
        .shared
        .as_ref()
        .is_some_and(shared::Publication::is_pending)
    {
        crate::progress::Phase::SharedPending
    } else if result.needs_retry() {
        crate::progress::Phase::Incomplete
    } else {
        crate::progress::Phase::Complete
    };
    crate::progress::emit(&crate::progress::Event {
        malformed_records: Some(result.malformed_records),
        record_diagnostics: Some(result.record_diagnostics.clone()),
        ..crate::progress::Event::new(
            phase,
            result.total_milliseconds,
            (!result.needs_retry()).then_some(0),
        )
    });
}

async fn rebuild_local(
    configuration: &ResolvedConfig,
    database: &Path,
    models: &Path,
    request: IndexRequest,
) -> Result<IndexResponse, AppError> {
    if !request.provider.is_empty() || request.since_days.is_some() || request.all_history {
        return Err(AppError::usage(
            "rebuild-local-views cannot be combined with provider or time selectors",
        ));
    }
    let _progress = crate::progress::Session;
    let db = database.to_path_buf();
    let root = models.to_path_buf();
    let embeddings = configuration.embeddings.clone();
    let build = configuration.views.build.clone();
    let result = crate::blocking(move || {
        index(
            &db,
            &root,
            embeddings,
            request.full,
            LocalInput::StoredSilver,
            &build,
        )
    })
    .await?;
    emit_finished(&result);
    Ok(result)
}

#[tracing::instrument(name = "local_index", skip_all)]
fn index(
    database_path: &Path,
    models_dir: &Path,
    embeddings_config: Option<semantic::remote::Configuration>,
    full: bool,
    input: LocalInput,
    build: &crate::views::config::Build,
) -> Result<IndexResponse, AppError> {
    let total_started = Instant::now();
    let rebuild_local_views = matches!(input, LocalInput::StoredSilver);
    if rebuild_local_views {
        // Read-only preflight prevents schema migration or empty database
        // creation from becoming an implicit bronze-to-silver operation.
        require_certified_silver(&Storage::open_catalog(database_path)?)?;
    }
    let models = Models::new(models_dir.to_path_buf()).with_remote(embeddings_config);
    if build.dense_enabled() && !build.users_only() {
        models.require_indexer()?;
    }
    let storage_started = Instant::now();
    let mut storage = Storage::open_writer(database_path)?;
    if rebuild_local_views {
        require_certified_silver(&storage)?;
        if full {
            // Replay: nothing from existing views is reused.
            storage.discard_views()?;
        }
    }
    if full || storage.derived_search_is_dirty()? {
        storage.defer_search_updates()?;
    }
    if full {
        storage.mark_derived_search_dirty()?;
    }
    let storage_setup_milliseconds = elapsed_milliseconds(storage_started);
    let (summary, certificate, ingestion_milliseconds) = match input {
        LocalInput::ProviderFiles(options, attachments) => {
            storage.configure_attachments(attachments)?;
            let source = silver::ingest(&mut storage, &options)?;
            (source.summary, source.certificate, source.milliseconds)
        }
        LocalInput::StoredSilver => (
            history::IndexSummary::default(),
            storage.silver_certificate()?,
            0,
        ),
    };
    let views = views::build(&mut storage, &models, &summary, build)?;
    let counts = views.counts;
    let embeddings = views.embeddings;
    storage.commit_writer()?;
    if full {
        crate::progress::during(crate::progress::Phase::DatabaseCheckpoint, || {
            storage.truncate_wal()
        })?;
    }
    emit_index_phase(crate::progress::Phase::LocalComplete, &summary);
    Ok(IndexResponse {
        views: storage.views_manifest()?,
        rebuild_local_views,
        silver: certificate,
        indexed_conversations: counts.conversations,
        indexed_messages: counts.messages,
        searchable_messages: counts.searchable_messages,
        scanned_files: summary.scanned_files,
        malformed_records: summary.malformed_records,
        record_diagnostics: summary.record_diagnostics,
        changed_messages: summary.changed_messages,
        removed_messages: summary.removed_messages,
        unchanged_sources: summary.unchanged_sources,
        checkpoint_skipped_sources: summary.checkpoint_skipped_sources,
        tombstoned_sources: summary.tombstoned_sources,
        purged_conversations: summary.purged_conversations,
        committed_batches: summary.committed_batches,
        discovered_bytes: summary.discovered_bytes,
        processed_bytes: summary.processed_bytes,
        full,
        embeddings: embeddings.stored_vectors,
        model_inferences: embeddings.model_inferences,
        reused_embeddings: embeddings.reused_vectors,
        realized_mode: if views.ready {
            if build.dense_enabled() {
                SearchMode::Hybrid
            } else {
                SearchMode::Lexical
            }
        } else {
            SearchMode::Unavailable
        },
        model_load_milliseconds: views.model_load_milliseconds,
        storage_setup_milliseconds,
        ingestion_milliseconds,
        search_refresh_milliseconds: views.search_refresh_milliseconds,
        embedding_milliseconds: views.embedding_milliseconds,
        total_milliseconds: elapsed_milliseconds(total_started),
        shared: None,
    })
}

fn require_certified_silver(storage: &Storage) -> Result<(), AppError> {
    if !storage.silver_certificate()?.permits_views() {
        return Err(AppError::usage(
            "local views rebuild requires a finished current scan; run explicit mash index to repair or replay history first",
        ));
    }
    Ok(())
}

fn elapsed_milliseconds(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn emit_index_phase(phase: crate::progress::Phase, summary: &history::IndexSummary) {
    let event =
        crate::progress::Event::files(phase, summary, 0, None, crate::progress::EtaScope::Phase);
    crate::progress::emit(&event);
}

#[cfg(test)]
fn write_embedding_progress(
    writer: &mut dyn Write,
    progress: &semantic::EmbeddingProgress,
) -> Result<(), AppError> {
    let event = crate::progress::Event::embedding(progress);
    serde_json::to_writer(&mut *writer, &event)
        .map_err(|error| AppError::internal(format!("failed to serialize progress: {error}")))?;
    writeln!(writer)
        .map_err(|error| AppError::internal(format!("failed to write progress: {error}")))
}

fn resolve_index_options(
    configuration: &ResolvedConfig,
    requested_providers: &[Provider],
    since_days: Option<u32>,
    all_history: bool,
) -> Result<history::IndexOptions, AppError> {
    if since_days.is_some() && all_history {
        return Err(AppError::usage(
            "--since-days and --all-history cannot be used together",
        ));
    }
    if matches!(since_days, Some(days) if !(1..=36_500).contains(&days)) {
        return Err(AppError::usage("--since-days must be between 1 and 36500"));
    }

    let providers = if requested_providers.is_empty() {
        configuration.providers.clone()
    } else {
        requested_providers
            .iter()
            .map(|&provider| {
                configuration
                    .providers
                    .get(&provider)
                    .cloned()
                    .map(|roots| (provider, roots))
                    .ok_or_else(|| AppError::usage(format!("provider is not enabled: {provider}")))
            })
            .collect::<Result<_, _>>()?
    };

    Ok(history::IndexOptions {
        providers,
        roots_are_authoritative: configuration.loaded,
        since_days: if all_history {
            None
        } else {
            since_days.or(configuration.since_days)
        },
    })
}

pub(crate) fn watch_targets(request: &Request) -> Result<crate::watch::Targets, AppError> {
    let configuration = config::load(request.paths.config.as_deref())?;
    let Operation::Index(IndexRequest {
        mode,
        provider,
        since_days,
        all_history,
        ..
    }) = &request.operation
    else {
        return Err(AppError::usage("watch requires an index request"));
    };
    if *mode != super::IndexMode::ProviderFiles {
        return Err(AppError::usage("watch cannot rebuild only views"));
    }
    let options = resolve_index_options(&configuration, provider, *since_days, *all_history)?;
    let mut roots = history::watch_roots(&options);
    roots.sort();
    roots.dedup();
    let models = request
        .paths
        .models_dir
        .clone()
        .unwrap_or_else(default_models_path);
    Ok(crate::watch::Targets {
        roots,
        files: vec![configuration.path, models.join("installed.json")],
    })
}

#[cfg(test)]
mod tests;
