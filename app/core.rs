//! Typed application facade shared by CLI and MCP.
use crate::AppError;
use crate::config::{self, ResolvedConfig};
use directories::ProjectDirs;
use futures_util::FutureExt;
use std::path::{Path, PathBuf};

mod chats;
pub(crate) mod coverage;
mod indexing;
mod inspection;
pub(crate) mod installation;
mod maintenance;
mod models;
pub(crate) mod output;
mod search;
mod setup;

pub(crate) use indexing::watch_targets;
pub(crate) use models::{
    BackfillSummary, IndexMode, IndexRequest, Operation, Paths, Request, Response, SearchMode,
    SearchResponse, ViewResponse,
};
pub(crate) use setup::{InitRequest, MARKETPLACE};

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
        Operation::Init(_) => "init",
        Operation::Index(_) => "index",
        Operation::ListChats { .. } => "list_chats",
        Operation::Search { .. } => "search",
        Operation::View { .. } => "view",
        Operation::Status { .. } | Operation::Installation { .. } => "status",
        Operation::Forget { .. } => "forget",
        Operation::QuerySilver { .. }
        | Operation::QueryStructured { .. }
        | Operation::QueryShared { .. } => "query",
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
        let deadline = matches!(&request.operation, Operation::Search { .. }).then_some(SEARCH_DEADLINE);
        let run = within(deadline, execute_inner(request, capacity).boxed());
        let mut result = match &capture {
            Some(capture) => capture.scope(run).await,
            None => run.await,
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

/// Every search, including any wait for model capacity, ends by this deadline
/// rather than hanging. Blocking work already started may finish unobserved.
const SEARCH_DEADLINE: std::time::Duration = std::time::Duration::from_secs(15);

async fn within<T>(
    deadline: Option<std::time::Duration>,
    work: impl std::future::Future<Output = Result<T, AppError>>,
) -> Result<T, AppError> {
    match deadline {
        Some(limit) => tokio::time::timeout(limit, work)
            .await
            .unwrap_or_else(|_| Err(AppError::search_timeout(limit))),
        None => work.await,
    }
}

async fn execute_inner(
    request: Request,
    capacity: Option<&tokio::sync::Semaphore>,
) -> Result<Response, AppError> {
    let mut request = request;
    let mut initialized = None;
    if let Operation::Init(init) = &request.operation {
        let index = init.index;
        let response = setup::init(request.paths.clone(), init.clone()).await?;
        if !index {
            return Ok(Response::Init(response));
        }
        // Continue as the first index, without re-entering this function.
        initialized = Some(response);
        request.operation = setup::first_index();
    }
    if let Operation::Installation { agents, probe } = &request.operation {
        return installation::inspect(agents.clone(), *probe, request.paths.clone())
            .boxed()
            .await
            .map(Response::Installation);
    }
    let path = request.paths.config;
    let configuration = crate::blocking(move || config::load(path.as_deref())).await?;
    let database = request.paths.db.unwrap_or_else(default_database_path);
    let models = request.paths.models_dir.unwrap_or_else(default_models_path);
    if !configuration.loaded
        && matches!(request.operation, Operation::Search { .. })
        && !database.exists()
    {
        return Err(AppError::not_set_up(&database));
    }
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
    let writer_database = match &request.operation {
        Operation::Index(_) | Operation::Forget { .. } => Some(database.clone()),
        Operation::ImportSilver { database, .. } => Some(database.clone()),
        _ => None,
    };
    let owner = match writer_database {
        Some(path) => {
            Some(crate::blocking(move || crate::storage::admission::acquire(&path)).await?)
        }
        None => None,
    };
    let mut response = crate::storage::admission::scope(
        owner,
        dispatch(request.operation, &configuration, &database, &models).boxed(),
    )
    .await?;
    if let Response::Search(search) = &mut response {
        search.notice = search::empty_notice(search, &configuration);
        search.query = configuration.redaction.output().text(&search.query);
    }
    if let Some(mut initialized) = initialized {
        initialized.index = Some(Box::new(response));
        return Ok(Response::Init(initialized));
    }
    Ok(response)
}

#[cfg(test)]
pub(crate) fn validate_search_request(request: &Request) -> Result<(), AppError> {
    let Operation::Search {
        shared_vector_search,
        query,
        intent,
        limit,
        offset,
        unit,
        fusion,
        ranking,
        filters,
        shared,
        local,
        ..
    } = &request.operation
    else {
        return Err(AppError::usage("expected a search request"));
    };
    // The node affects the resolved scope, but not syntax validation. Actual
    // execution resolves again with the current configuration and time cutoff.
    crate::views::config::Query::default().resolve(*limit, *offset, *unit)?;
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

pub(crate) fn validate_chat_request(
    input: &crate::chat_catalog::Request,
    local: bool,
    shared: bool,
) -> Result<(), AppError> {
    input.validate()?;
    if local && shared {
        return Err(AppError::usage(
            "local and shared thread discovery are mutually exclusive",
        ));
    }
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "Keep the mechanical operation-to-core dispatch together."
)]
async fn dispatch(
    command: Operation,
    configuration: &ResolvedConfig,
    database: &Path,
    models: &Path,
) -> Result<Response, AppError> {
    match command {
        Operation::QuerySilver {
            sql,
            limit,
            timeout_seconds,
        } => {
            let database = database.to_path_buf();
            crate::blocking(move || {
                crate::silver_query::run(
                    &database,
                    &sql,
                    limit,
                    std::time::Duration::from_secs(timeout_seconds),
                )
            })
            .await
            .map(Response::Query)
        }
        Operation::QueryShared { request, node } => {
            let config = required_shared(configuration)?;
            crate::shared::query(config, node.as_deref(), &request)
                .await
                .map(Response::Query)
        }
        Operation::QueryStructured {
            request,
            timeout_seconds,
        } => {
            let database = database.to_path_buf();
            crate::blocking(move || {
                crate::silver_query::run_request(
                    &database,
                    &request,
                    std::time::Duration::from_secs(timeout_seconds),
                )
            })
            .await
            .map(Response::Query)
        }
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
            offset,
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
                offset,
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
        Operation::Installation { agents, probe } => installation::inspect(
            agents,
            probe,
            Paths {
                db: Some(database.to_path_buf()),
                config: Some(configuration.path.clone()),
                models_dir: Some(models.to_path_buf()),
            },
        )
        .await
        .map(Response::Installation),
        Operation::Forget { id } => maintenance::forget(database, models, configuration, id)
            .await
            .map(Response::Forget),
        Operation::ModelsInstall => {
            maintenance::install_models(models, configuration.embeddings.clone())
                .await
                .map(Response::ModelsInstall)
        }
        Operation::Init(_) => Err(AppError::internal("init runs before configuration loads")),
    }
}

fn required_shared(configuration: &ResolvedConfig) -> Result<&config::SharedConfig, AppError> {
    configuration
        .shared
        .as_ref()
        .ok_or_else(|| AppError::configuration("the shared index is not configured; add a `[shared]` table with `group_id`, `node_id`, `api_url` and `token_env` to the Mash configuration, or search with `--local`"))
}

/// Both mode publishes exactly the local index.
pub(crate) fn publication_config(configuration: &ResolvedConfig) -> Option<&config::SharedConfig> {
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

#[cfg(test)]
mod deadline_tests {
    use super::within;

    #[tokio::test]
    async fn a_stalled_search_returns_a_retryable_timeout() {
        let limit = std::time::Duration::from_millis(20);
        let stalled = std::future::pending::<Result<(), crate::AppError>>();
        let error = within(Some(limit), stalled).await.unwrap_err();
        assert_eq!(error.error.kind, crate::ErrorKind::SearchTimeout);
        assert!(error.error.retryable);
        // Work that finishes in time, or has no deadline, is untouched.
        assert!(within(Some(limit), async { Ok(()) }).await.is_ok());
        assert!(within(None, async { Ok(()) }).await.is_ok());
    }
}
