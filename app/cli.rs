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
    /// List chat metadata without inference, ordered by activity, creation or title.
    ListChats {
        #[command(flatten)]
        input: Box<crate::chat_catalog::Request>,
        #[arg(long, conflicts_with = "shared")]
        local: bool,
        #[arg(long)]
        shared: bool,
        /// Include metadata source outcomes and listing/merge timings.
        #[arg(long)]
        diagnostics: bool,
    },
    /// Discover and index supported local histories.
    Index {
        /// Commit an external connector batch to local silver; receipt reports views readiness.
        #[arg(long, value_name = "PATH", conflicts_with_all = ["watch", "full", "rebuild_local_views", "provider", "since_days", "all_history"])]
        connector_batch: Option<PathBuf>,
        /// Build local retrieval state from certified stored silver, offline.
        /// With --full, discard all local views and replay it from silver.
        #[arg(long, conflicts_with_all = ["watch", "provider", "since_days", "all_history"])]
        rebuild_local_views: bool,
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
    /// Search indexed chats or messages.
    Search {
        /// Override dense shared retrieval; omitted inherits the active group default.
        #[arg(long, value_enum)]
        shared_vector_search: Option<crate::views::vector::Algorithm>,
        /// HNSW search effort for this shared override (1–4096, default 200).
        #[arg(long, requires = "shared_vector_search")]
        shared_hnsw_ef: Option<usize>,
        query: String,
        /// Retrieval intent; defaults to views.query.intent (related-discussion).
        #[arg(long, value_enum)]
        intent: Option<crate::views::exact::Intent>,
        /// Maximum results; defaults to views.query.limit (20).
        #[arg(long)]
        limit: Option<usize>,
        /// Result granularity; defaults to views.query.unit (chats).
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
        /// Override how much newer messages are favoured (0–100; 0 is off).
        #[arg(long)]
        recency_weight: Option<u16>,
        /// Override hybrid ranking for this request; defaults to views.query.ranking.
        #[arg(long, value_enum)]
        ranking: Option<crate::views::config::Ranking>,
        /// Include resolved retrieval policy, source outcomes and core timing.
        #[arg(long)]
        diagnostics: bool,
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
        /// Report scoped retained-history and provider scan facts without proving completeness.
        #[arg(long)]
        coverage: bool,
        #[command(flatten)]
        scope: Box<crate::core::coverage::Scope>,
        /// Inspect per-node freshness in the shared index.
        #[arg(long)]
        shared: bool,
    },
    /// Remove one conversation and its derived search rows.
    Forget { id: String },
    /// Export local silver and retained attachments to a new private directory.
    Export { directory: PathBuf },
    /// Import verified silver into a new database; project views explicitly afterward.
    Import {
        directory: PathBuf,
        #[arg(long, value_name = "NEW_DATABASE")]
        into: PathBuf,
        /// Continue the exporting node's shared identity (upgrades only; retire
        /// the exporting database first).
        #[arg(long)]
        keep_identity: bool,
    },
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
        /// Explicitly replace persisted shared group views settings from this TOML.
        #[arg(long)]
        activate_views: bool,
        /// Explicitly replace persisted shared group query defaults without indexing.
        #[arg(long)]
        activate_query: bool,
        /// Explicitly apply the configured durable/cached/cold Qdrant placement.
        #[arg(long)]
        activate_memory: bool,
        /// Rebuild all shared views from shared silver before serving.
        #[arg(long)]
        replay_views: bool,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum ModelsCommand {
    /// Explicitly install embedding and reranking models.
    Install,
}

#[derive(Debug, Clone, Default, clap::Args)]
struct SearchArguments {
    /// Lexical matching: flexible (prefixes and typos), strict, prefix, or typo.
    #[arg(long, value_enum, default_value = "flexible")]
    match_mode: crate::views::matching::Mode,
    /// Order qualifying results by relevance (default) or recent (newest first).
    #[arg(long, value_enum, default_value = "relevance")]
    sort: crate::search_filter::Sort,
    #[arg(long)]
    provider: Vec<Provider>,
    #[arg(long)]
    days: Option<u32>,
    #[arg(long)]
    node: Vec<String>,
    #[arg(long)]
    role: Vec<crate::role::Role>,
    #[arg(long = "chat")]
    conversation: Vec<String>,
    /// Exact recorded working directory; repeat for alternatives.
    #[arg(long)]
    project: Vec<String>,
    /// Exact Git origin URL; repeat for alternatives.
    #[arg(long)]
    repository: Vec<String>,
    /// Connector source ID; repeat for alternatives. Native history never matches.
    #[arg(long)]
    source: Vec<String>,
    /// Object kind label, such as codex:message or slack:message; repeat for alternatives.
    #[arg(long)]
    kind: Vec<String>,
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
            match_mode: self.match_mode,
            sort: self.sort,
            provider: self.provider,
            days: self.days,
            node: self.node,
            role: self.role,
            conversation: self.conversation,
            project: self.project,
            repository: self.repository,
            source: self.source,
            kind: self.kind,
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
    Help {
        help: String,
    },
    /// The release and the silver schema it opens; deployment compares the
    /// latter with an existing database's `user_version` before replacing it.
    Version {
        version: &'static str,
        silver_schema: i64,
    },
    WatchStopped {
        watching: bool,
    },
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

    #[expect(
        clippy::too_many_lines,
        reason = "Keep the mechanical CLI-to-core command mapping together."
    )]
    fn into_request(self) -> Result<Request, AppError> {
        let paths = self.paths();
        let operation = match self.command {
            Command::ListChats {
                input,
                local,
                shared,
                diagnostics,
            } => Operation::ListChats {
                input,
                local,
                shared,
                diagnostics,
            },
            Command::Index {
                connector_batch: Some(batch_file),
                ..
            } => Operation::IngestConnector { batch_file },
            Command::Index {
                rebuild_local_views,
                full,
                provider,
                since_days,
                all_history,
                ..
            } => Operation::Index(IndexRequest {
                mode: crate::core::IndexMode::from_rebuild_flag(rebuild_local_views),
                full,
                provider,
                since_days,
                all_history,
            }),
            Command::Search {
                shared_vector_search,
                shared_hnsw_ef,
                query,
                intent,
                limit,
                unit,
                fusion_rank_constant,
                lexical_weight,
                semantic_weight,
                recency_weight,
                ranking,
                diagnostics,
                filters,
                shared,
                local,
                ..
            } => Operation::Search {
                shared_vector_search: shared_vector_search
                    .map(|algorithm| algorithm.request(shared_hnsw_ef))
                    .transpose()?,
                ranking,
                query,
                intent,
                limit,
                unit,
                diagnostics,
                fusion: crate::views::config::FusionOverrides {
                    rank_constant: fusion_rank_constant,
                    lexical_weight,
                    semantic_weight,
                    recency_weight,
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
            Command::Status {
                shared,
                coverage,
                scope,
            } => Operation::Status {
                shared,
                coverage: coverage.then_some(*scope),
            },
            Command::Forget { id } => Operation::Forget { id },
            Command::Export { directory } => Operation::ExportSilver { directory },
            Command::Import {
                directory,
                into,
                keep_identity,
            } => Operation::ImportSilver {
                directory,
                database: into,
                keep_identity,
            },
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
                    silver_schema: crate::storage::schema::SCHEMA_VERSION,
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
        activate_views,
        activate_query,
        activate_memory,
        replay_views,
    } = cli.command
    {
        let models = cli.models_dir.unwrap_or_else(default_models_path);
        runtime.block_on(crate::shared::server::run(
            &server_config,
            crate::shared::server::Activation {
                views: activate_views,
                query: activate_query,
                memory: activate_memory,
            },
            &models,
            replay_views,
        ))?;
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
        if intent.unwrap_or(configuration.views.query.intent) == crate::views::exact::Intent::ExactMention)
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
    fn cli_preserves_typed_request_ranking_without_inventing_defaults() {
        for (flag, expected) in [
            (None, None),
            (Some("fusion"), Some(crate::views::config::Ranking::Fusion)),
            (
                Some("contextual-rerank"),
                Some(crate::views::config::Ranking::ContextualRerank),
            ),
        ] {
            let mut args = vec!["mash", "search", "database"];
            if let Some(flag) = flag {
                args.extend(["--ranking", flag]);
            }
            let Operation::Search { ranking, .. } = Cli::try_parse_from(args)
                .unwrap()
                .into_request()
                .unwrap()
                .operation
            else {
                panic!("search request")
            };
            assert_eq!(ranking, expected);
        }
        assert!(
            Cli::try_parse_from(["mash", "search", "database", "--ranking", "unknown"]).is_err()
        );
    }

    #[test]
    fn cli_lexical_matching_defaults_to_flexible_and_preserves_explicit_modes() {
        use crate::views::matching::Mode;
        for (name, expected) in [
            ("strict", Mode::Strict),
            ("prefix", Mode::Prefix),
            ("typo", Mode::Typo),
            ("flexible", Mode::Flexible),
        ] {
            let request = Cli::try_parse_from(["mash", "search", "idemp", "--match-mode", name])
                .unwrap()
                .into_request()
                .unwrap();
            let Operation::Search { filters, .. } = request.operation else {
                panic!("search")
            };
            assert_eq!(filters.match_mode, expected);
        }
        let request = Cli::try_parse_from(["mash", "search", "idemp"])
            .unwrap()
            .into_request()
            .unwrap();
        let Operation::Search { filters, .. } = request.operation else {
            panic!("search")
        };
        assert_eq!(filters.match_mode, Mode::Flexible);
        assert!(Cli::try_parse_from(["mash", "search", "idemp", "--match-mode", "fuzzy"]).is_err());
    }

    #[test]
    fn status_coverage_selectors_are_typed_and_explicit() {
        let request = Cli::try_parse_from([
            "mash",
            "status",
            "--coverage",
            "--provider",
            "codex",
            "--node",
            "peer",
            "--after",
            "10",
            "--before",
            "20",
            "--shared",
        ])
        .unwrap()
        .into_request()
        .unwrap();
        let Operation::Status { shared, coverage } = request.operation else {
            panic!("status")
        };
        assert!(shared);
        let scope = coverage.unwrap();
        assert_eq!(scope.provider, [Provider::Codex]);
        assert_eq!(scope.node, ["peer"]);
        assert_eq!((scope.after, scope.before), (Some(10), Some(20)));
        scope.validate().unwrap();
        assert!(Cli::try_parse_from(["mash", "status", "--provider", "codex"]).is_err());
    }

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
            "--diagnostics",
        ])
        .unwrap()
        .into_request()
        .unwrap();
        let Operation::Search {
            fusion,
            diagnostics,
            ..
        } = request.operation
        else {
            panic!("search request")
        };
        assert_eq!(fusion.rank_constant, Some(10));
        assert_eq!(fusion.lexical_weight, Some(4));
        assert_eq!(fusion.semantic_weight, None);
        assert!(diagnostics);
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
                "list-chats",
                "index",
                "search",
                "view",
                "status",
                "forget",
                "export",
                "import",
                "models",
                "mcp",
                "serve"
            ]
        );
    }
}
