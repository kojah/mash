//! Typed application facade shared by CLI, MCP and the idle search worker.
use crate::AppError;
use crate::config::{self, ResolvedConfig};
use directories::ProjectDirs;
use std::path::{Path, PathBuf};

mod indexing;
mod inspection;
mod maintenance;
mod models;
mod search;

pub(crate) use indexing::watch_targets;
pub(crate) use models::{
    IndexRequest, Operation, Paths, Request, Response, SearchMode, SearchResponse, ViewResponse,
};

pub(crate) async fn execute(request: Request) -> Result<Response, AppError> {
    let path = request.paths.config;
    let configuration = crate::blocking(move || config::load(path.as_deref())).await?;
    let database = request.paths.db.unwrap_or_else(default_database_path);
    let models = request.paths.models_dir.unwrap_or_else(default_models_path);
    dispatch(request.operation, &configuration, &database, &models).await
}

#[cfg(unix)]
pub(crate) fn validate_search_request(request: &Request) -> Result<(), AppError> {
    let Operation::Search {
        query,
        filters,
        shared,
        local,
        ..
    } = &request.operation
    else {
        return Err(AppError::usage("search worker accepts only searches"));
    };
    // The node affects the resolved scope, but not syntax validation. Actual
    // execution resolves again with the current configuration and time cutoff.
    search::resolve(query, filters.as_ref().clone(), *shared, *local, "local").map(|_| ())
}

#[cfg(unix)]
pub(crate) async fn worker_search(request: Request) -> Result<SearchResponse, AppError> {
    validate_search_request(&request)?;
    match execute(request).await? {
        Response::Search(response) => Ok(response),
        _ => Err(AppError::internal("unexpected search worker response")),
    }
}

async fn dispatch(
    command: Operation,
    configuration: &ResolvedConfig,
    database: &Path,
    models: &Path,
) -> Result<Response, AppError> {
    match command {
        Operation::Index(request) => indexing::run(configuration, database, models, request)
            .await
            .map(Response::Index),
        Operation::Search {
            query,
            limit,
            unit,
            filters,
            shared,
            local,
        } => search::run(
            configuration,
            database,
            models,
            search::Input {
                query,
                limit,
                unit,
                filters: *filters,
                shared,
                local,
            },
        )
        .await
        .map(Response::Search),
        Operation::View {
            id,
            context,
            shared,
        } => inspection::view(configuration, database, id, context, shared)
            .await
            .map(Response::View),
        Operation::Status { shared } => inspection::status(configuration, database, models, shared)
            .await
            .map(Response::Status),
        Operation::Forget { id } => maintenance::forget(database, configuration, id)
            .await
            .map(Response::Forget),
        Operation::ModelsInstall => {
            maintenance::install_models(models, configuration.embeddings.clone())
                .await
                .map(Response::ModelsInstall)
        }
    }
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
