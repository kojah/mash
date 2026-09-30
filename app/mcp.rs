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

#[derive(Clone)]
struct Mash {
    paths: Arc<Paths>,
    // Mutations serialize, while read-only calls may run concurrently.
    gate: Arc<tokio::sync::Mutex<()>>,
    model_work: Arc<tokio::sync::Semaphore>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Search {
    query: String,
    limit: Option<usize>,
    provider: Option<Provider>,
    days: Option<u32>,
    /// Boolean metadata filter: all/any/not, node/provider/role/conversation/project/repository lists, after/before Unix timestamps.
    filter: Option<crate::search_filter::Filter>,
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
struct View {
    id: String,
    context: Option<u32>,
    /// Publishing node ID when reading the shared index.
    shared: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Index {
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
            Operation::Index(_) | Operation::Forget { .. } | Operation::ModelsInstall
        ) {
            Some(self.gate.lock().await)
        } else {
            None
        };
        let _permit = if matches!(
            operation,
            Operation::Search { .. } | Operation::Index(_) | Operation::ModelsInstall
        ) {
            match self.model_work.acquire().await {
                Ok(permit) => Some(permit),
                Err(_) => {
                    return Err(crate::AppError::internal("model worker unavailable"));
                }
            }
        } else {
            None
        };
        core::execute(Request {
            paths: self.paths.as_ref().clone(),
            operation,
        })
        .await
    }
}

fn tool_result(result: Result<core::Response, crate::AppError>) -> CallToolResult {
    match result {
        Ok(value) => match serde_json::to_value(&value) {
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

#[tool_router]
impl Mash {
    #[tool(
        annotations(read_only_hint = true, destructive_hint = false),
        description = "Search local SQLite plus other nodes in the configured shared index, fusing candidates and reranking together. Requires explicitly installed models and a completed index. Never runs ingestion or indexing: call index explicitly first. Falls back to local results with shared_error if the shared API fails. Set local or shared for a single index."
    )]
    async fn search(&self, Parameters(request): Parameters<Search>) -> CallToolResult {
        self.execute(Operation::Search {
            query: request.query,
            limit: request.limit.unwrap_or(20),
            filters: Box::new(crate::search_filter::Selection {
                provider: request.provider.into_iter().collect(),
                days: request.days,
                filter: request.filter,
                ..Default::default()
            }),
            shared: request.shared,
            local: request.local,
        })
        .await
    }

    #[tool(
        annotations(read_only_hint = true, destructive_hint = false),
        description = "Read a message and adjacent context. Supply shared with its publishing node ID to read shared history."
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
        description = "Report index/model readiness. Set shared=true to include per-node last_synced_at and last_updated_at from PostgreSQL. Shared-only queries PostgreSQL without opening SQLite."
    )]
    async fn status(&self, Parameters(request): Parameters<Status>) -> CallToolResult {
        self.execute(Operation::Status {
            shared: request.shared,
        })
        .await
    }

    #[tool(
        annotations(read_only_hint = false, destructive_hint = false),
        description = "Explicitly index Claude Code and Codex histories into local SQLite and optionally the shared index, applying provider allowlists. Streams phases, counts, and phase ETAs through MCP progress notifications when the request includes _meta.progressToken. Never downloads models."
    )]
    async fn index(
        &self,
        Parameters(request): Parameters<Index>,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> CallToolResult {
        self.execute_with_progress(
            Operation::Index(IndexRequest {
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
        description = "Forget a conversation owned by this node in its configured indexes. Shared-only deletes directly in PostgreSQL. Other nodes' copies are unaffected. Check shared.state for deferred deletion."
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
        info.instructions = Some("Search Claude Code and Codex history. Search/view/status are read-only. Index and forget write data; models_install explicitly downloads models. Provider allowlists and destinations are configured at startup. Search merges local results with other nodes in the shared group by default. Supply a shared result's node ID to view its message.".to_owned());
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
    };
    let background = server.clone();
    let service = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|_| crate::AppError::internal("MCP initialization failed"))?;
    let (stop, shutdown) = tokio::sync::watch::channel(false);
    let watcher = watch.then(|| {
        tokio::spawn(async move {
            let request = Request {
                paths: background.paths.as_ref().clone(),
                operation: Operation::Index(IndexRequest::default()),
            };
            let result = crate::watch::run(request, debounce_ms, shutdown, move || {
                let server = background.clone();
                async move {
                    let result = tokio::spawn(async move {
                        server
                            .perform_response(Operation::Index(IndexRequest::default()))
                            .await
                    })
                    .await
                    .map_err(|_| crate::AppError::internal("index worker failed"))?;
                    let retry = result.as_ref().map_or(true, core::Response::needs_retry);
                    crate::write_json(
                        &mut std::io::stderr(),
                        &WatchIndex {
                            event: WatchEvent::Index,
                            result: tool_result(result),
                        },
                    )?;
                    Ok(retry)
                }
            })
            .await;
            if let Err(error) = result {
                let _ = crate::write_json(&mut std::io::stderr(), &error);
            }
        })
    });
    let success = service.waiting().await.is_ok();
    let _ = stop.send(true);
    if let Some(watcher) = watcher {
        let _ = watcher.await;
    }
    if success {
        Ok(())
    } else {
        Err(crate::AppError::internal("MCP session failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::CallToolRequestParams;
    use serde_json::json;

    #[tokio::test]
    async fn typed_search_requests_validate_before_touching_models_or_storage() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        std::fs::write(&config, "version = 1\nproviders = {}\n").unwrap();
        let database = dir.path().join("missing.sqlite3");
        let models = dir.path().join("missing-models");
        let server = Mash {
            paths: Arc::new(Paths {
                config: Some(config),
                db: Some(database.clone()),
                models_dir: Some(models.clone()),
            }),
            gate: Arc::new(tokio::sync::Mutex::new(())),
            model_work: Arc::new(tokio::sync::Semaphore::new(1)),
        };
        let invalid_filter = crate::search_filter::Filter::Project(vec![String::new()]);
        for (local, shared, filter) in [(true, true, None), (true, false, Some(invalid_filter))] {
            let result = server
                .search(Parameters(Search {
                    query: "--models-dir=untrusted".into(),
                    limit: None,
                    provider: Some(Provider::Codex),
                    days: None,
                    filter,
                    local,
                    shared,
                }))
                .await;
            assert_eq!(result.is_error, Some(true));
            assert_eq!(result.structured_content.unwrap()["error"]["kind"], "usage");
        }
        assert!(!database.exists());
        assert!(!models.exists());
    }

    fn install_test_marker(models: &std::path::Path) {
        std::fs::create_dir(models).unwrap();
        std::fs::write(models.join("dummy-model"), [0]).unwrap();
        std::fs::write(
            models.join("installed.json"),
            serde_json::to_vec(&json!({
                "schema":1,"embedding_model":"AllMiniLML6V2Q",
                "reranker_model":"jinaai/jina-reranker-v1-turbo-en","embedding_dimensions":384,
                "coreml_embedding":cfg!(all(target_os = "macos", target_arch = "aarch64")),
                "files":[{"path":"dummy-model","size":1}]
            }))
            .unwrap(),
        )
        .unwrap();
    }

    async fn wire_send(
        writer: &mut tokio::io::WriteHalf<tokio::io::DuplexStream>,
        value: serde_json::Value,
    ) {
        use tokio::io::AsyncWriteExt;
        let mut bytes = serde_json::to_vec(&value).unwrap();
        bytes.push(b'\n');
        writer.write_all(&bytes).await.unwrap();
    }

    async fn wire_next(
        reader: &mut tokio::io::BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
    ) -> serde_json::Value {
        use tokio::io::AsyncBufReadExt;
        let mut line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            reader.read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        serde_json::from_str(&line).unwrap()
    }

    #[tokio::test]
    #[allow(
        clippy::too_many_lines,
        reason = "wire-level handshake, live progress, token omission, and failure form one session"
    )]
    async fn indexing_streams_correlated_progress_before_the_tool_result() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        let database = dir.path().join("index.sqlite3");
        let models = dir.path().join("models");
        std::fs::write(&config, "version = 1\nproviders = {}\n").unwrap();
        install_test_marker(&models);
        let server = Mash {
            paths: Arc::new(Paths {
                config: Some(config),
                db: Some(database.clone()),
                models_dir: Some(models.clone()),
            }),
            gate: Arc::new(tokio::sync::Mutex::new(())),
            model_work: Arc::new(tokio::sync::Semaphore::new(1)),
        };
        let gate = server.gate.clone();
        let held = gate.lock().await;
        let (server_io, client_io) = tokio::io::duplex(64 * 1024);
        let serving = tokio::spawn(async move { server.serve(server_io).await.unwrap() });
        let (read, mut write) = tokio::io::split(client_io);
        let mut read = tokio::io::BufReader::new(read);
        // Use the wire directly: rmcp's client replaces caller-supplied tokens
        // and adds one even when none was requested.
        wire_send(&mut write, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"progress-test","version":"1"}}})).await;
        assert_eq!(wire_next(&mut read).await["id"], 1);
        wire_send(
            &mut write,
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        )
        .await;
        let server = serving.await.unwrap();
        wire_send(&mut write, json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"index","arguments":{},"_meta":{"progressToken":"index-one"}}})).await;
        let waiting = wire_next(&mut read).await;
        assert_eq!(waiting["method"], "notifications/progress");
        assert_eq!(waiting["params"]["progressToken"], "index-one");
        assert_eq!(
            waiting["params"]["_meta"]["dev.jamesqo.mash/index-progress"]["phase"],
            "waiting-for-index"
        );
        assert!(
            !database.exists(),
            "progress arrives while indexing is still queued"
        );
        drop(held);
        let mut previous = waiting["params"]["progress"].as_f64().unwrap();
        let mut complete = false;
        loop {
            let message = wire_next(&mut read).await;
            if message.get("id").is_some() {
                assert_eq!(message["id"], 2);
                assert_ne!(message["result"]["isError"], true);
                assert_eq!(
                    message["result"]["structuredContent"]["indexed_messages"],
                    0
                );
                break;
            }
            assert_eq!(message["method"], "notifications/progress");
            let event: rmcp::model::ProgressNotificationParam =
                serde_json::from_value(message["params"].clone()).unwrap();
            assert_eq!(
                event.progress_token,
                rmcp::model::ProgressToken(rmcp::model::NumberOrString::String("index-one".into()))
            );
            assert!(event.progress > previous);
            assert!(
                event.total.is_none(),
                "phase totals are not an overall total"
            );
            previous = event.progress;
            let metadata: crate::progress::Event = serde_json::from_value(
                event.meta.unwrap()["dev.jamesqo.mash/index-progress"].clone(),
            )
            .unwrap();
            if metadata.phase == crate::progress::Phase::Complete {
                complete = true;
                assert_eq!(metadata.eta_seconds, Some(0));
                assert!(event.message.unwrap().contains("ETA 0m 00s"));
            }
        }
        assert!(complete, "completion progress precedes the tool result");
        wire_send(&mut write, json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"index","arguments":{}}})).await;
        let result = wire_next(&mut read).await;
        assert_eq!(result["id"], 3, "no token means no unsolicited progress");
        assert_ne!(result["result"]["isError"], true);
        std::fs::remove_file(models.join("dummy-model")).unwrap();
        wire_send(&mut write, json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"index","arguments":{},"_meta":{"progressToken":42}}})).await;
        let event = wire_next(&mut read).await;
        assert_eq!(event["params"]["progressToken"], 42);
        assert_eq!(event["params"]["progress"], 1.0);
        let result = wire_next(&mut read).await;
        assert_eq!(result["id"], 4);
        assert_eq!(result["result"]["isError"], true);
        assert_eq!(
            result["result"]["structuredContent"]["error"]["recommended_action"],
            "models install"
        );
        drop(write);
        drop(read);
        server.waiting().await.unwrap();
    }

    fn assert_filter_schema(tools: &[rmcp::model::Tool]) {
        let search = tools.iter().find(|tool| tool.name == "search").unwrap();
        assert!(search.input_schema["properties"].get("filter").is_some());
    }

    #[tokio::test]
    #[allow(
        clippy::too_many_lines,
        reason = "exercise a complete SDK session including explicit index readiness"
    )]
    async fn sdk_handshake_discovery_calls_and_errors_share_cli_contract() {
        let dir = tempfile::tempdir().expect("temp");
        let config = dir.path().join("config.toml");
        std::fs::write(
            &config,
            r#""version" = 1
"providers" = {}
"#,
        )
        .expect("config");
        let server = Mash {
            paths: Arc::new(Paths {
                config: Some(config),
                db: Some(dir.path().join("missing.sqlite3")),
                models_dir: Some(dir.path().join("models")),
            }),
            gate: Arc::new(tokio::sync::Mutex::new(())),
            model_work: Arc::new(tokio::sync::Semaphore::new(1)),
        };
        let write_gate = server.gate.clone();
        let model_work = server.model_work.clone();
        let (server_io, client_io) = tokio::io::duplex(64 * 1024);
        let server = tokio::spawn(async move { server.serve(server_io).await.expect("server") });
        let client = ().serve(client_io).await.expect("SDK client initialization");
        let server = server.await.expect("server task");
        let tools = client.list_all_tools().await.expect("tools/list");
        assert_filter_schema(&tools);
        let mut names = tools
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect::<Vec<_>>();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "forget",
                "index",
                "models_install",
                "search",
                "status",
                "view"
            ]
        );
        let write_guard = write_gate.lock().await;
        let model_permit = model_work.acquire().await.expect("model permit");
        let status = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            client.call_tool(CallToolRequestParams::new("status")),
        )
        .await
        .expect("status remains responsive during a mutation and model work")
        .expect("status");
        drop(model_permit);
        drop(write_guard);
        assert_ne!(status.is_error, Some(true));
        let value = serde_json::to_value(&status).expect("serialize");
        let body: serde_json::Value =
            serde_json::from_str(value["content"][0]["text"].as_str().expect("text"))
                .expect("JSON status");
        assert_eq!(body["ready"], false);
        assert_eq!(status.structured_content.as_ref(), Some(&body));
        assert_eq!(body["configuration"]["loaded"], true);
        let args = json!({"query":"--models-dir=untrusted", "shared":false})
            .as_object()
            .expect("object")
            .clone();
        let result = client
            .call_tool(CallToolRequestParams::new("search").with_arguments(args))
            .await
            .expect("tools/call");
        assert_eq!(result.is_error, Some(true));
        let value = serde_json::to_value(result).expect("serialize");
        let body: serde_json::Value =
            serde_json::from_str(value["content"][0]["text"].as_str().expect("text"))
                .expect("typed error");
        assert_eq!(
            body["error"]["kind"], "model",
            "query cannot inject CLI flags"
        );
        // Valid installation metadata with unusable inference assets proves
        // search checks the missing index before trying to load a model.
        let models = dir.path().join("models");
        std::fs::create_dir(&models).unwrap();
        std::fs::write(models.join("dummy-model"), [0]).unwrap();
        std::fs::write(
            models.join("installed.json"),
            serde_json::to_vec(&json!({
                "schema":1,"embedding_model":"AllMiniLML6V2Q",
                "reranker_model":"jinaai/jina-reranker-v1-turbo-en","embedding_dimensions":384,
                "coreml_embedding":cfg!(all(target_os = "macos", target_arch = "aarch64")),
                "files":[{"path":"dummy-model","size":1}]
            }))
            .unwrap(),
        )
        .unwrap();
        let missing = client
            .call_tool(
                CallToolRequestParams::new("search")
                    .with_arguments(json!({"query":"needle"}).as_object().unwrap().clone()),
            )
            .await
            .unwrap();
        assert_eq!(missing.is_error, Some(true));
        assert_eq!(
            missing.structured_content.as_ref().unwrap()["error"]["recommended_action"],
            "index"
        );
        assert!(!dir.path().join("missing.sqlite3").exists());
        let args = json!({"query":"q", "unexpected":true})
            .as_object()
            .expect("object")
            .clone();
        let invalid = client
            .call_tool(CallToolRequestParams::new("search").with_arguments(args))
            .await
            .expect("invalid argument result");
        assert_eq!(invalid.is_error, Some(true));
        assert!(
            client
                .call_tool(CallToolRequestParams::new("unknown"))
                .await
                .is_err()
        );
        client.cancel().await.expect("client shutdown");
        server.waiting().await.expect("server shutdown");
        assert!(!dir.path().join("missing.sqlite3").exists());
    }
}
