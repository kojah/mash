use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use rmcp::{
    ServiceExt,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    schemars, tool, tool_router,
};
use serde::Deserialize;

#[derive(Parser)]
#[command(
    name = "mash-mcp",
    version,
    about = "Multi-Agent Search History MCP server"
)]
struct Options {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    db: Option<PathBuf>,
    #[arg(long)]
    models_dir: Option<PathBuf>,
}

#[derive(Clone)]
struct Mash {
    arguments: Arc<Vec<OsString>>,
    // Serialize expensive model loading and mutations for this stdio session.
    gate: Arc<tokio::sync::Mutex<()>>,
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
        let _guard = self.gate.lock().await;
        let mut all = self.arguments.as_ref().clone();
        all.extend(arguments);
        match tokio::task::spawn_blocking(move || crate::cli::run(all)).await {
            Ok(Ok(value)) => match serde_json::to_value(&value) {
                Ok(json) => CallToolResult::structured(json),
                Err(_) => CallToolResult::error(vec![ContentBlock::text(
                    "failed to serialize tool response",
                )]),
            },
            Ok(Err(error)) => CallToolResult::structured_error(
                serde_json::to_value(&error).unwrap_or_else(|_| serde_json::json!({"error":{"kind":"internal","message":"tool failed","retryable":false}})),
            ),
            Err(_) => CallToolResult::error(vec![ContentBlock::text("tool worker failed")]),
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
        description = "Report local index/model readiness and pending publication. Does not connect to PostgreSQL."
    )]
    async fn status(&self) -> CallToolResult {
        self.execute(vec!["status".into()]).await
    }

    #[tool(
        annotations(read_only_hint = false, destructive_hint = false),
        description = "Index local Claude Code and Codex histories, then publish to the configured node group. Never downloads models. Writes local and shared data."
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
        description = "Forget a locally owned conversation and publish its deletion. Other nodes' copies are unaffected. Check shared.state for deferred deletion."
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
        info.instructions = Some("Search Claude Code and Codex history. Search/view/status are read-only. Index and forget write data; models_install explicitly downloads models. Shared search addresses the configured node group.".to_owned());
        info
    }
}

pub(crate) async fn run() -> i32 {
    let options = match Options::try_parse() {
        Ok(options) => options,
        Err(error) => {
            let _ = error.print();
            return error.exit_code();
        }
    };
    let mut arguments = vec![OsString::from("mash")];
    for (name, value) in [
        ("--config", options.config),
        ("--db", options.db),
        ("--models-dir", options.models_dir),
    ] {
        if let Some(path) = value {
            arguments.extend([name.into(), path.into_os_string()]);
        }
    }
    let server = Mash {
        arguments: Arc::new(arguments),
        gate: Arc::new(tokio::sync::Mutex::new(())),
    };
    let Ok(service) = server.serve(rmcp::transport::stdio()).await else {
        eprintln!("MCP initialization failed");
        return 1;
    };
    if service.waiting().await.is_ok() {
        0
    } else {
        eprintln!("MCP session failed");
        1
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
        };
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
        let status = client
            .call_tool(CallToolRequestParams::new("status"))
            .await
            .expect("status");
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
