//! Typed application facade shared by CLI, MCP and the idle search worker.
use crate::AppError;
use crate::config::{self, ResolvedConfig};
use directories::ProjectDirs;
use futures_util::FutureExt;
use std::path::{Path, PathBuf};

mod chats;
mod connector_ingestion;
pub(crate) mod coverage;
mod indexing;
mod inspection;
mod maintenance;
mod models;
pub(crate) mod output;
mod search;

pub(crate) use indexing::watch_targets;
pub(crate) use models::{
    IndexMode, IndexRequest, Operation, Paths, Request, Response, SearchMode, SearchResponse,
    ViewResponse,
};

pub(crate) async fn execute(request: Request) -> Result<Response, AppError> {
    execute_admitted(request, None).await
}

pub(crate) async fn execute_with_model_limit(
    request: Request,
    capacity: &tokio::sync::Semaphore,
) -> Result<Response, AppError> {
    execute_admitted(request, Some(capacity)).await
}

async fn execute_admitted(
    request: Request,
    capacity: Option<&tokio::sync::Semaphore>,
) -> Result<Response, AppError> {
    use tracing::Instrument;
    let operation = match &request.operation {
        Operation::Index(_) => "index",
        Operation::IngestConnector { .. } => "ingest_connector",
        Operation::ListChats { .. } => "list_chats",
        Operation::Search { .. } => "search",
        Operation::View { .. } => "view",
        Operation::Status { .. } => "status",
        Operation::Forget { .. } => "forget",
        Operation::ModelsInstall => "models_install",
        Operation::ExportSilver { .. } => "export_silver",
        Operation::ImportSilver { .. } => "import_silver",
        Operation::ListAttachments { .. } => "list_attachments",
        Operation::GetAttachment { .. } => "get_attachment",
    };
    async {
        let started = std::time::Instant::now();
        let capture = matches!(&request.operation, Operation::Search { diagnostics: true, .. })
            .then(crate::observability::retrieval::Capture::default);
        let mut result = match &capture {
            Some(capture) => capture.scope(execute_inner(request, capacity)).await,
            None => execute_inner(request, capacity).await,
        };
        if let Ok(Response::Search(response)) = &mut result
            && let Some(diagnostics) = &mut response.diagnostics
        {
            diagnostics.elapsed_ms = crate::progress::milliseconds(started);
            if let Some(capture) = capture { diagnostics.record(capture.finish()); }
        }
        tracing::Span::current().record("elapsed_ms", crate::progress::milliseconds(started));
        match &result {
            Ok(response) if response.needs_retry() => tracing::info!(outcome = "incomplete", "request completed"),
            Ok(_) => tracing::info!(outcome = "success", "request completed"),
            Err(error) => tracing::info!(outcome = "error", error_kind = ?error.error.kind, "request completed"),
        }
        result
    }
        .instrument(tracing::info_span!(
            "request",
            request_id = crate::observability::request_id(),
            process_id = std::process::id(),
            operation,
            elapsed_ms = tracing::field::Empty
        ))
        .await
}

async fn execute_inner(
    request: Request,
    capacity: Option<&tokio::sync::Semaphore>,
) -> Result<Response, AppError> {
    let path = request.paths.config;
    let configuration = crate::blocking(move || config::load(path.as_deref())).await?;
    let database = request.paths.db.unwrap_or_else(default_database_path);
    let models = request.paths.models_dir.unwrap_or_else(default_models_path);
    search::preflight(&configuration, &request.operation).await?;
    let uses_models = match &request.operation {
        Operation::Search { intent, .. } => {
            intent.unwrap_or(configuration.views.query.intent)
                == crate::views::exact::Intent::RelatedDiscussion
        }
        Operation::Index(request) => {
            configuration.views.build.dense_enabled()
                || (request.mode != IndexMode::LocalViews && configuration.shared.is_some())
        }
        Operation::ModelsInstall => true,
        _ => false,
    };
    let _permit = if uses_models {
        match capacity {
            Some(capacity) => Some(
                capacity
                    .acquire()
                    .await
                    .map_err(|_| AppError::internal("model worker unavailable"))?,
            ),
            None => None,
        }
    } else {
        None
    };
    let mut response = dispatch(request.operation, &configuration, &database, &models)
        .boxed()
        .await?;
    if let Response::Search(search) = &mut response {
        search.query = configuration.redaction.output().text(&search.query);
    }
    Ok(response)
}

#[cfg(unix)]
pub(crate) fn validate_search_request(request: &Request) -> Result<(), AppError> {
    let Operation::Search {
        shared_vector_search,
        query,
        intent,
        fusion,
        ranking,
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
    search::validate_vector(
        *shared_vector_search,
        intent.unwrap_or_default(),
        *local,
        true,
    )?;
    search::resolve_fusion(
        *fusion,
        intent.unwrap_or_default(),
        crate::views::config::Fusion::default(),
    )?;
    search::resolve_ranking(
        *ranking,
        intent.unwrap_or_default(),
        crate::views::config::Ranking::default(),
    )?;
    search::resolve(
        query,
        intent.unwrap_or_default(),
        filters.as_ref().clone(),
        *shared,
        *local,
        "local",
    )
    .map(|_| ())
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
        Operation::IngestConnector { batch_file } => connector_ingestion::run(database, batch_file)
            .await
            .map(Response::IngestConnector),
        Operation::ExportSilver { directory } => {
            let database = database.to_path_buf();
            let source_node = configuration
                .shared
                .as_ref()
                .map(|shared| shared.node_id.clone());
            crate::blocking(move || crate::export::write(&database, &directory, source_node))
                .await
                .map(Response::ExportSilver)
        }
        Operation::ImportSilver {
            directory,
            database,
            keep_identity,
        } => crate::blocking(move || crate::export::load(&directory, &database, keep_identity))
            .await
            .map(Response::ImportSilver),
        Operation::ListAttachments { message_id, shared } => {
            inspection::list_attachments(configuration, database, message_id, shared)
                .await
                .map(Response::ListAttachments)
        }
        Operation::GetAttachment {
            message_id,
            attachment_id,
            shared,
        } => inspection::get_attachment(configuration, database, message_id, attachment_id, shared)
            .await
            .map(Response::GetAttachment),
        Operation::ListChats {
            input,
            local,
            shared,
            diagnostics,
        } => chats::run(configuration, database, *input, local, shared, diagnostics)
            .await
            .map(Response::ListChats),
        Operation::Index(request) => indexing::run(configuration, database, models, request)
            .await
            .map(Response::Index),
        Operation::Search {
            query,
            shared_vector_search,
            intent,
            limit,
            unit,
            fusion,
            ranking,
            diagnostics,
            filters,
            shared,
            local,
        } => search::run(
            configuration,
            database,
            models,
            search::Input {
                shared_vector_search,
                query,
                intent,
                limit,
                unit,
                fusion,
                ranking,
                diagnostics,
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
        Operation::Status { shared, coverage } => {
            inspection::status(configuration, database, models, shared, coverage)
                .await
                .map(|status| Response::Status(Box::new(status)))
        }
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

/// Both mode publishes exactly the local index.
fn publication_config(configuration: &ResolvedConfig) -> Option<&config::SharedConfig> {
    (configuration.destination == config::IndexDestination::Both)
        .then_some(configuration.shared.as_ref())
        .flatten()
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
