use crate::provider::Provider;
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

use clap::CommandFactory;
use clap::{Parser, Subcommand};
use serde::Serialize;

use crate::AppError;
use crate::core::{self, IndexRequest, Operation, Paths, Request};

mod wizard;

/// The flag that prints what `--version` leaves out; tools read it.
pub(crate) const VERSION_INFO: &str = "--version-info";
pub(crate) const VERSION_INFO_HELP: &str =
    "  --version-info  Print the release and database schema versions as JSON";

#[derive(Debug, Clone, Parser)]
#[command(
    name = "mash",
    version,
    about,
    arg_required_else_help = true,
    after_help = VERSION_INFO_HELP
)]
pub(crate) struct Cli {
    /// Versioned mash node and provider configuration.
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Canonical mash SQLite database.
    #[arg(long, global = true, value_name = "PATH")]
    db: Option<PathBuf>,

    /// Semantic model asset directory [default: ~/.mash/models].
    #[arg(long, global = true, value_name = "PATH")]
    models_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, Subcommand)]
enum Command {
    /// Set up Mash with a terminal wizard, or accept defaults with --yes.
    /// Configure histories, connect agent integrations, and index.
    ///
    /// Indexing and every search need a running local Qdrant service (by
    /// default on this machine's port 6333); init checks it before changing
    /// anything. Use --no-index to configure without it.
    ///
    /// Agents are connected through their own CLIs, which edit their user
    /// settings: ~/.claude.json, ~/.claude/settings.json and ~/.claude/plugins
    /// for Claude Code, ~/.codex/config.toml and ~/.codex/plugins for Codex.
    /// The connected agents are recorded in Mash's config.toml, so
    /// `mash status --installation` reports them if they lose Mash. To undo,
    /// run `claude mcp remove --scope user mash`, `claude plugin uninstall
    /// mash@mash`, `codex mcp remove mash` and `codex plugin remove mash@mash`,
    /// then delete Mash's configuration and data directories.
    Init {
        /// Accept defaults without prompting (also used automatically without a terminal).
        #[arg(short = 'y', long)]
        yes: bool,
        /// Replace an existing configuration (kept as config.toml.previous).
        #[arg(long)]
        force: bool,
        /// Leave agent settings unchanged.
        #[arg(long)]
        no_agents: bool,
        /// Skip the first index, which needs Qdrant running.
        #[arg(long)]
        no_index: bool,
        /// Explicitly backfill memory with this signed-in agent (last 3 days by default).
        #[arg(long, value_enum)]
        backfill_memory: Option<crate::core::installation::Agent>,
        /// Plugin marketplace to add: a GitHub owner/repo, URL or local path.
        #[arg(long, default_value = crate::core::MARKETPLACE)]
        marketplace: String,
        /// Connect this machine to a shared index with the join string that
        /// `mash-server --add-node` printed. It holds this machine's token.
        #[arg(long, value_name = "JOIN_STRING")]
        join: Option<String>,
    },
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
        /// Read every admitted source again, even where nothing has changed.
        /// Use it after upgrading to a release that reads an agent's history
        /// differently; unchanged chats keep their search data.
        #[arg(long, conflicts_with_all = ["watch", "rebuild_local_views"])]
        reread: bool,
        /// Publish this database as a progressive baseline that merges instead
        /// of replacing: a chat missing from the payload is not-yet-sent, never
        /// deleted. Plain `index` or `forget` removes chats from the server.
        #[arg(
            long,
            hide = true,
            value_name = "BACKFILL_ID",
            num_args = 0..=1,
            default_missing_value = "baseline",
            conflicts_with_all = ["watch", "rebuild_local_views"]
        )]
        backfill: Option<String>,
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
        /// Retrieval intent: exact-mention (every word in any order, or a "quoted"
        /// phrase; no models) or related-discussion (by meaning, needs models);
        /// defaults to views.query.intent, which is exact-mention unless configured.
        #[arg(long, value_enum)]
        intent: Option<crate::views::exact::Intent>,
        /// Results per page (1–200); defaults to views.query.limit (20).
        #[arg(long)]
        limit: Option<usize>,
        /// Page offset into the ranked results (below 200 chats or 50 messages);
        /// continue with the returned `next_offset`.
        #[arg(long, default_value_t = 0)]
        offset: usize,
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
        /// Query only this machine's index (local SQLite and local Qdrant),
        /// without contacting the shared service.
        #[arg(long, conflicts_with = "shared")]
        local: bool,
    },
    /// Return a message, by message ID, and adjacent context.
    View {
        /// Message ID: copy lookup.view.id from a list-chats row or search result.
        /// A chat ID is not accepted.
        #[arg(value_name = "MESSAGE_ID")]
        id: String,
        /// Also return up to N messages before and after the message.
        #[arg(long, value_name = "N", default_value_t = 0)]
        context: u32,
        /// Read shared context for this publishing node.
        #[arg(long, value_name = "NODE_ID")]
        shared: Option<String>,
    },
    /// Report readiness, including whether local Qdrant answers; --installation
    /// discovers providers and agent setup.
    Status {
        /// Inspect agent integrations and whether the configured Qdrant answers, without
        /// opening the Mash index or models.
        #[arg(long, conflicts_with_all = ["coverage", "shared"])]
        installation: bool,
        /// Inspect only this agent and require its integration (repeatable).
        /// Without it, every agent is inspected; the command exits 1 when an
        /// expected agent (named here, connected by `mash init`, or configured
        /// for Mash) needs attention.
        #[arg(long, value_enum, requires = "installation")]
        agent: Vec<crate::core::installation::Agent>,
        /// Launch configured stdio MCP commands and verify Mash help (10 seconds per client).
        #[arg(long, requires = "installation")]
        probe: bool,
        /// Report scoped retained-history and provider scan facts without proving completeness.
        #[arg(long)]
        coverage: bool,
        #[command(flatten)]
        scope: Box<crate::core::coverage::Scope>,
        /// Inspect per-node freshness in the shared index.
        #[arg(long)]
        shared: bool,
    },
    /// Remove one chat and its derived search rows.
    Forget { id: String },
    /// Run one read-only query over local silver.
    ///
    /// Use --request for the typed contract (target, filter, `group_by`, order,
    /// limit) or --sql for a read-only SQL escape hatch. Stable views are
    /// chats, messages and documents.
    Query {
        /// SQL to run; the connection is read-only and bounded.
        #[arg(long, required_unless_present = "request", conflicts_with = "request")]
        sql: Option<String>,
        /// Typed request as JSON, e.g.
        /// '{"target":"messages","filter":{"provider":"codex"},"limit":20}'.
        #[arg(long, conflicts_with = "sql")]
        request: Option<String>,
        /// Maximum rows returned in --sql mode; --request carries its own.
        #[arg(long)]
        limit: Option<usize>,
        /// Query the configured shared index instead of local silver. Requires
        /// --request; SQL is never sent to the server.
        #[arg(long, requires = "request", conflicts_with = "sql")]
        shared: bool,
        /// Shared node to read; defaults to this node and must belong to the
        /// token's group. Requires --shared.
        #[arg(long, requires = "shared")]
        node: Option<String>,
        /// Wall-clock budget in seconds for local queries.
        #[arg(long, default_value_t = 10)]
        timeout_seconds: u64,
    },
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
    /// Install or remove the local Qdrant search service.
    Qdrant {
        #[command(subcommand)]
        command: QdrantCommand,
    },
    /// Serve search tools over the stdio MCP protocol.
    Mcp,
    /// Serve the dashboard (history is read-only; service management is opt-in).
    #[command(args_conflicts_with_subcommands = true)]
    Dash {
        #[command(subcommand)]
        command: Option<DashCommand>,
        /// Address to listen on; loopback by default.
        #[arg(long, value_name = "ADDRESS", default_value = "127.0.0.1:8790")]
        listen: std::net::SocketAddr,
        /// Another name browsers use for this dashboard, such as the HTTPS name a
        /// reverse proxy or `tailscale serve` gives it; repeatable. Requires
        /// --access-token-env.
        #[arg(long, value_name = "HOST")]
        allow_host: Vec<String>,
        /// Environment variable holding a token of at least 32 characters that
        /// every visitor signs in with.
        #[arg(long, value_name = "NAME")]
        access_token_env: Option<String>,
        /// Show only the shared index, for a machine with no agent history of
        /// its own. Needs a [shared] table in the configuration.
        #[arg(long)]
        shared_only: bool,
        /// Explicit service inventory and authentication (TOML). Enables fleet management.
        #[arg(long, value_name = "PATH")]
        services_config: Option<PathBuf>,
        /// Shared machine/service inventory used by mash-fleet (TOML).
        #[arg(long, value_name = "PATH", conflicts_with = "services_config")]
        fleet_config: Option<PathBuf>,
        /// This dashboard's machine ID; defaults to the inventory controller.
        #[arg(long, requires = "fleet_config")]
        fleet_node: Option<String>,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum DashCommand {
    /// Keep the dashboard running and open it from your other devices over
    /// Tailscale, behind a sign-in. Prints the address, a one-time code and a
    /// QR code to scan.
    Remote {
        /// Port for the dashboard on this machine and in its address.
        #[arg(long, default_value_t = 8790)]
        port: u16,
        /// Show only the shared index, for a machine with no agent history of
        /// its own, such as the server that holds the shared index.
        #[arg(long)]
        shared_only: bool,
        /// Make the address reachable from the public internet with Tailscale
        /// Funnel, for devices that are not on your tailnet. The sign-in is
        /// then all that protects your history. Needs --port 443, 8443 or 10000.
        #[arg(long)]
        public: bool,
        /// Print a fresh one-time code for signing another device in.
        #[arg(long, conflicts_with_all = ["qr", "remove"])]
        code: bool,
        /// Show the sign-in QR code again.
        #[arg(long, conflicts_with = "remove")]
        qr: bool,
        /// Stop serving and remove the service; the token is kept.
        #[arg(long)]
        remove: bool,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum ModelsCommand {
    /// Explicitly install embedding and reranking models.
    #[command(long_about = concat!(
        "Explicitly install embedding and reranking models.\n\n",
        "Downloads about 177 MB (more on Apple silicon, which adds a Core ML embedding model) ",
        "from https://huggingface.co, or from the Hugging Face mirror named by HF_ENDPOINT, ",
        "into the models directory, ~/.mash/models unless --models-dir is set. ",
        "Word-exact search does not need these models.",
    ))]
    Install,
}

#[derive(Debug, Clone, Subcommand)]
enum QdrantCommand {
    /// Download, verify and start the local Qdrant search service.
    #[command(long_about = concat!(
        "Download, verify and start the local Qdrant search service.\n\n",
        "Downloads Qdrant 1.20.0 for this platform (about 30 MB) from ",
        "https://github.com/qdrant/qdrant/releases, checks it against the SHA-256 built into Mash, ",
        "and installs it in ~/.mash/qdrant. ",
        "It runs as your user, only on 127.0.0.1 at the ports in Mash's [qdrant] settings ",
        "(6333 and 6334 by default), and starts again at login: a systemd user service on Linux, ",
        "a LaunchAgent on macOS, a logon task on Windows. ",
        "Rerun it to repair or upgrade; Qdrant's data is kept. ",
        "If a compatible Qdrant already answers on those ports, nothing is installed. ",
        "Nothing else in Mash downloads or starts Qdrant.",
    ))]
    Install,
    /// Stop and remove the Qdrant service that `mash qdrant install` set up.
    Uninstall {
        /// Also delete Qdrant's data (Mash's search views; `mash index` rebuilds them from the index).
        #[arg(long)]
        delete_storage: bool,
    },
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
    chat: Vec<String>,
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
            match_mode: self.match_mode,
            sort: self.sort,
            provider: self.provider,
            days: self.days,
            node: self.node,
            role: self.role,
            chat: self.chat,
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
    Help {
        help: String,
    },
    /// The release and the silver schema it opens; deployment compares the
    /// latter with an existing database's `user_version` before replacing it.
    Version {
        version: &'static str,
        silver_schema: i64,
        silver_schema_min: i64,
        /// How `record` evidence may name its source. The plugin hooks read
        /// this before telling an agent to cite its own session.
        record_citations: [&'static str; 2],
    },
    WatchStopped {
        watching: bool,
    },
    /// The watcher stopped because its executable was replaced; the caller
    /// starts the replacement in this process once the runtime is gone.
    #[serde(skip)]
    WatchReplaced {
        reason: AppError,
        executable: PathBuf,
    },
    McpStopped,
    InitCancelled {
        cancelled: bool,
    },
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
            Command::Init {
                force,
                no_agents,
                no_index,
                backfill_memory,
                marketplace,
                join,
                yes: _,
            } => Operation::Init(crate::core::InitRequest {
                force,
                agents: !no_agents,
                index: !no_index,
                marketplace,
                plan: join
                    .map(|text| {
                        let join = crate::shared::join::Join::decode(&text)?;
                        crate::core::join_plan(&paths, &join)
                    })
                    .transpose()?,
                selected_agents: None,
                backfill_memory,
            }),
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
                rebuild_local_views,
                full,
                provider,
                since_days,
                all_history,
                reread,
                backfill,
                ..
            } => Operation::Index(IndexRequest {
                mode: crate::core::IndexMode::from_rebuild_flag(rebuild_local_views),
                full,
                provider,
                since_days,
                all_history,
                reading: if reread {
                    crate::core::Reading::Everything
                } else {
                    crate::core::Reading::Changed
                },
                defer_publication: false,
                backfill,
            }),
            Command::Search {
                shared_vector_search,
                shared_hnsw_ef,
                query,
                intent,
                limit,
                offset,
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
                offset,
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
                installation,
                agent,
                probe,
                shared,
                coverage,
                scope,
            } => {
                if installation {
                    Operation::Installation {
                        agents: agent,
                        probe: if probe {
                            Some(crate::mcp::guidance::fingerprint().map_err(|_| {
                                AppError::internal("cannot encode this CLI's MCP contract")
                            })?)
                        } else {
                            None
                        },
                    }
                } else {
                    Operation::Status {
                        shared,
                        coverage: coverage.then_some(*scope),
                    }
                }
            }
            Command::Forget { id } => Operation::Forget { id },
            Command::Query {
                sql,
                request,
                limit,
                shared,
                node,
                timeout_seconds,
            } => {
                let parsed = |request: String| {
                    serde_json::from_str(&request)
                        .map_err(|error| AppError::usage(format!("invalid query request: {error}")))
                };
                if shared {
                    let Some(request) = request else {
                        return Err(AppError::usage("--shared requires --request"));
                    };
                    Operation::QueryShared {
                        request: parsed(request)?,
                        node,
                    }
                } else if let Some(request) = request {
                    Operation::QueryStructured {
                        request: parsed(request)?,
                        timeout_seconds,
                    }
                } else {
                    Operation::QuerySilver {
                        sql: sql.unwrap_or_default(),
                        limit: limit.unwrap_or(200),
                        timeout_seconds,
                    }
                }
            }
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
            Command::Qdrant {
                command: QdrantCommand::Install,
            } => Operation::QdrantInstall,
            Command::Qdrant {
                command: QdrantCommand::Uninstall { delete_storage },
            } => Operation::QdrantUninstall { delete_storage },
            Command::Mcp | Command::Dash { .. } => {
                return Err(AppError::usage(
                    "server commands must run as process commands",
                ));
            }
        };
        Ok(Request { paths, operation })
    }
}

/// Serve the dashboard, or set up its remote access.
fn dash(runtime: &tokio::runtime::Runtime, paths: Paths, command: Command) -> Result<(), AppError> {
    match command {
        Command::Dash {
            command:
                Some(DashCommand::Remote {
                    port,
                    shared_only,
                    public,
                    code,
                    qr,
                    remove,
                }),
            ..
        } => runtime.block_on(crate::remote::run(
            paths,
            crate::remote::Options {
                port,
                shared_only,
                public,
            },
            remote_action(code, qr, remove),
        )),
        Command::Dash {
            command: None,
            listen,
            allow_host,
            access_token_env,
            shared_only,
            services_config,
            fleet_config,
            fleet_node,
        } => runtime.block_on(crate::dash::run(
            paths,
            crate::dash::Reach {
                listen,
                hosts: allow_host,
                access_token_env,
                shared_only,
            },
            services_config,
            fleet_config,
            fleet_node,
        )),
        _ => Err(AppError::internal("not a dashboard command")),
    }
}

fn remote_action(code: bool, qr: bool, remove: bool) -> crate::remote::Action {
    use crate::remote::Action as RemoteAction;
    if remove {
        RemoteAction::Remove
    } else if code {
        RemoteAction::Code
    } else if qr {
        RemoteAction::Qr
    } else {
        RemoteAction::Setup
    }
}

/// What `--version-info` prints, wherever the flag appears: the release and
/// the silver schemas it opens.
fn version_info(arguments: &[OsString]) -> Option<Response> {
    arguments
        .iter()
        .skip(1)
        .any(|argument| argument == VERSION_INFO)
        .then_some(Response::Version {
            version: env!("CARGO_PKG_VERSION"),
            silver_schema: crate::storage::schema::SCHEMA_VERSION,
            silver_schema_min: crate::storage::schema::MIN_SCHEMA_VERSION,
            record_citations: ["message_id", "session_id"],
        })
}

pub(super) fn run<I, T>(args: I) -> Result<Response, AppError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let arguments: Vec<OsString> = args.into_iter().map(Into::into).collect();
    if let Some(response) = version_info(&arguments) {
        return Ok(response);
    }
    let cli = match Cli::try_parse_from(&arguments) {
        Ok(cli) => cli,
        Err(error) => match error.kind() {
            clap::error::ErrorKind::DisplayHelp => {
                let mut help = error.to_string();
                if let Some(notice) =
                    crate::core::installation::help_notice(&help_paths(&arguments))
                {
                    help.push('\n');
                    help.push_str(notice);
                    help.push('\n');
                }
                return Ok(Response::Help { help });
            }
            // The short version is for a person, like help.
            clap::error::ErrorKind::DisplayVersion => {
                return Ok(Response::Help {
                    help: concat!("mash ", env!("CARGO_PKG_VERSION")).to_owned(),
                });
            }
            _ => return Err(AppError::usage(error.to_string())),
        },
    };
    let long_lived = matches!(
        cli.command,
        Command::Mcp | Command::Dash { .. } | Command::Index { watch: true, .. }
    );
    if long_lived {
        crate::platform::release_installed_executable();
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| AppError::internal("failed to start async runtime"))?;
    // Warm an opted-in remote reranker when this process makes a search
    // likely. Detached for long-running processes; a catch-up index waits
    // for it (bounded) so its exit does not abandon a cold start.
    let warm_up = {
        use crate::semantic::remote_reranker::{Occasion, warm_up_task};
        let occasion = match &cli.command {
            Command::Mcp | Command::Dash { command: None, .. } => Some(Occasion::LongRunning),
            Command::Index { watch: false, .. } => Some(Occasion::CatchUp),
            _ => None,
        };
        occasion
            .and_then(|occasion| warm_up_task(occasion, cli.config.clone()))
            .map(|task| runtime.spawn(task))
    };
    if matches!(cli.command, Command::Dash { .. }) {
        let paths = Paths {
            config: cli.config,
            db: cli.db,
            models_dir: cli.models_dir,
        };
        dash(&runtime, paths, cli.command)?;
        Ok(Response::McpStopped)
    } else if let Command::Mcp = cli.command {
        let paths = Paths {
            config: cli.config,
            db: cli.db,
            models_dir: cli.models_dir,
        };
        runtime.block_on(crate::mcp::run(paths))?;
        Ok(Response::McpStopped)
    } else {
        runtime.block_on(async {
            let response = run_parsed(cli).await;
            if let Some(warm_up) = warm_up {
                let _ = warm_up.await;
            }
            response
        })
    }
}

// Parse global paths with Clap even when help short-circuits the normal parser.
// Values containing `--help` remain values; no token stripping or hand parsing.
fn help_paths(arguments: &[OsString]) -> Paths {
    fn passive(command: clap::Command) -> clap::Command {
        command
            .disable_help_flag(true)
            .disable_help_subcommand(true)
            .arg_required_else_help(false)
            .subcommand_required(false)
            .mut_subcommands(passive)
    }
    let command = passive(Cli::command())
        .arg(
            clap::Arg::new("diagnostic-help")
                .long("help")
                .short('h')
                .global(true)
                .action(clap::ArgAction::SetTrue),
        )
        .subcommand(clap::Command::new("help").arg(clap::Arg::new("topic").num_args(0..)))
        .ignore_errors(true);
    command.try_get_matches_from(arguments).map_or_else(
        |_| Paths::default(),
        |matches| Paths {
            config: matches.get_one::<PathBuf>("config").cloned(),
            db: matches.get_one::<PathBuf>("db").cloned(),
            models_dir: matches.get_one::<PathBuf>("models_dir").cloned(),
        },
    )
}

#[derive(serde::Serialize)]
struct WatchRole {
    event: WatchRoleEvent,
    role: crate::watch::Role,
}

#[derive(serde::Serialize)]
enum WatchRoleEvent {
    #[serde(rename = "watch-role")]
    Role,
}

async fn run_parsed(mut cli: Cli) -> Result<Response, AppError> {
    if let Command::Init { yes, join, .. } = &cli.command {
        use std::io::IsTerminal;
        // A join string already says what to set up; nothing is left to ask.
        if wizard::interactive(
            *yes || join.is_some(),
            std::io::stdin().is_terminal(),
            std::io::stderr().is_terminal(),
        ) {
            return wizard::run(cli).await;
        }
    }
    if let Command::Index {
        watch, debounce_ms, ..
    } = &mut cli.command
        && *watch
    {
        *watch = false;
        let debounce = *debounce_ms;
        let request = cli.into_request()?;
        let paths = request.paths.clone();
        let lease = std::sync::Arc::new(
            crate::blocking(move || {
                crate::runtime::Lease::register(&paths, crate::runtime::Kind::IndexWatch)
            })
            .await?,
        );
        let (stop, shutdown) = tokio::sync::watch::channel(false);
        let monitor = lease.clone();
        let signal = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
            loop {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => break,
                    _ = interval.tick() => {
                        let lease = monitor.clone();
                        if crate::blocking(move || lease.ensure_current()).await.is_err() {
                            break;
                        }
                    }
                }
            }
            let _ = stop.send(true);
        });
        let role = |role| {
            let _ = crate::write_json(
                &mut std::io::stderr(),
                &WatchRole {
                    event: WatchRoleEvent::Role,
                    role,
                },
            );
        };
        let admission = lease.clone();
        // Watch collects state continuously; publish out of band so a slow
        // server never blocks the next indexing pass.
        let mut watch_request = request.clone();
        if let crate::core::Request {
            operation: crate::core::Operation::Index(index),
            ..
        } = &mut watch_request
        {
            index.defer_publication = true;
        }
        let result =
            crate::watch::run(watch_request.clone(), debounce, shutdown, role, move || {
                let request = watch_request.clone();
                let lease = admission.clone();
                async move {
                    crate::blocking(move || lease.ensure_current()).await?;
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
        // The monitor holds the lease; it must be gone before a restart.
        let _ = signal.await;
        result?;
        let executable = lease.executable().to_path_buf();
        if let Err(reason) = crate::blocking(move || lease.ensure_current()).await {
            return Ok(Response::WatchReplaced { reason, executable });
        }
        return Ok(Response::WatchStopped { watching: false });
    }
    core::execute(cli.into_request()?)
        .await
        .map(Box::new)
        .map(Response::Operation)
}

#[cfg(test)]
mod skill_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// The setup page agents follow (getmash.io/setup.md) names only commands
    /// this binary accepts.
    #[test]
    fn agent_setup_page_commands_parse() {
        let page = include_str!("../site/public/setup.md");
        let commands: Vec<&str> = page
            .lines()
            .filter(|line| line.starts_with("mash "))
            .collect();
        assert!(commands.len() >= 6, "{commands:?}");
        for command in commands {
            if let Err(error) = Cli::try_parse_from(command.split_whitespace()) {
                assert_eq!(
                    error.kind(),
                    clap::error::ErrorKind::DisplayVersion,
                    "{command}: {error}"
                );
            }
        }
    }

    #[test]
    fn mcp_rejects_retired_watch_options() {
        assert!(Cli::try_parse_from(["mash", "mcp", "--watch"]).is_err());
        assert!(Cli::try_parse_from(["mash", "mcp", "--debounce-ms", "500"]).is_err());
        assert!(Cli::try_parse_from(["mash", "index", "--watch", "--debounce-ms", "500"]).is_ok());
        assert!(Cli::try_parse_from(["mash", "index", "--reread"]).is_ok());
        assert!(Cli::try_parse_from(["mash", "index", "--reread", "--watch"]).is_err());
    }

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
    fn backfill_is_a_flag_on_index_not_a_sibling_command() {
        let Cli { command, .. } = Cli::try_parse_from(["mash", "index", "--backfill"]).unwrap();
        let Command::Index { backfill, .. } = command else {
            panic!("expected index");
        };
        assert_eq!(backfill.as_deref(), Some("baseline"));

        let Cli { command, .. } =
            Cli::try_parse_from(["mash", "index", "--backfill", "nightly"]).unwrap();
        let Command::Index { backfill, .. } = command else {
            panic!("expected index");
        };
        assert_eq!(backfill.as_deref(), Some("nightly"));

        // The baseline is a one-shot publication, not a watch or rebuild mode.
        assert!(Cli::try_parse_from(["mash", "index", "--backfill", "--watch"]).is_err());
        assert!(
            Cli::try_parse_from(["mash", "index", "--backfill", "--rebuild-local-views"]).is_err()
        );
        // The retired sibling command is gone.
        assert!(Cli::try_parse_from(["mash", "backfill"]).is_err());
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
                "init",
                "list-chats",
                "index",
                "search",
                "view",
                "status",
                "forget",
                "query",
                "export",
                "import",
                "models",
                "qdrant",
                "mcp",
                "dash"
            ]
        );
    }
}
