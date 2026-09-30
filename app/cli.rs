use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[cfg(test)]
use clap::CommandFactory;
use clap::{Parser, Subcommand};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::AppError;
use crate::config::{self, ConfigurationStatus, ResolvedConfig};
use crate::ingestion;
use crate::semantic::{self, Models};
use crate::shared;
use crate::storage::Storage;

#[derive(Debug, Clone, Deserialize, Serialize, Parser)]
#[serde(deny_unknown_fields)]
#[command(name = "mash", version, about, arg_required_else_help = true)]
pub(crate) struct Cli {
    /// Versioned mash node and provider configuration.
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Canonical mash SQLite database.
    #[arg(long, global = true, value_name = "PATH")]
    db: Option<PathBuf>,

    /// Semantic model asset directory.
    #[arg(long, global = true, value_name = "PATH")]
    models_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, Deserialize, Serialize, Subcommand)]
enum Command {
    /// Discover and index supported local histories.
    Index {
        /// Recreate all derived search state.
        #[arg(long, conflicts_with = "watch")]
        full: bool,
        /// Keep indexes synchronized until interrupted.
        #[arg(long)]
        watch: bool,
        /// Quiet period before a watch refresh (milliseconds).
        #[arg(long, requires = "watch", default_value = "1000", value_parser = clap::value_parser!(u64).range(50..=60_000))]
        debounce_ms: u64,
        /// Restrict indexing to a supported provider.
        #[arg(long, value_name = "PROVIDER")]
        provider: Vec<String>,
        /// Admit sources modified within this many days.
        #[arg(long, value_name = "DAYS")]
        since_days: Option<u32>,
        /// Admit source history regardless of age.
        #[arg(long)]
        all_history: bool,
    },
    /// Search indexed messages.
    Search {
        query: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[command(flatten)]
        filters: Box<crate::search_filter::Arguments>,
        /// Query only the shared index (including this node).
        #[arg(long)]
        shared: bool,
        /// Query only SQLite, without contacting the shared service.
        #[arg(long, conflicts_with = "shared")]
        local: bool,
        /// Run in this process without the shared idle search worker.
        #[arg(long)]
        no_worker: bool,
    },
    /// Return a message and adjacent context.
    View {
        id: String,
        #[arg(long, default_value_t = 0)]
        context: u32,
        /// Read shared context for this publishing node.
        #[arg(long, value_name = "NODE_ID")]
        shared: Option<String>,
    },
    /// Report canonical storage and model readiness.
    Status {
        /// Inspect per-node freshness in the shared index.
        #[arg(long)]
        shared: bool,
    },
    /// Remove one conversation and its derived search rows.
    Forget { id: String },
    /// Manage semantic retrieval models.
    Models {
        #[command(subcommand)]
        command: ModelsCommand,
    },
    /// Serve search tools over the stdio MCP protocol.
    Mcp {
        /// Synchronize histories for this MCP session's lifetime.
        #[arg(long)]
        watch: bool,
        /// Quiet period before a watch refresh (milliseconds).
        #[arg(long, requires = "watch", default_value = "1000", value_parser = clap::value_parser!(u64).range(50..=60_000))]
        debounce_ms: u64,
    },
    /// Serve the shared-index HTTP API using a server configuration file.
    Serve {
        #[arg(long, value_name = "PATH")]
        server_config: PathBuf,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, Subcommand)]
enum ModelsCommand {
    /// Explicitly install embedding and reranking models.
    Install,
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
    WatchStopped { watching: bool },
    McpStopped,
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
    realized_mode: &'static str,
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
pub(crate) struct SearchResponse {
    pub(crate) query: String,
    pub(crate) realized_mode: String,
    pub(crate) results: Vec<crate::storage::SearchHit>,
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
    realized_mode: &'static str,
    recommended_action: Option<&'static str>,
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
    recommended_action: &'static str,
}

pub(super) fn run<I, T>(args: I) -> Result<Response, AppError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let arguments: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let cli =
        Cli::try_parse_from(&arguments).map_err(|error| AppError::usage(error.to_string()))?;
    let mut builder = if matches!(cli.command, Command::Serve { .. }) {
        tokio::runtime::Builder::new_multi_thread()
    } else {
        tokio::runtime::Builder::new_current_thread()
    };
    let runtime = builder
        .enable_all()
        .build()
        .map_err(|_| AppError::internal("failed to start async runtime"))?;
    if let Command::Serve { server_config } = cli.command {
        runtime.block_on(crate::shared::server::run(&server_config))?;
        Ok(Response::McpStopped)
    } else if let Command::Mcp { watch, debounce_ms } = cli.command {
        let mut startup = vec![OsString::from("mash")];
        for (name, path) in [
            ("--config", cli.config),
            ("--db", cli.db),
            ("--models-dir", cli.models_dir),
        ] {
            if let Some(path) = path {
                startup.extend([name.into(), path.into_os_string()]);
            }
        }
        runtime.block_on(crate::mcp::run(startup, watch, debounce_ms))?;
        Ok(Response::McpStopped)
    } else {
        #[cfg(unix)]
        if matches!(
            cli.command,
            Command::Search {
                no_worker: false,
                ..
            }
        ) {
            return runtime.block_on(process_search(cli));
        }
        runtime.block_on(run_async(arguments))
    }
}

#[cfg(unix)]
async fn process_search(mut cli: Cli) -> Result<Response, AppError> {
    let path = cli.config.clone();
    let configuration = crate::blocking(move || config::load(path.as_deref())).await?;
    let cwd = std::env::current_dir().map_err(AppError::io)?;
    let absolute = |path: PathBuf| {
        if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        }
    };
    cli.config = cli
        .config
        .map(absolute)
        .or_else(|| configuration.loaded.then(|| configuration.path.clone()));
    cli.db = Some(absolute(cli.db.unwrap_or_else(default_database_path)));
    let models_dir = absolute(cli.models_dir.unwrap_or_else(default_models_path));
    cli.models_dir = Some(models_dir.clone());
    let models = Models::new(models_dir).with_remote(configuration.embeddings.clone());
    if !models.is_installed() {
        // Preserve direct validation/error ordering without starting an idle
        // process for a missing or incomplete model installation.
        return run_parsed(cli).await;
    }
    let mut environment = vec![
        "HOME".to_owned(),
        "XDG_CONFIG_HOME".to_owned(),
        "XDG_DATA_HOME".to_owned(),
        "HTTP_PROXY".to_owned(),
        "HTTPS_PROXY".to_owned(),
        "ALL_PROXY".to_owned(),
        "NO_PROXY".to_owned(),
        "http_proxy".to_owned(),
        "https_proxy".to_owned(),
        "all_proxy".to_owned(),
        "no_proxy".to_owned(),
        "SSL_CERT_FILE".to_owned(),
    ];
    if let Some(shared) = &configuration.shared {
        environment.push(shared.token_env.clone());
    }
    if let Some(embeddings) = &configuration.embeddings
        && let Some(name) = &embeddings.token_env
    {
        environment.push(name.clone());
    }
    let Ok(profile) = serde_json::to_vec(&(&cli.config, &cli.db, &cli.models_dir)) else {
        return run_parsed(cli).await;
    };
    if let Some(result) = crate::worker::search(&cli, &profile, &environment).await {
        return result.map(Response::Search);
    }
    run_parsed(cli).await
}

#[cfg(unix)]
pub(crate) async fn worker_search(cli: Cli) -> Result<SearchResponse, AppError> {
    if !matches!(cli.command, Command::Search { .. }) {
        return Err(AppError::usage("search worker accepts only searches"));
    }
    match run_parsed(cli).await? {
        Response::Search(response) => Ok(response),
        _ => Err(AppError::internal("unexpected search worker response")),
    }
}

pub(super) async fn run_async<I, T>(args: I) -> Result<Response, AppError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let arguments: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let mut cli =
        Cli::try_parse_from(&arguments).map_err(|error| AppError::usage(error.to_string()))?;
    if let Command::Index {
        watch, debounce_ms, ..
    } = &mut cli.command
        && *watch
    {
        *watch = false;
        let debounce = *debounce_ms;
        let (stop, shutdown) = tokio::sync::watch::channel(false);
        let signal = tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                let _ = stop.send(true);
            }
        });
        let result = crate::watch::run(arguments, debounce, shutdown, move || {
            let cli = cli.clone();
            async move {
                let response = run_parsed(cli).await?;
                let retry = response.needs_retry();
                let mut stdout = std::io::stdout();
                crate::write_json(&mut stdout, &response)?;
                stdout
                    .flush()
                    .map_err(|_| AppError::internal("flush watch output"))?;
                Ok(retry)
            }
        })
        .await;
        signal.abort();
        result?;
        return Ok(Response::WatchStopped { watching: false });
    }
    run_parsed(cli).await
}

async fn run_parsed(cli: Cli) -> Result<Response, AppError> {
    let path = cli.config;
    let resolved_config = crate::blocking(move || config::load(path.as_deref())).await?;
    let database_path = cli.db.unwrap_or_else(default_database_path);
    let models_dir = cli.models_dir.unwrap_or_else(default_models_path);
    dispatch(cli.command, &resolved_config, &database_path, &models_dir).await
}

async fn dispatch(
    command: Command,
    configuration: &ResolvedConfig,
    database: &Path,
    models: &Path,
) -> Result<Response, AppError> {
    match command {
        Command::Mcp { .. } | Command::Serve { .. } => Err(AppError::usage(
            "server commands must run as process commands",
        )),
        Command::Index {
            full,
            provider,
            since_days,
            all_history,
            ..
        } => {
            let options = resolve_index_options(configuration, &provider, since_days, all_history)?;
            index_routes(configuration, database, models, full, &options).await
        }
        Command::Search {
            query,
            limit,
            filters,
            shared: use_shared,
            local: use_local,
            ..
        } => {
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
                    limit,
                    &filters,
                )
                .await?;
                Ok(Response::Search(SearchResponse {
                    query,
                    realized_mode: search_mode(shared_error.is_some(), use_shared).to_owned(),
                    results,
                    shared: shared_error.is_none(),
                    shared_error,
                }))
            } else {
                let database = database.to_path_buf();
                let models = models.to_path_buf();
                let embeddings = configuration.embeddings.clone();
                crate::blocking(move || {
                    local_search(&database, &models, embeddings, query, limit, &filters)
                        .map(Response::Search)
                })
                .await
            }
        }
        Command::View {
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
        Command::Status {
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
        Command::Forget { id } => forget_routes(database, configuration, id).await,
        Command::Models {
            command: ModelsCommand::Install,
        } => {
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
        crate::progress::emit(&serde_json::json!({"event":"index-progress",
            "phase":if result.shared.as_ref().is_some_and(shared::Publication::is_pending) { "shared-pending" } else { "complete" },
            "elapsed_milliseconds":result.total_milliseconds,
            "eta_seconds":0,"eta_scope":"phase"}));
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
        emit_index_phase("search-rebuild", &summary);
        crate::progress::during("search-rebuild", || storage.rebuild_derived_search_state())?;
    }
    let search_refresh_milliseconds = elapsed_milliseconds(search_started);
    emit_index_phase("semantic-embeddings", &summary);
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
            crate::progress::during("model-loading", || models.load_indexer())?.ok_or_else(
                || AppError::model("semantic models are not installed; run `mash models install`"),
            )?;
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
        crate::progress::during("semantic-finalize", || {
            storage.mark_semantic_index_ready(&generation)
        })?;
    }
    storage.commit_writer()?;
    if full {
        crate::progress::during("database-checkpoint", || storage.truncate_wal())?;
    }
    emit_index_phase("local-complete", &summary);
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
        realized_mode: "hybrid",
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

fn emit_index_phase(phase: &'static str, summary: &ingestion::IndexSummary) {
    let event = serde_json::json!({
        "event": "index-progress",
        "phase": phase,
        "processed_files": summary.processed_files,
        "scanned_files": summary.scanned_files,
        "processed_bytes": summary.processed_bytes,
        "discovered_bytes": summary.discovered_bytes,
        "changed_messages": summary.changed_messages,
        "committed_batches": summary.committed_batches,
    });
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

fn embedding_progress_event(progress: &semantic::EmbeddingProgress) -> serde_json::Value {
    serde_json::json!({
        "event": "index-progress",
        "phase": "semantic-embeddings",
        "stored_vectors": progress.stored_vectors,
        "total_vectors": progress.total_vectors,
        "model_inferences": progress.model_inferences,
        "reused_vectors": progress.reused_vectors,
        "elapsed_milliseconds": progress.elapsed_milliseconds,
        "stored_vectors_per_second": progress.stored_vectors_per_second,
        "eta_seconds": progress.eta_seconds,
        "eta_scope": "phase",
    })
}

fn local_search(
    database_path: &Path,
    models_dir: &Path,
    embeddings_config: Option<semantic::remote::Configuration>,
    query: String,
    limit: usize,
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
        limit,
        (None, None),
        Some(filters),
    )?;
    Ok(SearchResponse {
        query,
        realized_mode: "hybrid".to_owned(),
        results,
        shared: false,
        shared_error: None,
    })
}

fn resolve_index_options(
    configuration: &ResolvedConfig,
    requested_providers: &[String],
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
        match provider.as_str() {
            "claude-code" => select_claude = true,
            "codex" => select_codex = true,
            _ => {
                return Err(AppError::usage(format!(
                    "unsupported index provider: {provider}"
                )));
            }
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

pub(crate) fn watch_targets(arguments: &[OsString]) -> Result<crate::watch::Targets, AppError> {
    let cli = Cli::try_parse_from(arguments).map_err(|e| AppError::usage(e.to_string()))?;
    let configuration = config::load(cli.config.as_deref())?;
    let Command::Index {
        provider,
        since_days,
        all_history,
        ..
    } = cli.command
    else {
        return Err(AppError::usage("watch requires index arguments"));
    };
    let options = resolve_index_options(&configuration, &provider, since_days, all_history)?;
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
    let models = cli.models_dir.unwrap_or_else(default_models_path);
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
            realized_mode: "unavailable",
            recommended_action: Some(if models_installed {
                "index"
            } else {
                "models install"
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
    let hybrid_ready = models_installed
        && snapshot.derived_clean
        && counts.messages > 0
        && snapshot.exact_semantic_coverage;
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
            "hybrid"
        } else {
            "unavailable"
        },
        recommended_action: if !models_installed {
            Some("models install")
        } else if hybrid_ready {
            None
        } else {
            Some("index")
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
        recommended_action: "index",
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

fn default_database_path() -> PathBuf {
    ProjectDirs::from("dev", "jamesqo", "mash").map_or_else(
        || PathBuf::from("mash.sqlite3"),
        |directories| directories.data_local_dir().join("mash.sqlite3"),
    )
}

fn default_models_path() -> PathBuf {
    ProjectDirs::from("dev", "jamesqo", "mash").map_or_else(
        || PathBuf::from("models"),
        |directories| directories.data_local_dir().join("models"),
    )
}

fn search_mode(failed: bool, shared_only: bool) -> &'static str {
    if failed {
        "local-hybrid"
    } else if shared_only {
        "hybrid"
    } else {
        "merged-hybrid"
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
    fn command_surface_contains_only_the_contract_commands() {
        let command = Cli::command();
        let names: Vec<_> = command
            .get_subcommands()
            .map(|command| command.get_name().to_owned())
            .collect();
        assert_eq!(
            names,
            [
                "index", "search", "view", "status", "forget", "models", "mcp", "serve"
            ]
        );
    }

    #[test]
    fn index_options_validate_deduplicate_and_apply_horizon_precedence() {
        let configuration = test_configuration(true, true, Some(90));
        let selected = resolve_index_options(
            &configuration,
            &["codex".to_owned(), "codex".to_owned()],
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
        assert!(
            resolve_index_options(&configuration, &["opencode".to_owned()], None, false).is_err()
        );
        assert!(
            resolve_index_options(
                &test_configuration(false, true, Some(90)),
                &["claude-code".to_owned()],
                None,
                false,
            )
            .is_err()
        );
    }
}
