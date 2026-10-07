use crate::core::{self, IndexRequest, Operation, Paths, Request};
use crate::provider::Provider;
use futures_util::FutureExt;
use std::sync::Arc;

use rmcp::{
    ServiceExt,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    schemars, tool, tool_router,
};
use serde::Deserialize;

mod watching;

#[derive(Clone)]
struct Mash {
    paths: Arc<Paths>,
    // Mutations serialize, while read-only calls may run concurrently.
    gate: Arc<tokio::sync::Mutex<()>>,
    model_work: Arc<tokio::sync::Semaphore>,
    watcher: Arc<tokio::sync::Mutex<watching::Watcher>>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Search {
    /// Dense shared retrieval override; omitted inherits the group's active default.
    #[serde(rename = "shared_vector_search")]
    shared_vector: Option<crate::gold::vector::Search>,
    query: String,
    /// related-discussion (default) or model-free exact-mention.
    intent: Option<crate::gold::exact::Intent>,
    /// Maximum results; omitted uses gold.query.limit (20 by default).
    limit: Option<usize>,
    /// Threads or messages; omitted uses gold.query.unit (threads by default).
    unit: Option<crate::search_results::SearchUnit>,
    /// Partial hybrid fusion overrides; omitted fields use current TOML defaults.
    fusion: Option<crate::gold::config::FusionOverrides>,
    /// Hybrid ranking for this request; omitted inherits gold.query.ranking.
    ranking: Option<crate::gold::config::Ranking>,
    /// Include resolved retrieval policy, source outcomes and core timing.
    #[serde(default)]
    diagnostics: bool,
    #[serde(flatten)]
    filters: crate::search_filter::Selection,
    #[serde(default)]
    shared: bool,
    /// Search only the local SQLite index.
    #[serde(default)]
    local: bool,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Status {
    /// Query shared per-node synchronization and update timestamps.
    #[serde(default)]
    shared: bool,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListThreads {
    #[serde(flatten)]
    input: crate::thread_catalog::Request,
    #[serde(default)]
    local: bool,
    #[serde(default)]
    shared: bool,
    /// Include metadata source outcomes and listing/merge timings.
    #[serde(default)]
    diagnostics: bool,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct View {
    id: String,
    context: Option<u32>,
    /// Publishing node ID only for evidence retrieved from the shared index.
    /// Omit for local evidence, even when it includes this machine's `node_id`.
    shared: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListAttachments {
    message_id: String,
    shared: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct GetAttachment {
    message_id: String,
    attachment_id: String,
    shared: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ExportSilver {
    /// New local directory; must not already exist. Contains sensitive silver history.
    directory: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ImportSilver {
    /// Local export directory created by `export_silver`.
    directory: String,
    /// New SQLite database path; existing databases and attachment directories are refused.
    database: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "SDK arguments mirror CLI flags; core validates and uses enum-selected indexing modes"
)]
struct Index {
    /// Rebuild only local gold from certified stored silver, without provider reads or shared publication.
    #[serde(default)]
    rebuild_local_gold: bool,
    #[serde(default)]
    full: bool,
    #[serde(default)]
    provider: Vec<Provider>,
    since_days: Option<u32>,
    #[serde(default)]
    all_history: bool,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Forget {
    conversation_id: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct WatchStart {
    /// Quiet period in milliseconds, 50–60000; defaults to 1000 on a new start.
    #[schemars(range(min = 50, max = 60_000))]
    debounce_ms: Option<u64>,
}

impl Mash {
    async fn execute(&self, operation: Operation) -> CallToolResult {
        self.execute_with_progress(operation, None).await
    }

    async fn execute_with_progress(
        &self,
        operation: Operation,
        context: Option<rmcp::service::RequestContext<rmcp::RoleServer>>,
    ) -> CallToolResult {
        // Blocking work cannot be interrupted once started. Keep its guards in
        // the operation task even if the client stops awaiting this request.
        let server = self.clone();
        tokio::spawn(async move {
            match context.filter(|ctx| ctx.meta.get_progress_token().is_some()) {
                Some(ctx) => server.perform_streamed(operation, ctx).await,
                None => server.perform(operation).await,
            }
        })
        .await
        .unwrap_or_else(|_| CallToolResult::error(vec![ContentBlock::text("tool worker failed")]))
    }

    async fn perform_streamed(
        &self,
        operation: Operation,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> CallToolResult {
        let Some(token) = context.meta.get_progress_token() else {
            return self.perform(operation).await;
        };
        let (sender, mut events) = tokio::sync::mpsc::channel(64);
        let (finished, done) = tokio::sync::oneshot::channel();
        let work = async {
            let result = crate::progress::scope(sender, async {
                crate::progress::phase(crate::progress::Phase::WaitingForIndex, 0);
                self.perform(operation).await
            })
            .boxed()
            .await;
            let _ = finished.send(());
            result
        };
        let notifications = async {
            tokio::pin!(done);
            let mut sequence = 0.0;
            let mut complete = false;
            loop {
                let event = tokio::select! {
                    _ = &mut done, if !complete => {
                        complete = true;
                        events.close();
                        continue;
                    }
                    event = events.recv() => match event { Some(event) => event, None => break },
                };
                sequence += 1.0;
                let mut notification =
                    rmcp::model::ProgressNotificationParam::new(token.clone(), sequence)
                        .with_message(crate::progress::display(&event));
                let mut meta = rmcp::model::NotificationMetaObject::new();
                if let Ok(event) = serde_json::to_value(&event) {
                    meta.insert("dev.jamesqo.mash/index-progress".to_owned(), event);
                }
                notification.meta = Some(meta);
                if !matches!(
                    tokio::time::timeout(
                        std::time::Duration::from_secs(2),
                        context.peer.notify_progress(notification)
                    )
                    .await,
                    Ok(Ok(()))
                ) {
                    break;
                }
            }
        };
        let (result, ()) = tokio::join!(work, notifications);
        result
    }

    async fn perform(&self, operation: Operation) -> CallToolResult {
        tool_result(self.perform_response(operation).await)
    }

    async fn perform_response(
        &self,
        operation: Operation,
    ) -> Result<core::Response, crate::AppError> {
        let _guard = if matches!(
            operation,
            Operation::Index(_)
                | Operation::Forget { .. }
                | Operation::ModelsInstall
                | Operation::ExportSilver { .. }
                | Operation::ImportSilver { .. }
        ) {
            Some(self.gate.lock().await)
        } else {
            None
        };
        core::execute_with_model_limit(
            Request {
                paths: self.paths.as_ref().clone(),
                operation,
            },
            &self.model_work,
        )
        .await
    }
}

fn tool_result(result: Result<core::Response, crate::AppError>) -> CallToolResult {
    match result {
        Ok(core::Response::GetAttachment(file)) => attachment_result(file),
        Ok(value) => match serde_json::to_value(&value) {
            Ok(json) if value.needs_retry() => CallToolResult::structured_error(json),
            Ok(json) => CallToolResult::structured(json),
            Err(_) => CallToolResult::error(vec![ContentBlock::text(
                "failed to serialize tool response",
            )]),
        },
        Err(error) => match serde_json::to_value(&error) {
            Ok(json) => CallToolResult::structured_error(json),
            Err(_) => {
                CallToolResult::error(vec![ContentBlock::text("failed to serialize tool error")])
            }
        },
    }
}

fn attachment_result(file: crate::attachments::Retrieved) -> CallToolResult {
    let uri = format!(
        "mash-attachment://{}/{}",
        file.message_id, file.attachment.id
    );
    let media_type = file.attachment.media_type.clone();
    let block = if matches!(
        media_type.as_str(),
        "image/png" | "image/jpeg" | "image/gif" | "image/webp"
    ) {
        ContentBlock::image(file.data, media_type)
    } else {
        ContentBlock::resource(
            rmcp::model::ResourceContents::blob(file.data, uri).with_mime_type(media_type),
        )
    };
    match serde_json::to_value(crate::attachments::Listing {
        message_id: file.message_id,
        attachments: vec![file.attachment],
    }) {
        Ok(json) => {
            let mut result = CallToolResult::structured(json);
            result.content.push(block);
            result
        }
        Err(_) => CallToolResult::error(vec![ContentBlock::text(
            "failed to serialize attachment metadata",
        )]),
    }
}

#[tool_router]
impl Mash {
    #[tool(
        annotations(read_only_hint = false, destructive_hint = false),
        description = "Start this MCP session's history watcher and an initial indexing pass. Returns promptly; poll watch_status for progress, errors and shared publication results. Uses TOML providers, destinations and credentials; never downloads models. Repeating a start preserves the active watcher; stop before changing debounce_ms. Settings are process-local."
    )]
    async fn watch_start(&self, Parameters(input): Parameters<WatchStart>) -> CallToolResult {
        watching::start(self.clone(), input.debounce_ms).await
    }

    #[tool(
        annotations(read_only_hint = true, destructive_hint = false),
        description = "List attachment metadata for a message without loading file contents or models. Omit shared for local evidence, or supply the publishing node ID for shared evidence. Returns stable attachment IDs, names, MIME types, sizes, redaction status and explicit availability, including not_uploaded for shared metadata without bytes. Only captured provider attachments are discoverable; reindex older histories to capture them."
    )]
    async fn list_attachments(
        &self,
        Parameters(input): Parameters<ListAttachments>,
    ) -> CallToolResult {
        self.execute(Operation::ListAttachments {
            message_id: input.message_id,
            shared: input.shared,
        })
        .await
    }

    #[tool(
        annotations(read_only_hint = true, destructive_hint = false),
        description = "Retrieve an attachment by message_id and attachment_id from list_attachments. Omit shared for local evidence, or supply the publishing node ID for uploaded shared files. Returns metadata plus native MCP image content or a base64 embedded resource, capped at 4 MiB per file. No arbitrary path reads or URL downloads. UTF-8 content is credential-redacted; binary images/PDFs are not sanitized. Missing or unretained files return an explicit error."
    )]
    async fn get_attachment(&self, Parameters(input): Parameters<GetAttachment>) -> CallToolResult {
        self.execute(Operation::GetAttachment {
            message_id: input.message_id,
            attachment_id: input.attachment_id,
            shared: input.shared,
        })
        .await
    }

    #[tool(
        annotations(read_only_hint = false, destructive_hint = false),
        description = "Stop future watched refreshes in this MCP session. Returns promptly with stopping while an active indexing pass finishes; poll watch_status until stopped. Safe to repeat."
    )]
    async fn watch_stop(&self) -> CallToolResult {
        watching::stop(self).await
    }

    #[tool(
        annotations(read_only_hint = true, destructive_hint = false),
        description = "Report this MCP session's watcher state, debounce_ms, indexing progress, last completed refresh/result (including shared publication state), and last error. Does not start indexing."
    )]
    async fn watch_status(&self) -> CallToolResult {
        self.watcher.lock().await.response()
    }

    #[tool(
        annotations(read_only_hint = true, destructive_hint = false),
        description = "Quickly list thread metadata without models or inference. Filter by provider, node, literal title substring or thread activity cutoff (Unix seconds). Sort by updated_at (default), created_at or title, asc/desc. Returned timestamps are Unix milliseconds. Default scope combines local and other nodes; local/shared select one index. Use returned conversation_id and provider in search with unit=messages. Unknown timestamps sort last. Pagination is best effort during updates; offset is capped at 10000."
    )]
    async fn list_threads(&self, Parameters(input): Parameters<ListThreads>) -> CallToolResult {
        self.execute(Operation::ListThreads {
            input: Box::new(input.input),
            local: input.local,
            shared: input.shared,
            diagnostics: input.diagnostics,
        })
        .await
    }

    #[tool(
        annotations(read_only_hint = true, destructive_hint = false),
        description = "Search using current gold.query defaults; override intent, unit, limit, ranking or partial fusion settings. Ranking overrides select fusion or contextual-rerank for one request. Ranking and fusion overrides apply only to related-discussion and inherit omitted fields from TOML. related-discussion uses hybrid local/shared retrieval and installed models. exact-mention matches one literal phrase or identifier in visible text, with ASCII case folding and punctuation preserved, without inference, locally and through the shared service. Narrow broad exact queries with metadata filters if the shared candidate or time budget is reached. Thread excerpts include message_id anchors for view; timestamps are Unix milliseconds. All modes require completed current retrieval artifacts and never run ingestion. Combined shared candidate failures fall back locally with shared_error; shared-only candidate failures return an error. Later context failures retain shared evidence and mode with shared_context_error. Optional diagnostics include resolved policy and measured stages."
    )]
    async fn search(&self, Parameters(request): Parameters<Search>) -> CallToolResult {
        self.execute(Operation::Search {
            shared_vector_search: request.shared_vector,
            query: request.query,
            intent: request.intent,
            limit: request.limit,
            unit: request.unit,
            fusion: request.fusion.unwrap_or_default(),
            ranking: request.ranking,
            diagnostics: request.diagnostics,
            filters: Box::new(request.filters),
            shared: request.shared,
            local: request.local,
        })
        .await
    }

    #[tool(
        annotations(read_only_hint = true, destructive_hint = false),
        description = "Read a message and adjacent context. Omit shared for local evidence, even when it has a node_id. Supply shared with the publishing node ID only for evidence retrieved from the shared index. A node_id alone does not mean the message is published."
    )]
    async fn view(&self, Parameters(request): Parameters<View>) -> CallToolResult {
        self.execute(Operation::View {
            id: request.id,
            context: request.context.unwrap_or(0),
            shared: request.shared,
        })
        .await
    }

    #[tool(
        annotations(read_only_hint = true, destructive_hint = false),
        description = "Report index/model readiness. Set shared=true to include each node's artifact publication states, last_synced_at and last_updated_at from the shared HTTP service backed by Qdrant. Artifact readiness does not certify complete provider history or installed query models."
    )]
    async fn status(&self, Parameters(request): Parameters<Status>) -> CallToolResult {
        self.execute(Operation::Status {
            shared: request.shared,
        })
        .await
    }

    #[tool(
        annotations(read_only_hint = false, destructive_hint = false),
        description = "Explicitly refresh/index configured agent histories into local SQLite and optionally the shared index, applying provider allowlists. Subsequent searches read the refreshed index without restarting MCP. Set rebuild_local_gold to build only local retrieval state from certified stored silver, offline and without provider reads (with full, all local gold is discarded and replayed from silver). The rebuild is incompatible with provider/time selectors. Streams phases, counts, and phase ETAs through MCP progress notifications when the request includes _meta.progressToken. Never downloads models."
    )]
    async fn index(
        &self,
        Parameters(request): Parameters<Index>,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> CallToolResult {
        let mode = core::IndexMode::from_rebuild_flag(request.rebuild_local_gold);
        self.execute_with_progress(
            Operation::Index(IndexRequest {
                mode,
                full: request.full,
                provider: request.provider,
                since_days: request.since_days,
                all_history: request.all_history,
            }),
            Some(context),
        )
        .await
    }

    #[tool(
        annotations(read_only_hint = false, destructive_hint = true),
        description = "Forget a conversation owned by this node in its configured indexes. Other nodes' copies are unaffected. Check shared.state for deferred deletion."
    )]
    async fn forget(&self, Parameters(request): Parameters<Forget>) -> CallToolResult {
        self.execute(Operation::Forget {
            id: request.conversation_id,
        })
        .await
    }

    #[tool(
        annotations(read_only_hint = false, destructive_hint = false),
        description = "Explicitly download and install embedding and reranking models. Requires network access and writes model assets."
    )]
    async fn models_install(&self) -> CallToolResult {
        self.execute(Operation::ModelsInstall).await
    }

    #[tool(
        annotations(read_only_hint = false, destructive_hint = false),
        description = "Export local silver to a new directory, including retained attachments and deletion/checkpoint evidence, without models or network access. Excludes gold/models/config. Contains canonical private history, not redacted search output."
    )]
    async fn export_silver(&self, Parameters(request): Parameters<ExportSilver>) -> CallToolResult {
        self.execute(Operation::ExportSilver {
            directory: request.directory.into(),
        })
        .await
    }

    #[tool(
        annotations(read_only_hint = false, destructive_hint = false),
        description = "Verify and import a local silver export to a new database. Never overwrites or merges existing history and does not change this MCP session's configured database. Search requires explicit gold rebuild using the imported database."
    )]
    async fn import_silver(&self, Parameters(request): Parameters<ImportSilver>) -> CallToolResult {
        self.execute(Operation::ImportSilver {
            directory: request.directory.into(),
            database: request.database.into(),
        })
        .await
    }
}

// The SDK macro emits an immediate async tools/list handler.
#[allow(clippy::unused_async_trait_impl)]
#[rmcp::tool_handler(router = Self::tool_router())]
impl rmcp::ServerHandler for Mash {
    fn get_info(&self) -> rmcp::model::ServerConfig {
        let mut info = rmcp::model::ServerConfig::default();
        info.capabilities = rmcp::model::ServerCapabilities::builder()
            .enable_tools()
            .build();
        info.server_info = rmcp::model::Implementation::new("mash", env!("CARGO_PKG_VERSION"));
        info.instructions = Some("Search configured agent histories. Search/view/status are read-only. Index and forget write data; models_install explicitly downloads models. Provider allowlists and destinations are configured at startup. Search merges local results with other nodes in the shared group by default. Supply a shared result's node ID to view its message.".to_owned());
        info
    }
}

#[derive(serde::Serialize)]
struct WatchIndex {
    event: WatchEvent,
    result: CallToolResult,
}
#[derive(serde::Serialize)]
enum WatchEvent {
    #[serde(rename = "watch-index")]
    Index,
}

pub(crate) async fn run(
    paths: Paths,
    watch: bool,
    debounce_ms: u64,
) -> Result<(), crate::AppError> {
    let server = Mash {
        paths: Arc::new(paths),
        gate: Arc::new(tokio::sync::Mutex::new(())),
        model_work: Arc::new(tokio::sync::Semaphore::new(1)),
        watcher: Arc::default(),
    };
    let background = server.clone();
    let service = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|_| crate::AppError::internal("MCP initialization failed"))?;
    if watch {
        let _ = watching::start(background.clone(), Some(debounce_ms)).await;
    }
    let success = service.waiting().await.is_ok();
    watching::shutdown(&background).await;
    if success {
        Ok(())
    } else {
        Err(crate::AppError::internal("MCP session failed"))
    }
}

#[cfg(test)]
mod tests;
