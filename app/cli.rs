use crate::provider::Provider;
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

#[cfg(test)]
use clap::CommandFactory;
use clap::{Parser, Subcommand};
use serde::Serialize;

use crate::AppError;
use crate::config;
use crate::core::{self, IndexRequest, Operation, Paths, Request};
#[cfg(unix)]
use crate::core::{default_database_path, default_models_path};
#[cfg(unix)]
use crate::semantic::Models;

#[derive(Debug, Clone, Parser)]
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

#[derive(Debug, Clone, Subcommand)]
enum Command {
    /// List thread metadata without inference, ordered by activity, creation or title.
    ListThreads {
        #[command(flatten)]
        input: Box<crate::thread_catalog::Request>,
        #[arg(long, conflicts_with = "shared")]
        local: bool,
        #[arg(long)]
        shared: bool,
    },
    /// Discover and index supported local histories.
    Index {
        /// Build local retrieval state from certified stored silver, offline.
        #[arg(long, conflicts_with_all = ["watch", "provider", "since_days", "all_history", "rebuild_shared_gold"])]
        rebuild_local_gold: bool,
        /// Rebuild this node's shared retrieval state from retained shared silver.
        #[arg(long, conflicts_with_all = ["watch", "provider", "since_days", "all_history", "rebuild_local_gold"])]
        rebuild_shared_gold: bool,
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
        provider: Vec<Provider>,
        /// Admit sources modified within this many days.
        #[arg(long, value_name = "DAYS")]
        since_days: Option<u32>,
        /// Admit source history regardless of age.
        #[arg(long)]
        all_history: bool,
    },
    /// Search indexed threads or messages.
    Search {
        query: String,
        /// Retrieval intent; defaults to gold.query.intent (related-discussion).
        #[arg(long, value_enum)]
        intent: Option<crate::gold::exact::Intent>,
        /// Maximum results; defaults to gold.query.limit (20).
        #[arg(long)]
        limit: Option<usize>,
        /// Result granularity; defaults to gold.query.unit (threads).
        #[arg(long, value_enum)]
        unit: Option<crate::search_results::SearchUnit>,
        /// Override the hybrid fusion rank constant (1–1000).
        #[arg(long)]
        fusion_rank_constant: Option<u16>,
        /// Override the lexical candidate weight (1–100).
        #[arg(long)]
        lexical_weight: Option<u16>,
        /// Override the semantic candidate weight (1–100).
        #[arg(long)]
        semantic_weight: Option<u16>,
        #[command(flatten)]
        filters: Box<SearchArguments>,
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
        /// Explicitly replace persisted shared group gold settings from this TOML.
        #[arg(long)]
        activate_gold: bool,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum ModelsCommand {
    /// Explicitly install embedding and reranking models.
    Install,
}

#[derive(Debug, Clone, Default, clap::Args)]
struct SearchArguments {
    #[arg(long)]
    provider: Vec<Provider>,
    #[arg(long)]
    days: Option<u32>,
    #[arg(long)]
    node: Vec<String>,
    #[arg(long)]
    role: Vec<crate::role::Role>,
    #[arg(long)]
    conversation: Vec<String>,
    /// Exact recorded working directory; repeat for alternatives.
    #[arg(long)]
    project: Vec<String>,
    /// Exact Git origin URL; repeat for alternatives.
    #[arg(long)]
    repository: Vec<String>,
    /// Inclusive date or RFC3339 timestamp.
    #[arg(long)]
    after: Option<String>,
    /// Exclusive date or RFC3339 timestamp.
    #[arg(long)]
    before: Option<String>,
    /// JSON Boolean expression using all/any/not and metadata predicates.
    #[arg(long)]
    filter: Option<String>,
}

impl SearchArguments {
    pub(crate) fn into_selection(self) -> Result<crate::search_filter::Selection, AppError> {
        let filter = self
            .filter
            .map(|json| {
                if json.len() > 65536 {
                    return Err(AppError::usage("search filter exceeds 64 KiB"));
                }
                serde_json::from_str(&json)
                    .map_err(|_| AppError::usage("invalid structured search filter"))
            })
            .transpose()?;
        Ok(crate::search_filter::Selection {
            provider: self.provider,
            days: self.days,
            node: self.node,
            role: self.role,
            conversation: self.conversation,
            project: self.project,
            repository: self.repository,
            after: self
                .after
                .as_deref()
                .map(crate::search_filter::timestamp)
                .transpose()?,
            before: self
                .before
                .as_deref()
                .map(crate::search_filter::timestamp)
                .transpose()?,
            filter,
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(super) enum Response {
    Operation(Box<core::Response>),
    Help { help: String },
    Version { version: &'static str },
    WatchStopped { watching: bool },
    McpStopped,
}

impl Cli {
    fn paths(&self) -> Paths {
        Paths {
            config: self.config.clone(),
            db: self.db.clone(),
            models_dir: self.models_dir.clone(),
        }
    }

    fn into_request(self) -> Result<Request, AppError> {
        let paths = self.paths();
        let operation = match self.command {
            Command::ListThreads {
                input,
                local,
                shared,
            } => Operation::ListThreads {
                input,
                local,
                shared,
            },
            Command::Index {
                rebuild_local_gold,
                rebuild_shared_gold,
                full,
                provider,
                since_days,
                all_history,
                ..
            } => Operation::Index(IndexRequest {
                mode: crate::core::IndexMode::from_rebuild_flags(
                    rebuild_local_gold,
                    rebuild_shared_gold,
                )?,
                full,
                provider,
                since_days,
                all_history,
            }),
            Command::Search {
                query,
                intent,
                limit,
                unit,
                fusion_rank_constant,
                lexical_weight,
                semantic_weight,
                filters,
                shared,
                local,
                ..
            } => Operation::Search {
                query,
                intent,
                limit,
                unit,
                fusion: crate::gold::config::FusionOverrides {
                    rank_constant: fusion_rank_constant,
                    lexical_weight,
                    semantic_weight,
                },
                filters: Box::new(filters.into_selection()?),
                shared,
                local,
            },
            Command::View {
                id,
                context,
                shared,
            } => Operation::View {
                id,
                context,
                shared,
            },
            Command::Status { shared } => Operation::Status { shared },
            Command::Forget { id } => Operation::Forget { id },
            Command::Models {
                command: ModelsCommand::Install,
            } => Operation::ModelsInstall,
            Command::Mcp { .. } | Command::Serve { .. } => {
                return Err(AppError::usage(
                    "server commands must run as process commands",
                ));
            }
        };
        Ok(Request { paths, operation })
    }
}

pub(super) fn run<I, T>(args: I) -> Result<Response, AppError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let arguments: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let cli = match Cli::try_parse_from(&arguments) {
        Ok(cli) => cli,
        Err(error) => match error.kind() {
            clap::error::ErrorKind::DisplayHelp => {
                return Ok(Response::Help {
                    help: error.to_string(),
                });
            }
            clap::error::ErrorKind::DisplayVersion => {
                return Ok(Response::Version {
                    version: env!("CARGO_PKG_VERSION"),
                });
            }
            _ => return Err(AppError::usage(error.to_string())),
        },
    };
    let mut builder = if matches!(cli.command, Command::Serve { .. }) {
        tokio::runtime::Builder::new_multi_thread()
    } else {
        tokio::runtime::Builder::new_current_thread()
    };
    let runtime = builder
        .enable_all()
        .build()
        .map_err(|_| AppError::internal("failed to start async runtime"))?;
    if let Command::Serve {
        server_config,
        activate_gold,
    } = cli.command
    {
        runtime.block_on(crate::shared::server::run(&server_config, activate_gold))?;
        Ok(Response::McpStopped)
    } else if let Command::Mcp { watch, debounce_ms } = cli.command {
        let paths = Paths {
            config: cli.config,
            db: cli.db,
            models_dir: cli.models_dir,
        };
        runtime.block_on(crate::mcp::run(paths, watch, debounce_ms))?;
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
        runtime.block_on(run_parsed(cli))
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
    let request = cli.into_request()?;
    if matches!(&request.operation, Operation::Search { intent, .. }
        if intent.unwrap_or(configuration.gold.query.intent) == crate::gold::exact::Intent::ExactMention)
    {
        return core::execute(request)
            .await
            .map(Box::new)
            .map(Response::Operation);
    }
    core::validate_search_request(&request)?;
    let models = Models::new(models_dir).with_remote(configuration.embeddings.clone());
    if !models.is_installed() {
        // Preserve direct validation/error ordering without starting an idle
        // process for a missing or incomplete model installation.
        return core::execute(request)
            .await
            .map(Box::new)
            .map(Response::Operation);
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
    let Ok(profile) = serde_json::to_vec(&(
        &request.paths.config,
        &request.paths.db,
        &request.paths.models_dir,
    )) else {
        return core::execute(request)
            .await
            .map(Box::new)
            .map(Response::Operation);
    };
    if let Some(result) = crate::worker::search(&request, &profile, &environment).await {
        return result
            .map(|response| Response::Operation(Box::new(core::Response::Search(response))));
    }
    core::execute(request)
        .await
        .map(Box::new)
        .map(Response::Operation)
}

async fn run_parsed(mut cli: Cli) -> Result<Response, AppError> {
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
        let request = cli.into_request()?;
        let result = crate::watch::run(request.clone(), debounce, shutdown, move || {
            let request = request.clone();
            async move {
                let response = core::execute(request).await?;
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
    core::execute(cli.into_request()?)
        .await
        .map(Box::new)
        .map(Response::Operation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_preserves_omitted_defaults_and_explicit_overrides_for_the_core() {
        for (args, expected) in [
            (vec!["mash", "search", "database"], (None, None)),
            (
                vec![
                    "mash", "search", "database", "--unit", "messages", "--limit", "3",
                ],
                (Some(3), Some(crate::search_results::SearchUnit::Messages)),
            ),
        ] {
            let request = Cli::try_parse_from(args).unwrap().into_request().unwrap();
            let Operation::Search {
                limit,
                unit,
                fusion,
                ..
            } = request.operation
            else {
                panic!("search request")
            };
            assert_eq!((limit, unit), expected);
            assert!(fusion.is_empty());
        }
    }

    #[test]
    fn cli_preserves_partial_fusion_overrides_for_the_core() {
        let request = Cli::try_parse_from([
            "mash",
            "search",
            "database",
            "--lexical-weight",
            "4",
            "--fusion-rank-constant",
            "10",
        ])
        .unwrap()
        .into_request()
        .unwrap();
        let Operation::Search { fusion, .. } = request.operation else {
            panic!("search request")
        };
        assert_eq!(fusion.rank_constant, Some(10));
        assert_eq!(fusion.lexical_weight, Some(4));
        assert_eq!(fusion.semantic_weight, None);
    }

    #[test]
    fn cli_translates_dates_and_boolean_filters_into_typed_core_requests() {
        let request = Cli::try_parse_from([
            "mash",
            "search",
            "--local",
            "--after",
            "2026-01-01",
            "--provider",
            "codex",
            "--filter",
            r#"{"not":{"role":["tool"]}}"#,
            "--",
            "--models-dir=untrusted",
        ])
        .unwrap()
        .into_request()
        .unwrap();
        let Operation::Search {
            query,
            filters,
            local,
            shared,
            ..
        } = request.operation
        else {
            panic!("expected search request");
        };
        assert_eq!(query, "--models-dir=untrusted");
        assert!(local);
        assert!(!shared);
        assert_eq!(filters.after, Some(1_767_225_600));
        assert_eq!(filters.provider, [Provider::Codex]);
        assert!(matches!(
            filters.filter,
            Some(crate::search_filter::Filter::Not(_))
        ));
        assert!(request.paths.models_dir.is_none());
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
                "list-threads",
                "index",
                "search",
                "view",
                "status",
                "forget",
                "models",
                "mcp",
                "serve"
            ]
        );
    }
}
