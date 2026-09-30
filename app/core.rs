//! Transport-independent application requests, responses, and operations.
//! CLI, MCP, and the idle search worker call this module directly.

use crate::AppError;
use crate::config::{self, ConfigurationStatus, ResolvedConfig};
use crate::ingestion;
use crate::provider::Provider;
use crate::semantic::{self, Models};
use crate::shared;
use crate::storage::Storage;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Paths {
    pub(crate) config: Option<PathBuf>,
    pub(crate) db: Option<PathBuf>,
    pub(crate) models_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    pub(crate) paths: Paths,
    pub(crate) operation: Operation,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IndexRequest {
    pub(crate) full: bool,
    pub(crate) provider: Vec<Provider>,
    pub(crate) since_days: Option<u32>,
    pub(crate) all_history: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) enum Operation {
    Index(IndexRequest),
    Search {
        query: String,
        limit: usize,
        unit: crate::search_results::SearchUnit,
        filters: Box<crate::search_filter::Selection>,
        shared: bool,
        local: bool,
    },
    View {
        id: String,
        context: u32,
        shared: Option<String>,
    },
    Status {
        shared: bool,
    },
    Forget {
        id: String,
    },
    ModelsInstall,
}

pub(crate) async fn execute(request: Request) -> Result<Response, AppError> {
    let path = request.paths.config;
    let configuration = crate::blocking(move || config::load(path.as_deref())).await?;
    let database = request.paths.db.unwrap_or_else(default_database_path);
    let models = request.paths.models_dir.unwrap_or_else(default_models_path);
    dispatch(request.operation, &configuration, &database, &models).await
}

#[cfg(unix)]
pub(crate) async fn worker_search(request: Request) -> Result<SearchResponse, AppError> {
    if !matches!(request.operation, Operation::Search { .. }) {
        return Err(AppError::usage("search worker accepts only searches"));
    }
    match execute(request).await? {
        Response::Search(response) => Ok(response),
        _ => Err(AppError::internal("unexpected search worker response")),
    }
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(super) enum Response {
    Index(IndexResponse),
    Search(SearchResponse),
    View(ViewResponse),
    Status(StatusResponse),
    Forget(ForgetResponse),
    ModelsInstall(ModelsInstallResponse),
}

#[derive(Debug, Serialize)]
pub(super) struct IndexResponse {
    indexed_conversations: u64,
    indexed_messages: u64,
    searchable_messages: u64,
    scanned_files: u64,
    malformed_records: u64,
    changed_messages: u64,
    removed_messages: u64,
    unchanged_sources: u64,
    checkpoint_skipped_sources: u64,
    tombstoned_sources: u64,
    purged_conversations: u64,
    committed_batches: u64,
    discovered_bytes: u64,
    processed_bytes: u64,
    full: bool,
    embeddings: u64,
    model_inferences: u64,
    reused_embeddings: u64,
    realized_mode: SearchMode,
    model_load_milliseconds: u64,
    storage_setup_milliseconds: u64,
    ingestion_milliseconds: u64,
    search_refresh_milliseconds: u64,
    embedding_milliseconds: u64,
    total_milliseconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    shared: Option<shared::Publication>,
}

impl Response {
    pub(crate) fn needs_retry(&self) -> bool {
        matches!(self, Self::Index(result) if result.malformed_records > 0
            || result.shared.as_ref().is_some_and(shared::Publication::is_pending))
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SearchMode {
    Hybrid,
    LocalHybrid,
    MergedHybrid,
    Unavailable,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct SearchResponse {
    pub(crate) query: String,
    pub(crate) realized_mode: SearchMode,
    pub(crate) unit: crate::search_results::SearchUnit,
    pub(crate) results: crate::search_results::Results,
    pub(crate) shared: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) shared_error: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ViewResponse {
    pub(crate) id: String,
    pub(crate) messages: Vec<crate::storage::Message>,
}

#[derive(Debug, Serialize)]
pub(super) struct StatusResponse {
    ready: bool,
    database_path: Option<PathBuf>,
    conversations: u64,
    messages: u64,
    searchable_messages: u64,
    embeddings: u64,
    stored_embeddings: u64,
    models_installed: bool,
    semantic_support: bool,
    realized_mode: SearchMode,
    recommended_action: Option<crate::Action>,
    configuration: ConfigurationStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    shared: Option<shared::Publication>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shared_index: Option<shared::Freshness>,
}

#[derive(Debug, Serialize)]
pub(super) struct ForgetResponse {
    id: String,
    forgotten: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    shared: Option<shared::Publication>,
}

#[derive(Debug, Serialize)]
pub(super) struct ModelsInstallResponse {
    installed: bool,
    model_directory: PathBuf,
    #[serde(flatten)]
    summary: semantic::InstallSummary,
    recommended_action: crate::Action,
}

#[allow(
    clippy::too_many_lines,
    reason = "explicit dispatch of the small typed operation surface"
)]
async fn dispatch(
    command: Operation,
    configuration: &ResolvedConfig,
    database: &Path,
    models: &Path,
) -> Result<Response, AppError> {
    match command {
        Operation::Index(IndexRequest {
            full,
            provider,
            since_days,
            all_history,
        }) => {
            let options = resolve_index_options(configuration, &provider, since_days, all_history)?;
            index_routes(configuration, database, models, full, &options).await
        }
        Operation::Search {
            query,
            limit,
            unit,
            filters,
            shared: use_shared,
            local: use_local,
            ..
        } => {
            if use_shared && use_local {
                return Err(AppError::usage(
                    "local and shared search are mutually exclusive",
                ));
            }
            let filters = filters.resolve(
                configuration
                    .shared
                    .as_ref()
                    .map_or("local", |c| c.node_id.as_str()),
            )?;
            if use_shared || (!use_local && configuration.shared.is_some()) {
                let (results, shared_error) = shared::search(
                    required_shared(configuration)?,
                    (!use_shared).then_some(database),
                    models,
                    configuration.embeddings.as_ref(),
                    &query,
                    (limit, unit),
                    &filters,
                )
                .await?;
                Ok(Response::Search(SearchResponse {
                    realized_mode: search_mode(shared_error.is_some(), use_shared),
                    unit,
                    results: crate::search_results::collect(results, unit, limit, &query),
                    query,
                    shared: shared_error.is_none(),
                    shared_error,
                }))
            } else {
                let database = database.to_path_buf();
                let models = models.to_path_buf();
                let embeddings = configuration.embeddings.clone();
                crate::blocking(move || {
                    local_search(
                        &database,
                        &models,
                        embeddings,
                        query,
                        (limit, unit),
                        &filters,
                    )
                    .map(Response::Search)
                })
                .await
            }
        }
        Operation::View {
            id,
            context,
            shared: node,
        } => {
            if node.is_some() {
                let config = required_shared(configuration)?;
                let node = node.as_deref().unwrap_or(&config.node_id);
                let messages = shared::view(config, node, &id, context).await?;
                Ok(Response::View(ViewResponse { id, messages }))
            } else {
                let database = database.to_path_buf();
                crate::blocking(move || local_view(&database, id, context).map(Response::View))
                    .await
            }
        }
        Operation::Status {
            shared: inspect_shared,
        } => {
            let database = database.to_path_buf();
            let models = models.to_path_buf();
            let config = configuration.clone();
            let mut response = crate::blocking(move || status(&database, &models, &config)).await?;
            if inspect_shared {
                let freshness = shared::freshness(required_shared(configuration)?).await?;
                if let Response::Status(status) = &mut response {
                    status.shared_index = Some(freshness);
                }
            }
            Ok(response)
        }
        Operation::Forget { id } => forget_routes(database, configuration, id).await,
        Operation::ModelsInstall => {
            let models = models.to_path_buf();
            let embeddings = configuration.embeddings.clone();
            crate::blocking(move || install_models(&models, embeddings)).await
        }
    }
}

async fn forget_routes(
    database: &Path,
    configuration: &ResolvedConfig,
    id: String,
) -> Result<Response, AppError> {
    let _progress = crate::progress::Session;
    let db = database.to_path_buf();
    let local_id = id.clone();
    let mut response = crate::blocking(move || forget(&db, local_id)).await?;
    if let Response::Forget(result) = &mut response {
        if routed_both(configuration) {
            let (publication, forgotten) =
                shared::forget_routed(database, configuration, &id).await?;
            result.forgotten |= forgotten;
            result.shared = Some(publication);
        } else if let Some(config) = publication_config(configuration) {
            result.shared = Some(shared::publish(database, config).await);
        }
    }
    Ok(response)
}

async fn index_routes(
    configuration: &ResolvedConfig,
    database: &Path,
    models: &Path,
    full: bool,
    options: &ingestion::IndexOptions,
) -> Result<Response, AppError> {
    let started = Instant::now();
    let mut local_options = options.clone();
    let _progress = crate::progress::Session;
    let mut shared_options = options.clone();
    config::IndexFilter::apply(configuration.local_filter.as_ref(), &mut local_options);
    config::IndexFilter::apply(configuration.shared_filter.as_ref(), &mut shared_options);
    let db = database.to_path_buf();
    let root = models.to_path_buf();
    let embeddings = configuration.embeddings.clone();
    let mut response =
        crate::blocking(move || index(&db, &root, embeddings, full, &local_options)).await?;
    if let Response::Index(result) = &mut response {
        if routed_both(configuration) {
            result.shared = Some(
                shared::publish_routed(database, configuration, models, &shared_options, full)
                    .await,
            );
        } else if let Some(config) = publication_config(configuration) {
            result.shared = Some(shared::publish(database, config).await);
        }
        result.total_milliseconds = elapsed_milliseconds(started);
        crate::progress::emit(&crate::progress::Event::new(
            if result
                .shared
                .as_ref()
                .is_some_and(shared::Publication::is_pending)
            {
                crate::progress::Phase::SharedPending
            } else {
                crate::progress::Phase::Complete
            },
            result.total_milliseconds,
            Some(0),
        ));
    }
    Ok(response)
}

fn required_shared(configuration: &ResolvedConfig) -> Result<&config::SharedConfig, AppError> {
    configuration
        .shared
        .as_ref()
        .ok_or_else(|| AppError::configuration("shared destination requires shared configuration"))
}

fn publication_config(configuration: &ResolvedConfig) -> Option<&config::SharedConfig> {
    if configuration.destination == config::IndexDestination::Both && !routed_both(configuration) {
        configuration.shared.as_ref()
    } else {
        None
    }
}

fn routed_both(configuration: &ResolvedConfig) -> bool {
    configuration.destination == config::IndexDestination::Both
        && (configuration.local_filter.is_some() || configuration.shared_filter.is_some())
}

fn index(
    database_path: &Path,
    models_dir: &Path,
    embeddings_config: Option<semantic::remote::Configuration>,
    full: bool,
    options: &ingestion::IndexOptions,
) -> Result<Response, AppError> {
    let total_started = Instant::now();
    let models = Models::new(models_dir.to_path_buf()).with_remote(embeddings_config);
    models.require_indexer()?;
    let mut model_load_milliseconds = 0;
    let storage_started = Instant::now();
    let mut storage = Storage::open_writer(database_path)?;
    if full || storage.derived_search_is_dirty()? {
        storage.defer_search_updates()?;
    }
    if full {
        storage.mark_derived_search_dirty()?;
    }
    let storage_setup_milliseconds = elapsed_milliseconds(storage_started);
    let ingestion_started = Instant::now();
    let summary = ingestion::index(&mut storage, options)?;
    let ingestion_milliseconds = elapsed_milliseconds(ingestion_started);
    let search_started = Instant::now();
    if storage.derived_search_is_dirty()? {
        emit_index_phase(crate::progress::Phase::SearchRebuild, &summary);
        crate::progress::during(crate::progress::Phase::SearchRebuild, || {
            storage.rebuild_derived_search_state()
        })?;
    }
    let search_refresh_milliseconds = elapsed_milliseconds(search_started);
    emit_index_phase(crate::progress::Phase::SemanticEmbeddings, &summary);
    let embedding_started = Instant::now();
    let generation = models.generation();
    let already_published = storage.semantic_generation_is_published(&generation)?
        && !storage.has_pending_embeddings()?;
    if !already_published {
        storage.invalidate_embedding_generation(&generation)?;
    }
    let embeddings = if !already_published && storage.has_pending_embeddings()? {
        storage.commit_and_continue_writer()?;
        let model_started = Instant::now();
        let mut embedding_pool =
            crate::progress::during(crate::progress::Phase::ModelLoading, || {
                models.load_indexer()
            })?
            .ok_or_else(|| {
                AppError::model("semantic models are not installed; run `mash models install`")
            })?;
        model_load_milliseconds = elapsed_milliseconds(model_started);
        semantic::rebuild_embeddings(
            &mut storage,
            &mut embedding_pool,
            &generation,
            emit_embedding_progress,
        )?
    } else {
        semantic::EmbeddingSummary::default()
    };
    let embedding_milliseconds = elapsed_milliseconds(embedding_started);
    let counts = storage.counts()?;
    if !already_published {
        crate::progress::during(crate::progress::Phase::SemanticFinalize, || {
            storage.mark_semantic_index_ready(&generation)
        })?;
    }
    storage.commit_writer()?;
    if full {
        crate::progress::during(crate::progress::Phase::DatabaseCheckpoint, || {
            storage.truncate_wal()
        })?;
    }
    emit_index_phase(crate::progress::Phase::LocalComplete, &summary);
    Ok(Response::Index(IndexResponse {
        indexed_conversations: counts.conversations,
        indexed_messages: counts.messages,
        searchable_messages: counts.searchable_messages,
        scanned_files: summary.scanned_files,
        malformed_records: summary.malformed_records,
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
        realized_mode: SearchMode::Hybrid,
        model_load_milliseconds,
        storage_setup_milliseconds,
        ingestion_milliseconds,
        search_refresh_milliseconds,
        embedding_milliseconds,
        total_milliseconds: elapsed_milliseconds(total_started),
        shared: None,
    }))
}

fn elapsed_milliseconds(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn emit_index_phase(phase: crate::progress::Phase, summary: &ingestion::IndexSummary) {
    let event =
        crate::progress::Event::files(phase, summary, 0, None, crate::progress::EtaScope::Phase);
    crate::progress::emit(&event);
}

fn emit_embedding_progress(progress: semantic::EmbeddingProgress) {
    crate::progress::emit(&embedding_progress_event(&progress));
}

#[cfg(test)]
fn write_embedding_progress(
    writer: &mut dyn Write,
    progress: &semantic::EmbeddingProgress,
) -> Result<(), AppError> {
    let event = embedding_progress_event(progress);
    serde_json::to_writer(&mut *writer, &event)
        .map_err(|error| AppError::internal(format!("failed to serialize progress: {error}")))?;
    writeln!(writer)
        .map_err(|error| AppError::internal(format!("failed to write progress: {error}")))
}

fn embedding_progress_event(progress: &semantic::EmbeddingProgress) -> crate::progress::Event {
    crate::progress::Event {
        stored_vectors: Some(progress.stored_vectors),
        total_vectors: Some(progress.total_vectors),
        model_inferences: Some(progress.model_inferences),
        reused_vectors: Some(progress.reused_vectors),
        stored_vectors_per_second: Some(progress.stored_vectors_per_second),
        ..crate::progress::Event::new(
            crate::progress::Phase::SemanticEmbeddings,
            progress.elapsed_milliseconds,
            progress.eta_seconds,
        )
    }
}

fn local_search(
    database_path: &Path,
    models_dir: &Path,
    embeddings_config: Option<semantic::remote::Configuration>,
    query: String,
    options: (usize, crate::search_results::SearchUnit),
    filters: &crate::search_filter::Scope,
) -> Result<SearchResponse, AppError> {
    let models = Models::new(models_dir.to_path_buf()).with_remote(embeddings_config);
    models.require_installed()?;
    let storage = Storage::open_search(database_path, &models.generation())?;
    let mut backend = models.load()?.ok_or_else(|| {
        AppError::model("semantic models are not installed; run `mash models install`")
    })?;
    let results = semantic::hybrid_search_filtered(
        &models.generation(),
        &storage,
        &mut backend,
        &query,
        options,
        (None, None),
        Some(filters),
    )?;
    Ok(SearchResponse {
        realized_mode: SearchMode::Hybrid,
        unit: options.1,
        results: crate::search_results::collect(results, options.1, options.0, &query),
        query,
        shared: false,
        shared_error: None,
    })
}

fn resolve_index_options(
    configuration: &ResolvedConfig,
    requested_providers: &[Provider],
    since_days: Option<u32>,
    all_history: bool,
) -> Result<ingestion::IndexOptions, AppError> {
    if since_days.is_some() && all_history {
        return Err(AppError::usage(
            "--since-days and --all-history cannot be used together",
        ));
    }
    if matches!(since_days, Some(days) if !(1..=36_500).contains(&days)) {
        return Err(AppError::usage("--since-days must be between 1 and 36500"));
    }

    let explicit = !requested_providers.is_empty();
    let mut select_claude = !explicit && configuration.providers.claude_code.is_some();
    let mut select_codex = !explicit && configuration.providers.codex.is_some();
    for provider in requested_providers {
        match provider {
            Provider::ClaudeCode => select_claude = true,
            Provider::Codex => select_codex = true,
        }
    }

    let claude_code = if select_claude {
        Some(
            configuration
                .providers
                .claude_code
                .clone()
                .ok_or_else(|| AppError::usage("provider is not enabled: claude-code"))?,
        )
    } else {
        None
    };
    let codex = if select_codex {
        Some(
            configuration
                .providers
                .codex
                .clone()
                .ok_or_else(|| AppError::usage("provider is not enabled: codex"))?,
        )
    } else {
        None
    };

    Ok(ingestion::IndexOptions {
        claude_code,
        codex,
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
            roots.extend(selected.claude_code.into_iter().flatten());
            roots.extend(selected.codex.into_iter().flatten());
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

fn local_view(database_path: &Path, id: String, context: u32) -> Result<ViewResponse, AppError> {
    let storage = Storage::open_existing(database_path)?;
    let messages = storage.view(&id, context)?;
    Ok(ViewResponse { id, messages })
}

fn status(
    database_path: &Path,
    models_dir: &Path,
    configuration: &ResolvedConfig,
) -> Result<Response, AppError> {
    let models_installed = Models::new(models_dir.to_path_buf())
        .with_remote(configuration.embeddings.clone())
        .is_installed();
    if !database_path.is_file() {
        return Ok(Response::Status(StatusResponse {
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
        }));
    }

    let snapshot = Storage::status_snapshot(
        database_path,
        &semantic::configured_generation(configuration.embeddings.as_ref()),
    )?;
    let counts = snapshot.counts;
    let current_embeddings = snapshot.current_embeddings;
    let hybrid_ready =
        models_installed && snapshot.derived_clean && snapshot.exact_semantic_coverage;
    Ok(Response::Status(StatusResponse {
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
    }))
}

fn install_models(
    models_dir: &Path,
    embeddings: Option<semantic::remote::Configuration>,
) -> Result<Response, AppError> {
    let summary = Models::new(models_dir.to_path_buf())
        .with_remote(embeddings)
        .install()?;
    Ok(Response::ModelsInstall(ModelsInstallResponse {
        installed: true,
        model_directory: models_dir.to_path_buf(),
        summary,
        recommended_action: crate::Action::Index,
    }))
}

fn forget(database_path: &Path, id: String) -> Result<Response, AppError> {
    let mut storage = Storage::open_existing(database_path)?;
    let forgotten = storage.forget(&id)?;
    Ok(Response::Forget(ForgetResponse {
        id,
        forgotten,
        shared: None,
    }))
}

pub(crate) fn default_database_path() -> PathBuf {
    ProjectDirs::from("dev", "jamesqo", "mash").map_or_else(
        || PathBuf::from("mash.sqlite3"),
        |directories| directories.data_local_dir().join("mash.sqlite3"),
    )
}

pub(crate) fn default_models_path() -> PathBuf {
    ProjectDirs::from("dev", "jamesqo", "mash").map_or_else(
        || PathBuf::from("models"),
        |directories| directories.data_local_dir().join("models"),
    )
}

fn search_mode(failed: bool, shared_only: bool) -> SearchMode {
    if failed {
        SearchMode::LocalHybrid
    } else if shared_only {
        SearchMode::Hybrid
    } else {
        SearchMode::MergedHybrid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedding_progress_is_a_monotonic_newline_delimited_json_stream() {
        let first = semantic::EmbeddingProgress {
            stored_vectors: 32,
            total_vectors: 128,
            model_inferences: 24,
            reused_vectors: 8,
            elapsed_milliseconds: 1_000,
            stored_vectors_per_second: 32.0,
            eta_seconds: Some(3),
        };
        let second = semantic::EmbeddingProgress {
            stored_vectors: 64,
            total_vectors: 128,
            model_inferences: 48,
            reused_vectors: 16,
            elapsed_milliseconds: 2_000,
            stored_vectors_per_second: 32.0,
            eta_seconds: Some(2),
        };
        let mut output = Vec::new();

        write_embedding_progress(&mut output, &first).expect("first progress JSON");
        write_embedding_progress(&mut output, &second).expect("second progress JSON");

        assert_eq!(output.last(), Some(&b'\n'));
        let values = output
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice::<serde_json::Value>(line).expect("progress JSON"))
            .collect::<Vec<_>>();
        assert_eq!(values.len(), 2);
        assert!(values[0]["stored_vectors"].as_u64() < values[1]["stored_vectors"].as_u64());
        assert!(values[0]["model_inferences"].as_u64() < values[1]["model_inferences"].as_u64());
        assert_eq!(values[0]["total_vectors"], values[1]["total_vectors"]);
        let value = &values[1];
        assert_eq!(value["event"], "index-progress");
        assert_eq!(value["phase"], "semantic-embeddings");
        assert_eq!(value["stored_vectors"], 64);
        assert_eq!(value["total_vectors"], 128);
        assert_eq!(value["model_inferences"], 48);
        assert_eq!(value["reused_vectors"], 16);
        assert_eq!(value["elapsed_milliseconds"], 2_000);
        assert_eq!(value["stored_vectors_per_second"], 32.0);
        assert_eq!(value["eta_seconds"], 2);
    }

    fn test_configuration(
        claude_enabled: bool,
        codex_enabled: bool,
        since_days: Option<u32>,
    ) -> ResolvedConfig {
        let root = std::env::temp_dir().join("mash-cli-options");
        ResolvedConfig {
            embeddings: None,
            path: root.join("config.toml"),
            loaded: true,
            shared: None,
            providers: crate::config::ResolvedProviders {
                claude_code: claude_enabled.then(|| vec![root.join("claude")]),
                codex: codex_enabled.then(|| vec![root.join("codex")]),
            },
            since_days,
            destination: config::IndexDestination::Local,
            local_filter: None,
            shared_filter: None,
        }
    }

    #[test]
    fn index_options_validate_deduplicate_and_apply_horizon_precedence() {
        let configuration = test_configuration(true, true, Some(90));
        let selected = resolve_index_options(
            &configuration,
            &[Provider::Codex, Provider::Codex],
            Some(30),
            false,
        )
        .expect("selected options");
        assert!(selected.claude_code.is_none());
        assert_eq!(selected.codex.unwrap().len(), 1);
        assert_eq!(selected.since_days, Some(30));

        let all =
            resolve_index_options(&configuration, &[], None, true).expect("all-history options");
        assert_eq!(all.since_days, None);
        assert!(all.claude_code.is_some());
        assert!(all.codex.is_some());

        for invalid in [Some(0), Some(36_501)] {
            assert!(resolve_index_options(&configuration, &[], invalid, false).is_err());
        }
        assert!(resolve_index_options(&configuration, &[], Some(1), true).is_err());
        assert!("opencode".parse::<Provider>().is_err());
        assert!(
            resolve_index_options(
                &test_configuration(false, true, Some(90)),
                &[Provider::ClaudeCode],
                None,
                false,
            )
            .is_err()
        );
    }
}
