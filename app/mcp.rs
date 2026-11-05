use crate::core::output::Detail;
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

pub(crate) mod guidance;

#[derive(Clone)]
struct Mash {
    paths: Arc<Paths>,
    // Mutations serialize, while read-only calls may run concurrently.
    gate: Arc<tokio::sync::Mutex<()>>,
    model_work: Arc<tokio::sync::Semaphore>,
    runtime: Option<Arc<crate::runtime::Lease>>,
}

#[derive(Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Search {
    /// compact (default) returns bounded excerpts; extended includes full hits and scores.
    #[serde(default)]
    detail: Detail,
    /// Dense shared retrieval override; omitted inherits the group's active default.
    #[serde(rename = "shared_vector_search")]
    shared_vector: Option<crate::views::vector::Search>,
    /// Words, a semantic question, or an exact-mention phrase/identifier; search never ingests.
    query: String,
    /// related-discussion or model-free exact-mention; omitted uses views.query.intent,
    /// which is exact-mention when semantic models were not installed at setup.
    intent: Option<crate::views::exact::Intent>,
    /// Results per page, from 1 to 200; omitted uses views.query.limit (20 by default).
    /// Use `next_offset` to fetch another page.
    #[schemars(range(min = 1, max = 200))]
    limit: Option<usize>,
    /// Page offset into one ranked list (default 0): below 200 for chats, 50 for messages.
    /// Repeat the same search with the returned `next_offset`; null means the last page.
    #[schemars(range(min = 0, max = 199))]
    offset: Option<usize>,
    /// Chats or chat messages; omitted uses views.query.unit (chats by default).
    unit: Option<crate::search_results::SearchUnit>,
    /// Advanced related-discussion fusion overrides; omitted fields inherit TOML. Invalid for exact-mention.
    fusion: Option<crate::views::config::FusionOverrides>,
    /// Advanced related-discussion ranking: fusion or contextual-rerank; omitted inherits TOML. Invalid for exact-mention.
    ranking: Option<crate::views::config::Ranking>,
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

impl Search {
    fn operation(self) -> Operation {
        Operation::Search {
            shared_vector_search: self.shared_vector,
            query: self.query,
            intent: self.intent,
            limit: self.limit,
            offset: self.offset.unwrap_or_default(),
            unit: self.unit,
            fusion: self.fusion.unwrap_or_default(),
            ranking: self.ranking,
            diagnostics: self.diagnostics,
            filters: Box::new(self.filters),
            shared: self.shared,
            local: self.local,
        }
    }
}

#[derive(Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Status {
    /// Optional provider/node/message-time scope. Reports retained rows and scan evidence;
    /// unknown coverage never proves that matching conversations do not exist.
    coverage: Option<core::coverage::Scope>,
    /// compact (default) shows readiness and freshness; extended includes manifests/configuration.
    #[serde(default)]
    detail: Detail,
    /// Query shared per-node synchronization and update timestamps.
    #[serde(default)]
    shared: bool,
}

#[derive(Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListChats {
    /// compact (default) omits enrichment provenance; extended includes all metadata.
    #[serde(default)]
    detail: Detail,
    #[serde(flatten)]
    input: crate::chat_catalog::Request,
    #[serde(default)]
    local: bool,
    #[serde(default)]
    shared: bool,
    /// Include metadata source outcomes and listing/merge timings.
    #[serde(default)]
    diagnostics: bool,
}

#[derive(Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct View {
    /// Copy lookup.view from a search hit; id is the message anchor, not the chat ID.
    id: String,
    /// Number of adjacent messages on each side (default 0); message bodies are never truncated.
    context: Option<u32>,
    /// Publishing node ID only for evidence retrieved from the shared index.
    /// Omit for local evidence, even when it includes this machine's `node_id`.
    shared: Option<String>,
}

#[derive(Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListAttachments {
    message_id: String,
    shared: Option<String>,
}

#[derive(Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct GetAttachment {
    message_id: String,
    attachment_id: String,
    shared: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct QuerySilver {
    /// Typed, declarative query. Preferred over sql: it cannot name tables,
    /// columns or SQL fragments and works the same way for shared silver.
    request: Option<crate::silver_query::Request>,
    /// One read-only SQL statement over local silver. Local diagnostics only.
    sql: Option<String>,
    /// Query the configured shared index instead of local silver. Requires
    /// request; SQL is never sent to another machine.
    #[serde(default)]
    shared: bool,
    /// Shared node to read; defaults to this node and must belong to the
    /// token's group. Requires shared.
    #[serde(default)]
    node: Option<String>,
    /// Maximum rows in sql mode; request carries its own limit.
    limit: Option<usize>,
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
    /// Continue the exporting node's shared identity; for upgrades that retire it.
    #[serde(default)]
    keep_identity: bool,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "SDK arguments mirror CLI flags; core validates and uses enum-selected indexing modes"
)]
struct Index {
    /// Rebuild only local views from certified stored silver, without provider reads or shared publication.
    #[serde(default)]
    rebuild_local_views: bool,
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
    chat_id: String,
}

impl Mash {
    async fn execute_detailed(&self, operation: Operation, detail: Detail) -> CallToolResult {
        let server = self.clone();
        tokio::spawn(async move {
            tool_result_detailed(server.perform_response(operation).await, detail)
        })
        .await
        .unwrap_or_else(|_| CallToolResult::error(vec![ContentBlock::text("tool worker failed")]))
    }

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
        let guard = if matches!(
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
        if guard.is_some()
            && let Some(lease) = &self.runtime
        {
            let lease = Arc::clone(lease);
            crate::blocking(move || lease.ensure_current()).await?;
        }
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
    tool_result_detailed(result, Detail::Extended)
}

fn tool_result_detailed(
    result: Result<core::Response, crate::AppError>,
    detail: Detail,
) -> CallToolResult {
    match result {
        Ok(core::Response::GetAttachment(file)) => attachment_result(file),
        Ok(value) => match core::output::serialize(&value, detail) {
            Ok(json) => match core::output::text::render(&value, detail, &json) {
                Ok(text) => {
                    let mut result = if value.needs_retry() {
                        CallToolResult::structured_error(json)
                    } else {
                        CallToolResult::structured(json)
                    };
                    result.content = vec![ContentBlock::text(text)];
                    result
                }
                Err(_) => CallToolResult::error(vec![ContentBlock::text(
                    "failed to serialize tool response",
                )]),
            },
            Err(_) => CallToolResult::error(vec![ContentBlock::text(
                "failed to serialize tool response",
            )]),
        },
        Err(error) => match (
            serde_json::to_value(&error),
            core::output::text::error(&error),
        ) {
            (Ok(json), Ok(text)) => {
                let mut result = CallToolResult::structured_error(json);
                result.content = vec![ContentBlock::text(text)];
                result
            }
            _ => CallToolResult::error(vec![ContentBlock::text("failed to serialize tool error")]),
        },
    }
}

fn attachment_result(file: crate::attachments::Retrieved) -> CallToolResult {
    let listing = crate::attachments::Listing {
        message_id: file.message_id.clone(),
        attachments: vec![file.attachment.clone()],
    };
    let response = core::Response::ListAttachments(listing);
    let mut result = tool_result(Ok(response));
    if result.is_error == Some(true) {
        return result;
    }
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
    result.content.push(block);
    result
}

#[tool_router]
impl Mash {
    #[tool(
        annotations(read_only_hint = true, destructive_hint = false),
        description = "List attachment metadata without loading contents or models. Copy lookup.list_attachments from a search result. Omit shared for local evidence, or supply the publishing node ID for shared evidence. Returns stable attachment IDs, names, MIME types, sizes, redaction status and explicit availability, including not_uploaded for shared metadata without bytes. Only captured provider attachments are discoverable; reindex older histories to capture them."
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
        annotations(read_only_hint = true, destructive_hint = false),
        description = "Quickly list chat metadata without models or inference. Omit unused filters; title_contains must be nonempty when supplied. Filter by provider, node, literal title substring or chat activity cutoff (Unix seconds). Sort by updated_at (default), created_at or title, asc/desc. Returned timestamps are Unix milliseconds. Default scope combines local and other nodes; local/shared select one index. Use returned chat_id and provider in search with unit=messages and chat filters. Unknown timestamps sort last. Pagination is best effort during updates; offset is capped at 10000."
    )]
    async fn list_chats(&self, Parameters(input): Parameters<ListChats>) -> CallToolResult {
        self.execute_detailed(
            Operation::ListChats {
                input: Box::new(input.input),
                local: input.local,
                shared: input.shared,
                diagnostics: input.diagnostics,
            },
            input.detail,
        )
        .await
    }

    #[tool(
        annotations(read_only_hint = true, destructive_hint = false),
        description = "Find past conversations or messages. Use related-discussion for semantic questions, exact-mention for phrases or identifiers without inference. Results default to chats with compact evidence; drill down with unit=messages and chat=[returned chat IDs]. Omit unused filters. Copy lookup.view into view to read complete evidence. Lexical matching defaults to flexible (prefixes and conservative typos); use strict for exact spelling. Quoted whole queries and numbers stay strict. For more results repeat the same search with offset=next_offset; null next_offset is the last page. Pages slice one ranked list of at most 200 chats or 50 messages; sort=recent orders each page. Search never indexes. Partial shared failures retain local results and report shared_error; check status for readiness or coverage gaps. Call help for live examples or contract drift."
    )]
    async fn search(&self, Parameters(request): Parameters<Search>) -> CallToolResult {
        let detail = request.detail;
        self.execute_detailed(request.operation(), detail).await
    }

    #[tool(
        annotations(read_only_hint = true, destructive_hint = false),
        description = "Read complete message text and surrounding replies after discovery. Copy lookup.view from a search result and add context when needed; bodies are never truncated. To read further, view the first or last returned message id with the same shared value. Omit shared for local evidence, even when it has a node_id. Supply shared with the publishing node ID only for evidence retrieved from the shared index. A node_id alone does not mean the message is published."
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
        description = "Report local readiness and optional shared freshness. Supply coverage with provider/node/message-time selectors for retained-row and recorded-scan facts; unknown coverage never proves absence. Set shared=true to include each node's artifact publication states, last_synced_at and last_updated_at from the shared HTTP service backed by Qdrant. Artifact readiness does not certify complete provider history or installed query models."
    )]
    async fn status(&self, Parameters(request): Parameters<Status>) -> CallToolResult {
        let mut result = self
            .execute_detailed(
                Operation::Status {
                    shared: request.shared,
                    coverage: request.coverage,
                },
                request.detail,
            )
            .await;
        guidance::attach_runtime(&mut result, None).await;
        result
    }

    #[tool(
        annotations(read_only_hint = true, destructive_hint = false),
        description = "Read concise usage, typed examples and input schema from this running MCP binary. Optional tool or topic narrows guidance. Supply expected_contract from the history skill to detect stale instructions. Reports build revision, contract fingerprint and whether the supplied history skill contract matches. No index, models, configuration or network needed."
    )]
    async fn help(&self, Parameters(request): Parameters<guidance::Help>) -> CallToolResult {
        guidance::help(request).await
    }

    #[tool(
        annotations(read_only_hint = false, destructive_hint = false),
        description = "Explicitly refresh/index configured agent histories into local SQLite silver and Qdrant views, and optionally publish to the shared index, applying provider allowlists. Local indexing requires the configured Qdrant service, without a local Mash HTTP server. Subsequent searches read the refreshed index without restarting MCP. Set rebuild_local_views to rebuild local Qdrant retrieval state from certified stored silver without provider reads (with full, replay all local views). The rebuild is incompatible with provider/time selectors. Streams phases, counts, and phase ETAs through MCP progress notifications when the request includes _meta.progressToken. Never downloads models."
    )]
    async fn index(
        &self,
        Parameters(request): Parameters<Index>,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> CallToolResult {
        let mode = core::IndexMode::from_rebuild_flag(request.rebuild_local_views);
        self.execute_with_progress(
            Operation::Index(IndexRequest {
                mode,
                full: request.full,
                provider: request.provider,
                since_days: request.since_days,
                all_history: request.all_history,
                defer_publication: false,
                backfill: None,
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
            id: request.chat_id,
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
        description = "Export local silver to a new directory, including retained attachments and deletion/checkpoint evidence, without models or network access. Excludes views/models/config. Contains canonical private history, not redacted search output."
    )]
    async fn export_silver(&self, Parameters(request): Parameters<ExportSilver>) -> CallToolResult {
        self.execute(Operation::ExportSilver {
            directory: request.directory.into(),
        })
        .await
    }

    #[tool(
        annotations(read_only_hint = true, destructive_hint = false),
        description = "Query silver with a typed, declarative request (preferred): target messages/sessions/turns, filter by provider, session, role, time or substring, group by provider/role/session, order and limit. Identifiers come from an allowlist and values are bound, so a request cannot name a table, column or SQL fragment. Set shared=true to read the configured shared index, optionally naming a node in this token's group; otherwise the request reads this machine. The local-only sql escape hatch runs one read-only statement with query_only, a read-only authorizer, row and payload caps and a wall-clock budget, and is never sent to shared silver. Provide exactly one of request or sql. SQL reads the canonical sessions, messages and turns tables; the raw schema is release-specific."
    )]
    async fn query_silver(&self, Parameters(input): Parameters<QuerySilver>) -> CallToolResult {
        if input.shared {
            let Some(request) = input.request else {
                return CallToolResult::error(vec![ContentBlock::text(
                    "shared queries require request; SQL is local only",
                )]);
            };
            return self
                .execute(Operation::QueryShared {
                    request,
                    node: input.node,
                })
                .await;
        }
        if input.node.is_some() {
            return CallToolResult::error(vec![ContentBlock::text(
                "node requires shared; local queries always read this machine",
            )]);
        }
        match (input.request, input.sql) {
            (Some(request), None) => {
                self.execute(Operation::QueryStructured {
                    request,
                    timeout_seconds: 10,
                })
                .await
            }
            (None, Some(sql)) => {
                self.execute(Operation::QuerySilver {
                    sql,
                    limit: input.limit.unwrap_or(200),
                    timeout_seconds: 10,
                })
                .await
            }
            _ => CallToolResult::error(vec![ContentBlock::text(
                "provide exactly one of request or sql",
            )]),
        }
    }

    #[tool(
        annotations(read_only_hint = false, destructive_hint = false),
        description = "Verify and import a local silver export to a new database. Never overwrites or merges existing history and does not change this MCP session's configured database. Search requires explicit views rebuild using the imported database."
    )]
    async fn import_silver(&self, Parameters(request): Parameters<ImportSilver>) -> CallToolResult {
        self.execute(Operation::ImportSilver {
            directory: request.directory.into(),
            database: request.database.into(),
            keep_identity: request.keep_identity,
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
        info.instructions = Some(guidance::instructions());
        info
    }
}

pub(crate) async fn run(paths: Paths) -> Result<(), crate::AppError> {
    let registered_paths = paths.clone();
    let lease = crate::blocking(move || {
        crate::runtime::Lease::register(&registered_paths, crate::runtime::Kind::Mcp)
    })
    .await?;
    let server = Mash {
        paths: Arc::new(paths),
        gate: Arc::new(tokio::sync::Mutex::new(())),
        model_work: Arc::new(tokio::sync::Semaphore::new(1)),
        runtime: Some(Arc::new(lease)),
    };
    let service = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|_| crate::AppError::internal("MCP initialization failed"))?;
    service
        .waiting()
        .await
        .map(|_| ())
        .map_err(|_| crate::AppError::internal("MCP session failed"))
}

#[cfg(test)]
mod tests;
