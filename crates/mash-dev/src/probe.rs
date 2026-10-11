//! Ansible invokes this concrete RMCP client for read-only checks or explicit refresh.
mod readiness;
mod report;

use clap::{Parser, ValueEnum};
use report::Report;
use rmcp::{
    ClientHandler, ServiceExt,
    model::{CallToolRequestParams, CallToolResult, ProgressNotificationParam, RequestMetaObject},
    service::{NotificationContext, RoleClient},
    transport::TokioChildProcess,
};
use serde::Serialize;
use std::{io::Write, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

#[derive(Parser)]
struct Arguments {
    #[arg(long)]
    repository: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long)]
    progress: PathBuf,
    #[arg(long, value_enum)]
    mode: Mode,
    #[arg(long)]
    shared: bool,
    /// Trusted inventory's MCP command, executed directly without a shell.
    #[arg(last = true, required = true, num_args = 1..)]
    command: Vec<std::ffi::OsString>,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Mode {
    Check,
    Sync,
    LocalViews,
}

#[derive(Serialize)]
struct Help<'a> {
    tool: &'static str,
    expected_contract: &'a str,
}
#[derive(Serialize)]
struct Status {
    shared: bool,
    detail: Detail,
}
#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum Detail {
    Extended,
}
#[derive(Serialize)]
struct Index {
    rebuild_local_views: bool,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Intent {
    ExactMention,
    RelatedDiscussion,
}
#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum Unit {
    Messages,
}
#[derive(Serialize)]
struct Search {
    query: &'static str,
    intent: Intent,
    unit: Unit,
    limit: usize,
    chat: [&'static str; 1],
    shared: bool,
    local: bool,
}

fn request(name: &'static str, input: &impl Serialize) -> Result<CallToolRequestParams, String> {
    let serde_json::Value::Object(arguments) =
        serde_json::to_value(input).map_err(|_| "cannot encode Mash request")?
    else {
        return Err("invalid request arguments".into());
    };
    Ok(CallToolRequestParams::new(name).with_arguments(arguments))
}

struct Progress {
    output: PathBuf,
    report: Arc<Mutex<report::Report>>,
}

impl ClientHandler for Progress {
    async fn on_progress(
        &self,
        progress: ProgressNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) {
        let mut report = self.report.lock().await;
        report.progress = Some(progress);
        let _ = write_report(&self.output, &report);
    }
}

async fn record(report: &Arc<Mutex<report::Report>>, result: CallToolResult) -> Result<(), String> {
    let failed = result.is_error == Some(true);
    report.lock().await.results.push(result);
    if failed {
        Err("Mash returned a tool error; inspect results".into())
    } else {
        Ok(())
    }
}

async fn check(arguments: &Arguments, report: Arc<Mutex<report::Report>>) -> Result<(), String> {
    let expected = readiness::expected(&arguments.repository).await?;
    let mut command = tokio::process::Command::new(&arguments.command[0]);
    command
        .args(&arguments.command[1..])
        .current_dir(&arguments.repository)
        .kill_on_drop(true)
        .stderr(std::process::Stdio::null());
    let client = Progress {
        output: arguments.progress.clone(),
        report: report.clone(),
    };
    let transport =
        TokioChildProcess::new(command).map_err(|_| "cannot start inventory Mash MCP")?;
    let service = tokio::time::timeout(std::time::Duration::from_secs(60), client.serve(transport))
        .await
        .map_err(|_| "Mash MCP initialization timed out")?
        .map_err(|_| "Mash MCP initialization failed")?;
    let result = run(arguments, &report, &expected, &service).await;
    let _ = service.cancel().await;
    result
}

async fn run(
    arguments: &Arguments,
    report: &Arc<Mutex<report::Report>>,
    expected: &readiness::Stamp,
    service: &rmcp::service::Peer<RoleClient>,
) -> Result<(), String> {
    let tools = service
        .list_all_tools()
        .await
        .map_err(|_| "cannot discover installed Mash tools")?;
    for name in ["help", "list_chats", "search", "view", "status"] {
        if !tools.iter().any(|tool| tool.name == name) {
            return Err(format!(
                "installed MCP lacks {name}; reconnect to the matching release"
            ));
        }
    }
    let result = service
        .call_tool(request(
            "help",
            &Help {
                tool: "search",
                expected_contract: &expected.contract_fingerprint,
            },
        )?)
        .await
        .map_err(|_| "Mash live contract request failed")?;
    let check = readiness::help(&result, expected);
    record(report, result).await?;
    check?;
    if arguments.mode != Mode::Check {
        let mut request = request(
            "index",
            &Index {
                rebuild_local_views: arguments.mode == Mode::LocalViews,
            },
        )?;
        let mut meta = RequestMetaObject::new();
        meta.insert("progressToken".into(), serde_json::json!("ansible-refresh"));
        request.meta = Some(meta);
        let result = service
            .call_tool(request)
            .await
            .map_err(|_| "Mash index request failed")?;
        record(report, result).await?;
    }
    let shared = arguments.shared && arguments.mode != Mode::LocalViews;
    let result = service
        .call_tool(request(
            "status",
            &Status {
                shared,
                detail: Detail::Extended,
            },
        )?)
        .await
        .map_err(|_| "Mash readiness request failed")?;
    let status = readiness::status(&result, shared);
    record(report, result).await?;
    let status = status?;
    let mut intents = vec![Intent::ExactMention];
    if status.query_models == readiness::Models::Available && arguments.mode != Mode::LocalViews {
        intents.push(Intent::RelatedDiscussion);
    }
    for intent in intents {
        let input = Search {
            query: "deployment health check",
            intent,
            unit: Unit::Messages,
            limit: 1,
            chat: ["mash-deployment-smoke-no-such-chat"],
            shared,
            local: !shared,
        };
        let result = service
            .call_tool(request("search", &input)?)
            .await
            .map_err(|_| "Mash smoke search request failed")?;
        let check = readiness::search(&result, shared);
        record(report, result).await?;
        check?;
    }
    Ok(())
}

fn write_report(path: &std::path::Path, report: &Report) -> Result<(), String> {
    let bytes = serde_json::to_vec(report).map_err(|_| "cannot encode probe report")?;
    let incoming = path.with_extension(format!("incoming.{}", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(&incoming)
        .and_then(|mut file| file.write_all(&bytes))
        .map_err(|_| "cannot write private probe report")?;
    std::fs::rename(incoming, path).map_err(|_| "cannot publish probe report".into())
}

#[tokio::main]
async fn main() {
    let arguments = Arguments::parse();
    let report = Arc::new(Mutex::new(report::Report::default()));
    let outcome = check(&arguments, report.clone()).await;
    let mut report = report.lock().await;
    report.state = if outcome.is_ok() {
        report::State::Succeeded
    } else {
        report::State::Failed
    };
    report.error = outcome.err();
    if let Err(error) = write_report(&arguments.output, &report) {
        eprintln!("{error}");
        std::process::exit(1);
    }
    if report.state == report::State::Failed {
        eprintln!("Mash probe failed; inspect its private report");
        std::process::exit(1);
    }
}
