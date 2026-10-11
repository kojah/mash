mod config;
mod jobs;
mod report;

use rmcp::{
    ServiceExt, handler::server::wrapper::Parameters, model::CallToolResult, schemars, tool,
    tool_router,
};
use serde::Deserialize;

#[derive(Clone)]
struct Server(jobs::Runner);

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Targets {
    /// Explicit configured target names. No implicit fleet-wide deployment.
    nodes: Vec<String>,
}

#[derive(Default, Clone, Copy, Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum RefreshMode {
    /// Ingest provider changes and apply supported local schema migrations.
    #[default]
    Sync,
    /// Rebuild local search data from certified silver.
    LocalViews,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Refresh {
    nodes: Vec<String>,
    #[serde(default)]
    mode: RefreshMode,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Status {
    /// Poll an existing job. Omit to start a fresh readiness check for nodes.
    job_id: Option<u64>,
    #[serde(default)]
    nodes: Vec<String>,
}

fn result(value: Result<jobs::Job, String>) -> CallToolResult {
    match value {
        Ok(value) => match serde_json::to_value(value) {
            Ok(value) => CallToolResult::structured(value),
            Err(_) => {
                CallToolResult::error(vec![rmcp::model::ContentBlock::text("cannot encode job")])
            }
        },
        Err(message) => CallToolResult::error(vec![rmcp::model::ContentBlock::text(message)]),
    }
}

#[tool_router]
impl Server {
    #[tool(
        description = "Run the registered Ansible deploy playbook on explicit targets. Ansible owns build reuse, API-first ordering, offline-host skips, installs and live contract/readiness checks. Include required API targets with their history nodes. Returns a Runner job ID promptly; poll deployment_status. Never indexes or downloads models as a deployment side effect. Existing Mash clients must reconnect after binary replacement.",
        annotations(read_only_hint = false, destructive_hint = false)
    )]
    async fn deploy(&self, Parameters(input): Parameters<Targets>) -> CallToolResult {
        result(self.0.start(input.nodes, jobs::Operation::Deploy).await)
    }

    #[tool(
        description = "Run the registered Ansible refresh playbook on explicit history nodes. Its RMCP probe calls the installed Mash index tool: sync ingests providers; local-views rebuilds retrieval from certified silver. Never deploys binaries or downloads models. Returns a Runner job ID; deployment_status reports task/index progress and redacted results. Ansible serializes node refreshes.",
        annotations(read_only_hint = false, destructive_hint = false)
    )]
    async fn refresh_index(&self, Parameters(input): Parameters<Refresh>) -> CallToolResult {
        result(
            self.0
                .start(input.nodes, jobs::Operation::Refresh(input.mode))
                .await,
        )
    }

    #[tool(
        description = "Poll Runner task progress, per-target outcomes and redacted Mash probe results. With no job_id, supply explicit history nodes to run the registered read-only check playbook. Job history is bounded and session-local; raw Ansible logs stay private. Skipped targets have not been deployed or certified ready.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    async fn deployment_status(&self, Parameters(input): Parameters<Status>) -> CallToolResult {
        result(match input.job_id {
            Some(id) if input.nodes.is_empty() => self.0.get(id).await,
            Some(_) => Err("choose job_id or nodes, not both".into()),
            None => self.0.start(input.nodes, jobs::Operation::Check).await,
        })
    }
}

#[allow(clippy::unused_async_trait_impl)]
#[rmcp::tool_handler(router = Self::tool_router())]
impl rmcp::ServerHandler for Server {
    fn get_info(&self) -> rmcp::model::ServerConfig {
        let mut info = rmcp::model::ServerConfig::default();
        info.capabilities = rmcp::model::ServerCapabilities::builder()
            .enable_tools()
            .build();
        info.server_info = rmcp::model::Implementation::new("mash_dev", env!("CARGO_PKG_VERSION"));
        info.instructions = Some("Thin agent interface to registered Ansible playbooks. Select explicit target names and poll Runner jobs until terminal. Inspect skipped/failed target states before claiming readiness. Refresh is explicit. Inventory and runner configuration are trusted operator input; tool calls never accept commands or playbook paths.".into());
        info
    }
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("mash_dev: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let path = if args.is_empty() {
        std::env::var_os("MASH_DEV_CONFIG")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(|home| std::path::PathBuf::from(home).join(".config/mash/dev.toml"))
            })
            .ok_or("set MASH_DEV_CONFIG or supply --config PATH")?
    } else if args.len() == 2 && args[0] == "--config" {
        std::path::PathBuf::from(&args[1])
    } else {
        return Err("usage: mash-dev-mcp [--config PATH]".into());
    };
    let config = config::load(&path)?;
    let server = Server(jobs::Runner::new(config));
    let service = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|_| "MCP initialization failed")?;
    service.waiting().await.map_err(|_| "MCP session failed")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn official_sdk_exposes_only_dev_tools_and_rejects_unknown_targets() {
        let directory = tempfile::tempdir().unwrap();
        let inventory = directory.path().join("inventory.yml");
        std::fs::write(&inventory, "all: {}\n").unwrap();
        std::fs::write(directory.path().join("playbook.yml"), "[]\n").unwrap();
        let config = config::Config {
            repository: directory.path().into(),
            logs: directory.path().join("logs"),
            inventory,
            timeout_seconds: 30,
            runner: config::Command {
                argv: vec!["missing-runner".into()],
            },
            nodes: vec!["test".into()],
            playbooks: config::Playbooks {
                deploy: "playbook.yml".into(),
                refresh: "playbook.yml".into(),
                check: "playbook.yml".into(),
            },
        };
        let (server_io, client_io) = tokio::io::duplex(65536);
        let serving = tokio::spawn(async move {
            Server(jobs::Runner::new(config))
                .serve(server_io)
                .await
                .unwrap()
        });
        let client = ().serve(client_io).await.unwrap();
        let server = serving.await.unwrap();
        let tools = client.list_all_tools().await.unwrap();
        let mut names: Vec<_> = tools.iter().map(|t| t.name.as_ref()).collect();
        names.sort_unstable();
        assert_eq!(names, ["deploy", "deployment_status", "refresh_index"]);
        let result = client
            .call_tool(
                rmcp::model::CallToolRequestParams::new("deploy").with_arguments(
                    serde_json::json!({"nodes":["unknown"]})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(true));
        assert!(!directory.path().join("logs").exists());
        client.cancel().await.unwrap();
        server.cancel().await.unwrap();
    }
}
