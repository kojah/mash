//! Explicit indexing, per-index routing, progress and watcher selection.
use super::models::IndexResponse;
use super::{
    IndexRequest, Operation, Request, SearchMode, default_models_path, publication_config,
    routed_both,
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

use crate::{gold::local as gold, silver::local as silver};

#[tracing::instrument(name = "index", skip_all)]
pub(super) async fn run(
    configuration: &ResolvedConfig,
    database: &Path,
    models: &Path,
    request: IndexRequest,
) -> Result<IndexResponse, AppError> {
    let full = request.full;
    let options = resolve_index_options(
        configuration,
        &request.provider,
        request.since_days,
        request.all_history,
    )?;
    let started = Instant::now();
    let mut local_options = options.clone();
    let _progress = crate::progress::Session;
    let mut shared_options = options.clone();
    config::IndexFilter::apply(configuration.local_filter.as_ref(), &mut local_options);
    config::IndexFilter::apply(configuration.shared_filter.as_ref(), &mut shared_options);
    let db = database.to_path_buf();
    let root = models.to_path_buf();
    let embeddings = configuration.embeddings.clone();
    let all_sources = options.clone();
    let mut result = crate::blocking(move || {
        history::validate_output_path(&db, &all_sources)?;
        index(&db, &root, embeddings, full, &local_options)
    })
    .await?;
    if routed_both(configuration) {
        result.shared = Some(
            shared::publish_routed(database, configuration, models, &shared_options, full).await,
        );
    } else if let Some(config) = publication_config(configuration) {
        result.shared = Some(shared::publish(database, config).await);
    }
    result.total_milliseconds = elapsed_milliseconds(started);
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
    Ok(result)
}

#[tracing::instrument(name = "local_index", skip_all)]
fn index(
    database_path: &Path,
    models_dir: &Path,
    embeddings_config: Option<semantic::remote::Configuration>,
    full: bool,
    options: &history::IndexOptions,
) -> Result<IndexResponse, AppError> {
    let total_started = Instant::now();
    let models = Models::new(models_dir.to_path_buf()).with_remote(embeddings_config);
    models.require_indexer()?;
    let storage_started = Instant::now();
    let mut storage = Storage::open_writer(database_path)?;
    if full || storage.derived_search_is_dirty()? {
        storage.defer_search_updates()?;
    }
    if full {
        storage.mark_derived_search_dirty()?;
    }
    let storage_setup_milliseconds = elapsed_milliseconds(storage_started);
    let source = silver::ingest(&mut storage, options)?;
    let gold = gold::build(&mut storage, &models, &source)?;
    let summary = source.summary;
    let counts = gold.counts;
    let embeddings = gold.embeddings;
    storage.commit_writer()?;
    if full {
        crate::progress::during(crate::progress::Phase::DatabaseCheckpoint, || {
            storage.truncate_wal()
        })?;
    }
    emit_index_phase(crate::progress::Phase::LocalComplete, &summary);
    Ok(IndexResponse {
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
        realized_mode: if gold.ready {
            SearchMode::Hybrid
        } else {
            SearchMode::Unavailable
        },
        model_load_milliseconds: gold.model_load_milliseconds,
        storage_setup_milliseconds,
        ingestion_milliseconds: source.milliseconds,
        search_refresh_milliseconds: gold.search_refresh_milliseconds,
        embedding_milliseconds: gold.embedding_milliseconds,
        total_milliseconds: elapsed_milliseconds(total_started),
        shared: None,
    })
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
        provider,
        since_days,
        all_history,
        ..
    }) = &request.operation
    else {
        return Err(AppError::usage("watch requires an index request"));
    };
    let options = resolve_index_options(&configuration, provider, *since_days, *all_history)?;
    let mut roots = Vec::new();
    for (enabled, filter) in [
        (true, &configuration.local_filter),
        (
            configuration.destination != config::IndexDestination::Local,
            &configuration.shared_filter,
        ),
    ] {
        if enabled {
            let mut selected = options.clone();
            config::IndexFilter::apply(filter.as_ref(), &mut selected);
            roots.extend(history::watch_roots(&selected));
        }
    }
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
