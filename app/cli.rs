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

#[derive(Debug, Parser)]
#[command(name = "mash", version, about, arg_required_else_help = true)]
struct Cli {
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

#[derive(Debug, Subcommand)]
enum Command {
    /// Discover and index supported local histories.
    Index {
        /// Recreate all derived search state.
        #[arg(long)]
        full: bool,
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
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        days: Option<u32>,
        /// Query the joint PostgreSQL index instead of the local index.
        #[arg(long)]
        shared: bool,
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
    Status,
    /// Remove one conversation and its derived search rows.
    Forget { id: String },
    /// Manage semantic retrieval models.
    Models {
        #[command(subcommand)]
        command: ModelsCommand,
    },
}

#[derive(Debug, Subcommand)]
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

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct SearchResponse {
    pub(crate) query: String,
    pub(crate) realized_mode: String,
    pub(crate) results: Vec<crate::storage::SearchHit>,
    pub(crate) shared: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ViewResponse {
    pub(crate) id: String,
    pub(crate) messages: Vec<crate::storage::Message>,
}

#[derive(Debug, Serialize)]
pub(super) struct StatusResponse {
    ready: bool,
    database_path: PathBuf,
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
    let cli = Cli::try_parse_from(args).map_err(|error| AppError::usage(error.to_string()))?;
    let resolved_config = config::load(cli.config.as_deref())?;
    let database_path = cli.db.unwrap_or_else(default_database_path);
    let models_dir = cli.models_dir.unwrap_or_else(default_models_path);
    dispatch(cli.command, &resolved_config, &database_path, &models_dir)
}

fn dispatch(
    command: Command,
    resolved_config: &ResolvedConfig,
    database_path: &Path,
    models_dir: &Path,
) -> Result<Response, AppError> {
    match command {
        Command::Index {
            full,
            provider,
            since_days,
            all_history,
        } => {
            let options =
                resolve_index_options(resolved_config, &provider, since_days, all_history)?;
            index(
                database_path,
                models_dir,
                full,
                &options,
                resolved_config.shared.as_ref(),
            )
        }
        Command::Search {
            query,
            limit,
            provider,
            days,
            shared: use_shared,
        } => {
            let provider = normalize_provider_filter(provider.as_deref())?;
            if use_shared {
                let configuration = resolved_config.shared.as_ref().ok_or_else(|| {
                    AppError::configuration("--shared requires shared configuration")
                })?;
                shared::search(configuration, models_dir, &query, limit, provider, days).map(
                    |results| {
                        Response::Search(SearchResponse {
                            query,
                            realized_mode: "hybrid".to_owned(),
                            results,
                            shared: true,
                        })
                    },
                )
            } else {
                local_search(database_path, models_dir, query, limit, provider, days)
                    .map(Response::Search)
            }
        }
        Command::View {
            id,
            context,
            shared: node,
        } => {
            if let Some(node) = node {
                let configuration = resolved_config.shared.as_ref().ok_or_else(|| {
                    AppError::configuration("--shared requires shared configuration")
                })?;
                shared::view(configuration, &node, &id, context)
                    .map(|messages| Response::View(ViewResponse { id, messages }))
            } else {
                local_view(database_path, id, context).map(Response::View)
            }
        }
        Command::Status => status(database_path, models_dir, resolved_config),
        Command::Forget { id } => forget(database_path, id, resolved_config.shared.as_ref()),
        Command::Models {
            command: ModelsCommand::Install,
        } => install_models(models_dir),
    }
}

fn index(
    database_path: &Path,
    models_dir: &Path,
    full: bool,
    options: &ingestion::IndexOptions,
    shared_config: Option<&config::SharedConfig>,
) -> Result<Response, AppError> {
    let total_started = Instant::now();
    let models = Models::new(models_dir.to_path_buf());
    models.require_installed()?;
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
        storage.rebuild_derived_search_state()?;
    }
    let search_refresh_milliseconds = elapsed_milliseconds(search_started);
    emit_index_phase("semantic-embeddings", &summary);
    let embedding_started = Instant::now();
    let generation = semantic::embedding_generation();
    let already_published = storage.semantic_generation_is_published(generation)?
        && !storage.has_pending_embeddings()?;
    if !already_published {
        storage.invalidate_embedding_generation(generation)?;
    }
    let embeddings = if !already_published && storage.has_pending_embeddings()? {
        storage.commit_and_continue_writer()?;
        let model_started = Instant::now();
        let mut embedding_pool = models.load_indexer()?.ok_or_else(|| {
            AppError::model("semantic models are not installed; run `mash models install`")
        })?;
        model_load_milliseconds = elapsed_milliseconds(model_started);
        semantic::rebuild_embeddings(&mut storage, &mut embedding_pool, emit_embedding_progress)?
    } else {
        semantic::EmbeddingSummary::default()
    };
    let embedding_milliseconds = elapsed_milliseconds(embedding_started);
    let counts = storage.counts()?;
    if !already_published {
        storage.mark_semantic_index_ready(generation)?;
    }
    storage.commit_writer()?;
    if full {
        storage.truncate_wal()?;
    }
    emit_index_phase("complete", &summary);
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
        shared: shared_config.map(|config| shared::publish(&mut storage, config)),
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
    let stderr = std::io::stderr();
    let mut stderr = stderr.lock();
    if serde_json::to_writer(&mut stderr, &event).is_ok() {
        let _ = writeln!(stderr);
    }
}

fn emit_embedding_progress(progress: semantic::EmbeddingProgress) {
    let stderr = std::io::stderr();
    let mut stderr = stderr.lock();
    let _ = write_embedding_progress(&mut stderr, &progress);
}

fn write_embedding_progress(
    writer: &mut dyn Write,
    progress: &semantic::EmbeddingProgress,
) -> Result<(), AppError> {
    let event = serde_json::json!({
        "event": "index-progress",
        "phase": "semantic-embeddings",
        "stored_vectors": progress.stored_vectors,
        "total_vectors": progress.total_vectors,
        "model_inferences": progress.model_inferences,
        "reused_vectors": progress.reused_vectors,
        "elapsed_milliseconds": progress.elapsed_milliseconds,
        "stored_vectors_per_second": progress.stored_vectors_per_second,
    });
    serde_json::to_writer(&mut *writer, &event)
        .map_err(|error| AppError::internal(format!("failed to serialize progress: {error}")))?;
    writeln!(writer)
        .map_err(|error| AppError::internal(format!("failed to write progress: {error}")))
}

fn local_search(
    database_path: &Path,
    models_dir: &Path,
    query: String,
    limit: usize,
    provider: Option<&str>,
    days: Option<u32>,
) -> Result<SearchResponse, AppError> {
    let provider = normalize_provider_filter(provider)?;
    let models = Models::new(models_dir.to_path_buf());
    models.require_installed()?;
    let storage = Storage::open_existing(database_path)?;
    if storage.derived_search_is_dirty()?
        || !storage.has_messages()?
        || !storage.semantic_index_is_ready(semantic::embedding_generation())?
    {
        return Err(AppError::search_not_ready(
            "semantic index is incomplete; run `mash index`",
        ));
    }
    let mut backend = models.load()?.ok_or_else(|| {
        AppError::model("semantic models are not installed; run `mash models install`")
    })?;
    let results = semantic::hybrid_search(&storage, &mut backend, &query, limit, provider, days)?;
    Ok(SearchResponse {
        query,
        realized_mode: "hybrid".to_owned(),
        results,
        shared: false,
    })
}

fn normalize_provider_filter(provider: Option<&str>) -> Result<Option<&'static str>, AppError> {
    match provider {
        None => Ok(None),
        Some("claude-code") => Ok(Some("claude-code")),
        Some("codex") => Ok(Some("codex")),
        Some(provider) => Err(AppError::usage(format!(
            "unsupported provider filter: {provider}"
        ))),
    }
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
    let models_installed = Models::new(models_dir.to_path_buf()).is_installed();
    if !database_path.is_file() {
        return Ok(Response::Status(StatusResponse {
            ready: false,
            database_path: database_path.to_path_buf(),
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
            shared: configuration
                .shared
                .as_ref()
                .map(|_| shared::Publication::pending()),
        }));
    }

    let snapshot = Storage::status_snapshot(database_path, semantic::embedding_generation())?;
    let counts = snapshot.counts;
    let current_embeddings = snapshot.current_embeddings;
    let hybrid_ready = models_installed
        && snapshot.derived_clean
        && counts.messages > 0
        && snapshot.exact_semantic_coverage;
    Ok(Response::Status(StatusResponse {
        ready: hybrid_ready,
        database_path: database_path.to_path_buf(),
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
        shared: shared::status(database_path, configuration.shared.as_ref())?,
    }))
}

fn install_models(models_dir: &Path) -> Result<Response, AppError> {
    let summary = Models::new(models_dir.to_path_buf()).install()?;
    Ok(Response::ModelsInstall(ModelsInstallResponse {
        installed: true,
        model_directory: models_dir.to_path_buf(),
        summary,
        recommended_action: "index",
    }))
}

fn forget(
    database_path: &Path,
    id: String,
    shared_config: Option<&config::SharedConfig>,
) -> Result<Response, AppError> {
    let mut storage = Storage::open_existing(database_path)?;
    let forgotten = storage.forget(&id)?;
    Ok(Response::Forget(ForgetResponse {
        id,
        forgotten,
        shared: shared_config.map(|config| shared::publish(&mut storage, config)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use veritas_test_macros as veritas;

    #[veritas::claims("semantic-indexing/progress-is-monotonic")]
    #[test]
    fn embedding_progress_is_a_monotonic_newline_delimited_json_stream() {
        let first = semantic::EmbeddingProgress {
            stored_vectors: 32,
            total_vectors: 128,
            model_inferences: 24,
            reused_vectors: 8,
            elapsed_milliseconds: 1_000,
            stored_vectors_per_second: 32.0,
        };
        let second = semantic::EmbeddingProgress {
            stored_vectors: 64,
            total_vectors: 128,
            model_inferences: 48,
            reused_vectors: 16,
            elapsed_milliseconds: 2_000,
            stored_vectors_per_second: 32.0,
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
    }

    fn test_configuration(
        claude_enabled: bool,
        codex_enabled: bool,
        since_days: Option<u32>,
    ) -> ResolvedConfig {
        let root = std::env::temp_dir().join("mash-cli-options");
        ResolvedConfig {
            path: root.join("config.json"),
            loaded: true,
            shared: None,
            providers: crate::config::ResolvedProviders {
                claude_code: claude_enabled.then(|| vec![root.join("claude")]),
                codex: codex_enabled.then(|| vec![root.join("codex")]),
            },
            since_days,
        }
    }

    #[veritas::claims("cli/operational-command-surface")]
    #[test]
    fn command_surface_contains_only_the_contract_commands() {
        let command = Cli::command();
        let names: Vec<_> = command
            .get_subcommands()
            .map(|command| command.get_name().to_owned())
            .collect();
        assert_eq!(
            names,
            ["index", "search", "view", "status", "forget", "models"]
        );
    }

    #[veritas::claims(
        "configuration/cli-values-have-precedence",
        "indexing/cli-provider-selection-is-bounded"
    )]
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
