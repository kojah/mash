use std::ffi::OsString;
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
    arguments: Arc<Vec<OsString>>,
    // Mutations serialize, while read-only calls may run concurrently.
    gate: Arc<tokio::sync::Mutex<()>>,
    model_work: Arc<tokio::sync::Semaphore>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Search {
    query: String,
    limit: Option<usize>,
    provider: Option<String>,
    days: Option<u32>,
    #[serde(default)]
    shared: bool,
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
    provider: Vec<String>,
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
    async fn execute(&self, arguments: Vec<OsString>) -> CallToolResult {
        // Blocking work cannot be interrupted once started. Keep its guards in
        // the operation task even if the client stops awaiting this request.
        let server = self.clone();
        tokio::spawn(async move { server.perform(arguments).await })
            .await
            .unwrap_or_else(|_| {
                CallToolResult::error(vec![ContentBlock::text("tool worker failed")])
            })
    }

    async fn perform(&self, arguments: Vec<OsString>) -> CallToolResult {
        let operation = arguments
            .first()
            .and_then(|arg| arg.to_str())
            .unwrap_or_default();
        let _guard = if matches!(operation, "index" | "forget" | "models") {
            Some(self.gate.lock().await)
        } else {
            None
        };
        let _permit = if matches!(operation, "search" | "index" | "models") {
            match self.model_work.acquire().await {
                Ok(permit) => Some(permit),
                Err(_) => {
                    return CallToolResult::error(vec![ContentBlock::text(
                        "model worker unavailable",
                    )]);
                }
            }
        } else {
            None
        };
        let mut all = self.arguments.as_ref().clone();
        all.extend(arguments);
        match crate::cli::run_async(all).await {
            Ok(value) => match serde_json::to_value(&value) {
                Ok(json) => CallToolResult::structured(json),
                Err(_) => CallToolResult::error(vec![ContentBlock::text(
                    "failed to serialize tool response",
                )]),
            },
            Err(error) => CallToolResult::structured_error(
                serde_json::to_value(&error).unwrap_or_else(|_| serde_json::json!({"error":{"kind":"internal","message":"tool failed","retryable":false}})),
            ),
        }
    }
}

fn flag(arguments: &mut Vec<OsString>, name: &str, value: Option<impl ToString>) {
    if let Some(value) = value {
        arguments.extend([name.into(), value.to_string().into()]);
    }
}

#[tool_router]
impl Mash {
    #[tool(
        annotations(read_only_hint = true, destructive_hint = false),
        description = "Search local chat history, or the configured node group's shared PostgreSQL index. Requires explicitly installed models."
    )]
    async fn search(&self, Parameters(request): Parameters<Search>) -> CallToolResult {
        let mut args = vec!["search".into()];
        flag(&mut args, "--limit", request.limit);
        flag(&mut args, "--provider", request.provider);
        flag(&mut args, "--days", request.days);
        if request.shared {
            args.push("--shared".into());
        }
        args.extend(["--".into(), request.query.into()]);
        self.execute(args).await
    }

    #[tool(
        annotations(read_only_hint = true, destructive_hint = false),
        description = "Read a message and adjacent context. Supply shared with its publishing node ID to read shared history."
    )]
    async fn view(&self, Parameters(request): Parameters<View>) -> CallToolResult {
        let mut args = vec!["view".into()];
        flag(&mut args, "--context", request.context);
        flag(&mut args, "--shared", request.shared);
        args.extend(["--".into(), request.id.into()]);
        self.execute(args).await
    }

    #[tool(
        annotations(read_only_hint = true, destructive_hint = false),
        description = "Report index/model readiness. Set shared=true to include per-node last_synced_at and last_updated_at from PostgreSQL. Shared-only queries PostgreSQL without opening SQLite."
    )]
    async fn status(&self, Parameters(request): Parameters<Status>) -> CallToolResult {
        let mut args = vec!["status".into()];
        if request.shared {
            args.push("--shared".into());
        }
        self.execute(args).await
    }

    #[tool(
        annotations(read_only_hint = false, destructive_hint = false),
        description = "Index Claude Code and Codex histories into the configured local, shared, or both destinations, applying each index's provider allowlist. Shared-only uses no SQLite database. Never downloads models."
    )]
    async fn index(&self, Parameters(request): Parameters<Index>) -> CallToolResult {
        let mut args = vec!["index".into()];
        if request.full {
            args.push("--full".into());
        }
        if request.all_history {
            args.push("--all-history".into());
        }
        flag(&mut args, "--since-days", request.since_days);
        for provider in request.provider {
            flag(&mut args, "--provider", Some(provider));
        }
        self.execute(args).await
    }

    #[tool(
        annotations(read_only_hint = false, destructive_hint = true),
        description = "Forget a conversation owned by this node in its configured indexes. Shared-only deletes directly in PostgreSQL. Other nodes' copies are unaffected. Check shared.state for deferred deletion."
    )]
    async fn forget(&self, Parameters(request): Parameters<Forget>) -> CallToolResult {
        self.execute(vec![
            "forget".into(),
            "--".into(),
            request.conversation_id.into(),
        ])
        .await
    }

    #[tool(
        annotations(read_only_hint = false, destructive_hint = false),
        description = "Explicitly download and install embedding and reranking models. Requires network access and writes model assets."
    )]
    async fn models_install(&self) -> CallToolResult {
        self.execute(vec!["models".into(), "install".into()]).await
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
        info.instructions = Some("Search Claude Code and Codex history. Search/view/status are read-only. Index and forget write data; models_install explicitly downloads models. Provider allowlists and destinations are configured at startup. Shared-only mode defaults to shared search and this node's shared view; supply the result's node ID to view another node's message.".to_owned());
        info
    }
}

pub(crate) async fn run(
    arguments: Vec<OsString>,
    watch: bool,
    debounce_ms: u64,
) -> Result<(), crate::AppError> {
    let server = Mash {
        arguments: Arc::new(arguments),
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
            let mut args = background.arguments.as_ref().clone();
            args.push("index".into());
            let result = crate::watch::run(args, debounce_ms, shutdown, move || {
                let server = background.clone();
                async move {
                    let result = server.execute(vec!["index".into()]).await;
                    let json = result.structured_content.as_ref();
                    let retry = result.is_error == Some(true)
                        || json.is_some_and(|body| {
                            body["shared"]["state"] == "pending"
                                || body["malformed_records"].as_u64().is_some_and(|n| n > 0)
                        });
                    crate::write_json(
                        &mut std::io::stderr(),
                        &serde_json::json!({"event":"watch-index","result":result}),
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
    async fn sdk_handshake_discovery_calls_and_errors_share_cli_contract() {
        let dir = tempfile::tempdir().expect("temp");
        let config = dir.path().join("config.json");
        std::fs::write(&config, r#"{"version":1,"providers":{}}"#).expect("config");
        let server = Mash {
            arguments: Arc::new(vec![
                "mash".into(),
                "--config".into(),
                config.into_os_string(),
                "--db".into(),
                dir.path().join("missing.sqlite3").into_os_string(),
                "--models-dir".into(),
                dir.path().join("models").into_os_string(),
            ]),
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
