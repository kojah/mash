//! Process arguments for the shared server; no local indexing or MCP commands.
use std::ffi::OsString;
use std::path::PathBuf;

use clap::Parser;
use serde::Serialize;

use crate::{AppError, shared::server};

#[derive(Parser)]
// Independent explicit startup operations, not mutually exclusive states.
#[allow(clippy::struct_excessive_bools)]
#[command(
    name = "mash-server",
    version,
    about = "Mash shared-index HTTP server",
    after_help = crate::cli::VERSION_INFO_HELP
)]
struct Arguments {
    /// Server configuration in TOML format.
    #[arg(long, value_name = "PATH", required_unless_present = "setup")]
    server_config: Option<PathBuf>,
    /// Set this machine up as the home of a shared index: write a first
    /// configuration under ~/.mash/server, keep the server running as a user
    /// service and give it an HTTPS address inside your tailnet. Needs
    /// --add-node for the first machine, a running Qdrant and Tailscale.
    #[arg(
        long,
        requires = "add_node",
        conflicts_with_all = ["server_config", "api_url", "retire_node", "replay_views", "activate_views", "activate_query", "activate_memory"]
    )]
    setup: bool,
    /// Port for the server on this machine and in its tailnet address.
    #[arg(long, default_value_t = 8787, requires = "setup")]
    port: u16,
    /// Semantic model asset directory; models are never downloaded here.
    #[arg(long, value_name = "PATH")]
    models_dir: Option<PathBuf>,
    /// Explicitly replace persisted shared group views settings from this TOML.
    #[arg(long)]
    activate_views: bool,
    /// Explicitly replace persisted shared group query defaults without indexing.
    #[arg(long)]
    activate_query: bool,
    /// Explicitly apply configured durable/cached/cold Qdrant placement.
    #[arg(long)]
    activate_memory: bool,
    /// Rebuild all shared views from shared silver before serving.
    #[arg(long)]
    replay_views: bool,
    /// Delete everything one node published, then exit without serving. Use it
    /// when a machine's local index was rebuilt and it can no longer publish
    /// under its name, or to remove a machine for good. Stop the service first.
    #[arg(
        long,
        value_name = "GROUP/NODE",
        conflicts_with_all = ["replay_views", "activate_views", "activate_query", "activate_memory"]
    )]
    retire_node: Option<String>,
    /// Add a machine to this server and print a join string for it, then
    /// exit without serving. Needs --api-url. Restart the service afterwards.
    #[arg(
        long,
        value_name = "GROUP/NODE",
        conflicts_with_all = ["retire_node", "replay_views", "activate_views", "activate_query", "activate_memory"]
    )]
    add_node: Option<String>,
    /// The address other machines use to reach this server, written into the
    /// join string: an HTTPS name or a private-network address.
    #[arg(long, value_name = "URL", requires = "add_node")]
    api_url: Option<String>,
}

#[derive(Serialize)]
#[serde(untagged)]
pub(super) enum Information {
    Help {
        help: String,
    },
    Version {
        version: &'static str,
        silver_schema: i64,
        silver_schema_min: i64,
    },
}

pub(super) fn run<I, T>(args: I) -> Result<Option<Information>, AppError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    if args
        .iter()
        .skip(1)
        .any(|argument| argument == crate::cli::VERSION_INFO)
    {
        return Ok(Some(Information::Version {
            version: env!("CARGO_PKG_VERSION"),
            silver_schema: crate::storage::schema::SCHEMA_VERSION,
            silver_schema_min: crate::storage::schema::MIN_SCHEMA_VERSION,
        }));
    }
    let args = match Arguments::try_parse_from(args) {
        Ok(args) => args,
        Err(error) => {
            return match error.kind() {
                clap::error::ErrorKind::DisplayHelp => Ok(Some(Information::Help {
                    help: error.to_string(),
                })),
                clap::error::ErrorKind::DisplayVersion => Ok(Some(Information::Help {
                    help: concat!("mash-server ", env!("CARGO_PKG_VERSION")).to_owned(),
                })),
                _ => Err(AppError::usage(error.to_string())),
            };
        }
    };
    crate::platform::release_installed_executable();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| AppError::internal("failed to start async runtime"))?;
    let models = args
        .models_dir
        .unwrap_or_else(crate::core::default_models_path);
    if args.setup {
        let node = args
            .add_node
            .as_deref()
            .ok_or_else(|| AppError::usage("--setup needs --add-node GROUP/NODE"))?;
        runtime.block_on(server::setup::run(node, args.port))?;
        return Ok(None);
    }
    let server_config = args
        .server_config
        .ok_or_else(|| AppError::usage("--server-config is required"))?;
    if let Some(node) = &args.retire_node {
        runtime.block_on(server::retire(&server_config, node))?;
        return Ok(None);
    }
    if let Some(node) = &args.add_node {
        let api_url = args.api_url.as_deref().ok_or_else(|| {
            AppError::usage(
                "--add-node needs --api-url, the address the new machine will use for this server",
            )
        })?;
        runtime.block_on(server::add_node(&server_config, node, api_url))?;
        return Ok(None);
    }
    runtime.block_on(server::run(
        &server_config,
        server::Activation {
            views: args.activate_views,
            query: args.activate_query,
            memory: args.activate_memory,
        },
        &models,
        args.replay_views,
    ))?;
    Ok(None)
}
