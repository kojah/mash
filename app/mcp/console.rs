//! The dashboard's console: the MCP tools as an agent sees them, over an
//! in-process connection, limited to the ones that only read.
use super::Mash;
use crate::AppError;
use crate::core::Paths;
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, Tool};
use std::sync::Arc;
use std::time::Duration;

const DEADLINE: Duration = Duration::from_secs(120);

#[derive(Clone)]
pub struct Console {
    server: Mash,
}

/// What the server publishes about itself: the same listing an agent reads.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct Catalogue {
    build_revision: &'static str,
    contract_fingerprint: String,
    tools: Vec<Tool>,
}

/// A tool's answer as an agent receives it: the structured reply and the text
/// beside it. `failed` covers tool errors and rejected arguments alike.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct Answer {
    failed: bool,
    reply: Option<serde_json::Value>,
    text: String,
}

impl Answer {
    /// Whether the tool failed or turned its arguments down.
    pub fn failed(&self) -> bool {
        self.failed
    }

    /// The structured reply, when the tool gave one.
    pub fn reply(&self) -> Option<&serde_json::Value> {
        self.reply.as_ref()
    }
}

fn rejected(kind: &str, message: &str) -> Answer {
    Answer {
        failed: true,
        reply: Some(
            serde_json::json!({"error": {"kind": kind, "message": message, "retryable": false}}),
        ),
        text: message.to_owned(),
    }
}

impl Console {
    pub fn new(paths: Paths) -> Self {
        Self {
            server: Mash {
                paths: Arc::new(paths),
                gate: Arc::new(tokio::sync::Mutex::new(())),
                model_work: Arc::new(tokio::sync::Semaphore::new(1)),
                // The dashboard is not an agent session and holds no runtime lease.
                runtime: None,
            },
        }
    }

    pub fn catalogue() -> Result<Catalogue, AppError> {
        let mut tools = Mash::tools().list_all();
        tools.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(Catalogue {
            build_revision: env!("MASH_BUILD_REVISION"),
            contract_fingerprint: super::guidance::fingerprint()
                .map_err(|_| AppError::internal("fingerprint the tool contract"))?,
            tools,
        })
    }

    /// Calls one tool exactly as a connected agent would. Tools not marked
    /// read-only are refused here, whatever the caller sends.
    pub async fn call(
        &self,
        name: &str,
        arguments: serde_json::Map<String, serde_json::Value>,
    ) -> Result<Answer, AppError> {
        let Some(tool) = Mash::tools()
            .list_all()
            .into_iter()
            .find(|tool| tool.name == name)
        else {
            return Ok(rejected("unknown-tool", "Mash has no tool by that name."));
        };
        if tool
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.read_only_hint)
            != Some(true)
        {
            return Ok(rejected(
                "not-read-only",
                "This tool changes data, so it can only be run by an agent or from the command line.",
            ));
        }
        let (server_io, client_io) = tokio::io::duplex(64 * 1024);
        let server = self.server.clone();
        let serving = tokio::spawn(async move { server.serve(server_io).await });
        let client = ()
            .serve(client_io)
            .await
            .map_err(|_| AppError::internal("open the console connection"))?;
        let request = CallToolRequestParams::new(name.to_owned()).with_arguments(arguments);
        let outcome = tokio::time::timeout(DEADLINE, client.call_tool(request)).await;
        let _ = client.cancel().await;
        if let Ok(Ok(service)) = serving.await {
            let _ = service.cancel().await;
        }
        Ok(match outcome {
            Err(_) => rejected("timeout", "The tool did not answer in time."),
            Ok(Err(rmcp::ServiceError::McpError(error))) => {
                rejected("invalid-arguments", &error.message)
            }
            Ok(Err(_)) => return Err(AppError::internal("the console connection failed")),
            Ok(Ok(result)) => {
                let text = result
                    .content
                    .iter()
                    .filter_map(|block| block.as_text().map(|text| text.text.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n");
                let failed = result.is_error == Some(true);
                match result.structured_content {
                    // A refusal that carries only prose, such as arguments the
                    // tool's schema rejects, is given the same error shape.
                    None if failed => rejected("rejected", &text),
                    reply => Answer {
                        failed,
                        reply,
                        text,
                    },
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn console(directory: &std::path::Path) -> Console {
        let config = directory.join("config.toml");
        std::fs::write(&config, "version = 1\nproviders = {}\n").unwrap();
        Console::new(Paths {
            config: Some(config),
            db: Some(directory.join("missing.sqlite3")),
            models_dir: Some(directory.join("models")),
        })
    }

    #[test]
    fn console_catalogue_is_the_agent_tool_listing_with_its_read_only_marks() {
        let catalogue = Console::catalogue().unwrap();
        let read_only = |name: &str| {
            catalogue
                .tools
                .iter()
                .find(|tool| tool.name == name)
                .and_then(|tool| tool.annotations.as_ref())
                .and_then(|annotations| annotations.read_only_hint)
        };
        assert_eq!(read_only("search"), Some(true));
        assert_eq!(read_only("index"), Some(false));
        assert_eq!(catalogue.contract_fingerprint.len(), 64);
        let names: Vec<_> = catalogue.tools.iter().map(|tool| &tool.name).collect();
        assert!(names.is_sorted());
    }

    #[tokio::test]
    async fn console_runs_read_only_tools_and_refuses_the_rest() {
        let directory = tempfile::tempdir().unwrap();
        let console = console(directory.path());
        let json = |reply: &Answer| serde_json::to_value(reply).unwrap();

        // A tool that changes data is refused before any connection is made.
        let refused = json(&console.call("index", serde_json::Map::new()).await.unwrap());
        assert_eq!(refused["failed"], true);
        assert_eq!(refused["reply"]["error"]["kind"], "not-read-only");
        assert!(!directory.path().join("missing.sqlite3").exists());

        let unknown = json(&console.call("nope", serde_json::Map::new()).await.unwrap());
        assert_eq!(unknown["reply"]["error"]["kind"], "unknown-tool");

        // Arguments the tool's schema rejects come back as a failure, not a crash.
        let invalid = json(&console.call("view", serde_json::Map::new()).await.unwrap());
        assert_eq!(invalid["failed"], true);
        assert_ne!(invalid["reply"]["error"]["message"], "");
        assert!(invalid["reply"]["error"]["kind"].is_string());

        // A read-only tool answers over the same path an agent uses.
        let help = json(&console.call("help", serde_json::Map::new()).await.unwrap());
        assert_eq!(help["failed"], false, "{help}");
        assert!(help["reply"].is_object());
        assert_ne!(help["text"].as_str().unwrap(), "");
    }
}
