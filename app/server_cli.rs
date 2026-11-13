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
    #[arg(long, value_name = "PATH")]
    server_config: PathBuf,
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
    if let Some(node) = &args.retire_node {
        runtime.block_on(server::retire(&args.server_config, node))?;
        return Ok(None);
    }
    runtime.block_on(server::run(
        &args.server_config,
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
