use crate::{
    RefreshMode,
    config::{Command, Config, Node},
};
use rmcp::{
    ClientHandler, ServiceExt,
    model::{CallToolRequestParams, CallToolResult, ProgressNotificationParam, RequestMetaObject},
    service::{NotificationContext, RoleClient},
    transport::TokioChildProcess,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;

#[derive(Serialize)]
struct IndexArguments {
    rebuild_local_gold: bool,
    rebuild_shared_gold: bool,
}

#[derive(Serialize)]
struct StatusArguments {
    shared: bool,
}

fn request(
    name: &'static str,
    arguments: &impl Serialize,
) -> Result<CallToolRequestParams, String> {
    let serde_json::Value::Object(arguments) =
        serde_json::to_value(arguments).map_err(|_| "cannot encode Mash request")?
    else {
        return Err("invalid Mash request arguments".into());
    };
    Ok(CallToolRequestParams::new(name).with_arguments(arguments))
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case", tag = "operation", content = "mode")]
pub enum Operation {
    Deploy,
    Refresh(RefreshMode),
    Check,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    Running,
    Succeeded,
    Failed,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Pending,
    Build,
    Install,
    Refresh,
    Check,
    Complete,
}

#[derive(Clone, Serialize)]
pub struct Outcome {
    phase: Phase,
    state: State,
    progress: Option<ProgressNotificationParam>,
    error: Option<String>,
    implementation: Option<rmcp::model::Implementation>,
    /// Opaque MCP SDK result, already redacted by Mash.
    results: Vec<CallToolResult>,
}

#[derive(Clone, Serialize)]
pub struct Job {
    pub id: u64,
    pub state: State,
    operation: Operation,
    logs_directory: std::path::PathBuf,
    nodes: BTreeMap<String, Outcome>,
}

#[derive(Default)]
struct Registry {
    next: u64,
    jobs: BTreeMap<u64, Job>,
}

#[derive(Clone)]
pub struct Runner {
    config: Arc<Config>,
    registry: Arc<Mutex<Registry>>,
}

impl Runner {
    pub fn new(mut config: Config) -> Self {
        // IDs are session-local; logs from separate sessions must not collide.
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        config.logs = config
            .logs
            .join(format!("{timestamp}-{}", std::process::id()));
        Self {
            config: Arc::new(config),
            registry: Arc::default(),
        }
    }

    pub async fn get(&self, id: u64) -> Result<Job, String> {
        self.registry
            .lock()
            .await
            .jobs
            .get(&id)
            .cloned()
            .ok_or_else(|| "unknown or expired job ID".into())
    }

    pub async fn start(&self, nodes: Vec<String>, operation: Operation) -> Result<Job, String> {
        if nodes.is_empty()
            || nodes.len() > 32
            || nodes.iter().collect::<BTreeSet<_>>().len() != nodes.len()
            || nodes.iter().any(|n| !self.config.nodes.contains_key(n))
        {
            return Err("select 1..32 distinct configured nodes".into());
        }
        for (position, name) in nodes.iter().enumerate() {
            let node = &self.config.nodes[name];
            if matches!(operation, Operation::Refresh(_)) && matches!(node, Node::Api { .. }) {
                return Err("refresh_index accepts history nodes only".into());
            }
            if matches!(operation, Operation::Deploy)
                && node
                    .requires()
                    .iter()
                    .any(|required| !nodes[..position].contains(required))
            {
                return Err(
                    "include each required API target before its history nodes in deploy".into(),
                );
            }
        }
        let mut registry = self.registry.lock().await;
        // Serialize fleet mutations and checks so a deployment cannot replace a
        // binary under a dev-initiated refresh; product mutations retain core locks.
        if registry.jobs.values().any(|j| j.state == State::Running) {
            return Err("a dev job is already running; poll it before starting another".into());
        }
        while registry.jobs.len() >= 32 {
            registry.jobs.pop_first();
        }
        registry.next += 1;
        let job = Job {
            id: registry.next,
            state: State::Running,
            operation,
            logs_directory: self.config.logs.join(registry.next.to_string()),
            nodes: nodes
                .iter()
                .map(|name| {
                    (
                        name.clone(),
                        Outcome {
                            phase: Phase::Pending,
                            state: State::Running,
                            progress: None,
                            error: None,
                            implementation: None,
                            results: vec![],
                        },
                    )
                })
                .collect(),
        };
        registry.jobs.insert(job.id, job.clone());
        let runner = self.clone();
        let id = job.id;
        tokio::spawn(async move {
            runner.perform(id, nodes, operation).await;
        });
        Ok(job)
    }

    async fn phase(&self, id: u64, node: &str, phase: Phase) {
        if let Some(outcome) = self
            .registry
            .lock()
            .await
            .jobs
            .get_mut(&id)
            .and_then(|j| j.nodes.get_mut(node))
        {
            outcome.phase = phase;
            if !matches!(phase, Phase::Check | Phase::Complete) {
                outcome.progress = None;
            }
        }
    }

    async fn perform(&self, id: u64, nodes: Vec<String>, operation: Operation) {
        let mut builds: BTreeMap<String, Result<(), String>> = BTreeMap::new();
        for name in nodes {
            let node = &self.config.nodes[&name];
            let dependency_failed = if matches!(operation, Operation::Deploy) {
                let registry = self.registry.lock().await;
                node.requires()
                    .iter()
                    .any(|required| registry.jobs[&id].nodes[required].state != State::Succeeded)
            } else {
                false
            };
            let outcome = if dependency_failed {
                Err("required API deployment failed; history target was not installed".into())
            } else {
                self.node(id, &name, node, operation, &mut builds).await
            };
            let mut registry = self.registry.lock().await;
            if let Some(result) = registry
                .jobs
                .get_mut(&id)
                .and_then(|j| j.nodes.get_mut(&name))
            {
                result.state = if outcome.is_ok() {
                    State::Succeeded
                } else {
                    State::Failed
                };
                result.error = outcome.err();
                if result.state == State::Succeeded {
                    result.phase = Phase::Complete;
                }
            }
        }
        if let Some(job) = self.registry.lock().await.jobs.get_mut(&id) {
            job.state = if job.nodes.values().all(|n| n.state == State::Succeeded) {
                State::Succeeded
            } else {
                State::Failed
            };
        }
    }

    async fn node(
        &self,
        id: u64,
        name: &str,
        node: &Node,
        operation: Operation,
        builds: &mut BTreeMap<String, Result<(), String>>,
    ) -> Result<(), String> {
        if matches!(operation, Operation::Deploy) {
            self.phase(id, name, Phase::Build).await;
            if !builds.contains_key(node.build()) {
                let mut result = Ok(());
                for (step, command) in self.config.builds[node.build()].iter().enumerate() {
                    result = self.command(id, name, "build", step, command).await;
                    if result.is_err() {
                        break;
                    }
                }
                builds.insert(node.build().to_owned(), result);
            }
            builds[node.build()].clone()?;
            self.phase(id, name, Phase::Install).await;
            for (step, command) in node.install().iter().enumerate() {
                self.command(id, name, "install", step, command).await?;
            }
        }
        if let Node::Api { verify, .. } = node {
            self.phase(id, name, Phase::Check).await;
            for (step, command) in verify.iter().enumerate() {
                self.command(id, name, "check", step, command).await?;
            }
            return Ok(());
        }
        for required in node.requires() {
            if let Node::Api { verify, .. } = &self.config.nodes[required] {
                for (step, command) in verify.iter().enumerate() {
                    self.command(id, name, "api-check", step, command).await?;
                }
            }
        }
        let Node::History { mcp, shared, .. } = node else {
            return Err("invalid history target".into());
        };
        let client = Progress {
            runner: self.clone(),
            id,
            node: name.into(),
        };
        let mut command = mcp.process(&self.config.repository);
        // Never expose credentials, build output or raw child stderr through MCP.
        command.stderr(std::process::Stdio::null());
        let transport =
            TokioChildProcess::new(command).map_err(|_| "cannot start configured Mash MCP")?;
        let service = tokio::time::timeout(Duration::from_secs(60), client.serve(transport))
            .await
            .map_err(|_| "Mash MCP connection timed out")?
            .map_err(|_| "Mash MCP initialization failed")?;
        if let Some(outcome) = self
            .registry
            .lock()
            .await
            .jobs
            .get_mut(&id)
            .and_then(|j| j.nodes.get_mut(name))
        {
            outcome.implementation = service
                .peer_info()
                .and_then(|info| info.server_info.clone());
        }
        let work = async {
            if let Operation::Refresh(mode) = operation {
                self.phase(id, name, Phase::Refresh).await;
                let arguments = IndexArguments {
                    rebuild_local_gold: matches!(mode, RefreshMode::LocalGold),
                    rebuild_shared_gold: matches!(mode, RefreshMode::SharedGold),
                };
                let mut request = request("index", &arguments)?;
                let mut meta = RequestMetaObject::new();
                meta.insert(
                    "progressToken".into(),
                    serde_json::json!(format!("{id}:{name}")),
                );
                request.meta = Some(meta);
                let result = service
                    .call_tool(request)
                    .await
                    .map_err(|_| "Mash index request failed")?;
                self.record(id, name, result).await?;
            }
            self.phase(id, name, Phase::Check).await;
            let result = service
                .call_tool(request("status", &StatusArguments { shared: *shared })?)
                .await
                .map_err(|_| "Mash readiness request failed")?;
            self.record(id, name, result).await
        };
        let result = tokio::time::timeout(Duration::from_secs(self.config.timeout_seconds), work).await
            .map_err(|_| "Mash request timed out; index may be partially committed; check readiness before retry".to_owned()).and_then(std::convert::identity);
        let _ = service.cancel().await;
        result
    }

    async fn record(&self, id: u64, name: &str, result: CallToolResult) -> Result<(), String> {
        let failed = result.is_error == Some(true);
        if let Some(node) = self
            .registry
            .lock()
            .await
            .jobs
            .get_mut(&id)
            .and_then(|j| j.nodes.get_mut(name))
        {
            node.results.push(result);
        }
        if failed {
            Err("Mash returned a tool error; inspect results".into())
        } else {
            Ok(())
        }
    }

    async fn command(
        &self,
        id: u64,
        name: &str,
        phase: &str,
        step: usize,
        command: &Command,
    ) -> Result<(), String> {
        let directory = self.config.logs.join(id.to_string());
        std::fs::create_dir_all(&directory).map_err(|_| "cannot create private job logs")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| "cannot secure job logs")?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let log = options
            .open(directory.join(format!("{name}-{phase}-{step}.log")))
            .map_err(|_| "cannot open job log")?;
        let stderr = log.try_clone().map_err(|_| "cannot open job log")?;
        let mut command = command.process(&self.config.repository);
        command
            .stdin(std::process::Stdio::null())
            .stdout(log)
            .stderr(stderr);
        let status = tokio::time::timeout(
            Duration::from_secs(self.config.timeout_seconds),
            command.status(),
        )
        .await
        .map_err(|_| "workflow command timed out; inspect private logs and target before retry")?
        .map_err(|_| "cannot start workflow command; inspect private logs")?;
        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "{phase} step {step} failed; inspect private job logs"
            ))
        }
    }
}

struct Progress {
    runner: Runner,
    id: u64,
    node: String,
}

#[cfg(test)]
mod tests;

impl ClientHandler for Progress {
    async fn on_progress(
        &self,
        params: ProgressNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) {
        if let Some(outcome) = self
            .runner
            .registry
            .lock()
            .await
            .jobs
            .get_mut(&self.id)
            .and_then(|j| j.nodes.get_mut(&self.node))
        {
            outcome.progress = Some(params);
        }
    }
}
