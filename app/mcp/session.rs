//! A process-local MCP connection survives compatible worker replacements.
//! Only startup paths become child arguments; tool calls stay typed MCP requests.
use super::{Paths, guidance};
use crate::AppError;
use rmcp::{
    ClientHandler, ServerHandler, ServiceExt,
    model::{
        CallToolRequest, CallToolRequestParams, CallToolResponse, CallToolResult, ClientRequest,
        ContentBlock, ListToolsResult, PaginatedRequestParams, ProgressNotificationParam,
        ProgressToken, ServerConfig, ServerResult, Tool,
    },
    service::{
        NotificationContext, Peer, PeerRequestOptions, RequestContext, RoleClient, RoleServer,
        RunningService,
    },
    transport::TokioChildProcess,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{Mutex, RwLock};

type Routes = Arc<Mutex<HashMap<ProgressToken, (Peer<RoleServer>, ProgressToken)>>>;

#[derive(Clone, Default)]
struct Forward(Routes);

impl ClientHandler for Forward {
    async fn on_progress(
        &self,
        mut progress: ProgressNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) {
        let route = self.0.lock().await.get(&progress.progress_token).cloned();
        if let Some((peer, token)) = route {
            progress.progress_token = token;
            let _ = peer.notify_progress(progress).await;
        }
    }
}

#[derive(Deserialize, PartialEq, Eq)]
struct Schemas {
    silver_schema: i64,
    views_schema: i64,
}

#[derive(Deserialize)]
struct Runtime {
    #[serde(flatten)]
    schemas: Schemas,
    executable_state: ExecutableState,
    reconnect_required: bool,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ExecutableState {
    Current,
    Replaced,
    Unknown,
}

#[derive(Deserialize)]
struct Help {
    runtime: Runtime,
}

struct Worker {
    pid: Option<u32>,
    service: RunningService<RoleClient, Forward>,
    forward: Forward,
    stamp: guidance::FileStamp,
    tools: Vec<Tool>,
    schemas: Schemas,
}

async fn installed(path: &std::path::Path) -> Option<guidance::FileStamp> {
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || guidance::stamp(&path).ok())
        .await
        .ok()
        .flatten()
}

impl Worker {
    fn closed(&self) -> bool {
        self.service.is_closed() || self.service.is_transport_closed()
    }

    async fn start(binary: &std::path::Path, paths: &Paths) -> Result<Self, &'static str> {
        let stamp = installed(binary)
            .await
            .ok_or("Installed worker cannot be verified.")?;
        let mut command = tokio::process::Command::new(binary);
        for (flag, value) in [
            ("--config", &paths.config),
            ("--db", &paths.db),
            ("--models-dir", &paths.models_dir),
        ] {
            if let Some(value) = value {
                command.arg(flag).arg(value);
            }
        }
        command.args(["mcp", "--worker"]).kill_on_drop(true);
        let transport =
            TokioChildProcess::new(command).map_err(|_| "Installed worker could not start.")?;
        let pid = transport.id();
        let forward = Forward::default();
        let service =
            tokio::time::timeout(Duration::from_secs(15), forward.clone().serve(transport))
                .await
                .map_err(|_| "Installed worker initialization timed out.")?
                .map_err(|_| "Installed worker initialization failed.")?;
        let (tools, help) = tokio::time::timeout(Duration::from_secs(15), async {
            let tools = service
                .list_all_tools()
                .await
                .map_err(|_| "Installed tools could not be read.")?;
            let help = service
                .call_tool(CallToolRequestParams::new("help"))
                .await
                .map_err(|_| "Installed worker help failed.")?;
            if help.is_error == Some(true) {
                return Err("Installed worker rejected its help check.");
            }
            let help: Help = serde_json::from_value(
                help.structured_content
                    .ok_or("Installed worker has no runtime metadata.")?,
            )
            .map_err(|_| "Installed worker runtime metadata is incompatible.")?;
            Ok::<_, &'static str>((tools, help))
        })
        .await
        .map_err(|_| "Installed worker contract check timed out.")??;
        if help.runtime.executable_state != ExecutableState::Current
            || help.runtime.reconnect_required
            || installed(binary).await.as_ref() != Some(&stamp)
        {
            return Err("Installed executable changed during worker initialization.");
        }
        Ok(Self {
            pid,
            service,
            forward,
            stamp,
            tools,
            schemas: help.runtime.schemas,
        })
    }

    fn compatible(&self, candidate: &Self) -> bool {
        self.schemas == candidate.schemas
            && interface(&self.tools) == interface(&candidate.tools)
            && self
                .service
                .peer_info()
                .map(|info| info.capabilities.clone())
                == candidate
                    .service
                    .peer_info()
                    .map(|info| info.capabilities.clone())
    }

    async fn watch(&self) -> Result<Watch, &'static str> {
        let result = self
            .service
            .call_tool(CallToolRequestParams::new("watch_status"))
            .await
            .map_err(|_| "Worker watcher status failed.")?;
        if result.is_error == Some(true) {
            return Err("Worker watcher status was rejected.");
        }
        serde_json::from_value(
            result
                .structured_content
                .ok_or("Worker watcher status is missing.")?,
        )
        .map_err(|_| "Worker watcher status is incompatible.")
    }

    async fn stop_watch(&self) -> Result<(), &'static str> {
        let result = self
            .service
            .call_tool(CallToolRequestParams::new("watch_stop"))
            .await
            .map_err(|_| "Worker watcher stop failed.")?;
        if result.is_error == Some(true) {
            return Err("Worker watcher stop was rejected.");
        }
        while self.watch().await?.state != WatchState::Stopped {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Ok(())
    }

    async fn start_watch(&self, debounce_ms: u64) -> Result<(), &'static str> {
        let result = self
            .service
            .call_tool(
                CallToolRequestParams::new("watch_start").with_arguments(
                    serde_json::json!({"debounce_ms": debounce_ms})
                        .as_object()
                        .ok_or("Invalid watcher arguments.")?
                        .clone(),
                ),
            )
            .await
            .map_err(|_| "Worker watcher start failed.")?;
        if result.is_error == Some(true) {
            Err("Worker watcher start was rejected.")
        } else {
            Ok(())
        }
    }
}

fn interface(tools: &[Tool]) -> Vec<Tool> {
    let mut tools = tools.to_vec();
    for tool in &mut tools {
        tool.description = None;
        tool.title = None;
        tool.icons = None;
        tool.meta = None;
    }
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    tools
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum WatchState {
    Running,
    Stopping,
    Stopped,
}

#[derive(Deserialize, Serialize)]
struct Watch {
    state: WatchState,
    debounce_ms: Option<u64>,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum UpdateState {
    Current,
    Draining,
    Rejected,
}

#[derive(Debug)]
enum UpdateError {
    Incompatible,
    Worker(&'static str),
}

impl UpdateError {
    fn message(&self) -> &'static str {
        match self {
            Self::Incompatible => {
                "Installed tools or storage formats changed; current worker retained. Reconnect to use the new contract after completing required migrations."
            }
            Self::Worker(message) => message,
        }
    }
}

#[derive(Serialize)]
struct Update {
    state: UpdateState,
    generation: u64,
    front_build_revision: &'static str,
    error: Option<&'static str>,
    reconnect_required: bool,
    #[serde(skip)]
    attempted: Option<guidance::FileStamp>,
}

#[derive(Serialize)]
struct SessionInfo<'a> {
    #[serde(flatten)]
    update: &'a Update,
    front_process_id: u32,
    worker_process_id: Option<u32>,
}

#[derive(Clone)]
struct Session {
    binary: PathBuf,
    paths: Paths,
    worker: Arc<RwLock<Worker>>,
    update: Arc<Mutex<Update>>,
    upgrade: Arc<Mutex<()>>,
    info: ServerConfig,
}

impl Session {
    async fn unchanged(&self, stamp: &guidance::FileStamp) -> bool {
        let worker = self.worker.read().await;
        let unchanged = worker.stamp == *stamp && !worker.closed();
        if unchanged {
            let mut update = self.update.lock().await;
            update.state = UpdateState::Current;
            update.error = None;
            update.reconnect_required = false;
            update.attempted = None;
        }
        unchanged
    }

    async fn new(binary: PathBuf, paths: Paths) -> Result<Self, &'static str> {
        let worker = Worker::start(&binary, &paths).await?;
        let peer_info = worker
            .service
            .peer_info()
            .ok_or("Worker did not initialize.")?
            .as_ref()
            .clone();
        let mut info = ServerConfig::new(peer_info.capabilities)
            .with_protocol_version(peer_info.protocol_version)
            .with_server_info(peer_info.server_info.ok_or("Worker identity is missing.")?);
        info.instructions = peer_info.instructions;
        info.instructions.get_or_insert_default().push_str(" This connection automatically replaces compatible Mash workers after active calls finish. Use live help for current worker instructions and build identity; the handshake describes the initial worker. Incompatible tools or storage formats require reconnecting.");
        Ok(Self {
            binary,
            paths,
            worker: Arc::new(RwLock::new(worker)),
            update: Arc::new(Mutex::new(Update {
                state: UpdateState::Current,
                generation: 1,
                front_build_revision: env!("MASH_BUILD_REVISION"),
                error: None,
                reconnect_required: false,
                attempted: None,
            })),
            upgrade: Arc::default(),
            info,
        })
    }

    async fn refresh(&self) {
        let Some(stamp) = installed(&self.binary).await else {
            let mut update = self.update.lock().await;
            update.state = UpdateState::Rejected;
            update.error = Some(
                "Installed executable is unavailable; current worker retained. Repair the deployment.",
            );
            update.reconnect_required = false;
            return;
        };
        if self.unchanged(&stamp).await {
            return;
        }
        let _upgrade = self.upgrade.lock().await;
        if self.unchanged(&stamp).await {
            return;
        }
        if self.update.lock().await.attempted.as_ref() == Some(&stamp) {
            return;
        }
        self.update.lock().await.attempted = Some(stamp);
        let result = self.replace().await;
        let mut update = self.update.lock().await;
        match result {
            Ok(()) => {
                // The generation changes while the exclusive worker lease is held.
            }
            Err(error) => {
                update.state = UpdateState::Rejected;
                update.error = Some(error.message());
                update.reconnect_required = matches!(error, UpdateError::Incompatible);
            }
        }
    }

    async fn replace(&self) -> Result<(), UpdateError> {
        let candidate = Worker::start(&self.binary, &self.paths)
            .await
            .map_err(UpdateError::Worker)?;
        self.handover(candidate).await
    }

    async fn handover(&self, mut candidate: Worker) -> Result<(), UpdateError> {
        if !self.worker.read().await.compatible(&candidate) {
            return Err(UpdateError::Incompatible);
        }
        self.update.lock().await.state = UpdateState::Draining;
        let mut current = self.worker.write().await;
        if installed(&self.binary).await.as_ref() != Some(&candidate.stamp) {
            return Err(UpdateError::Worker(
                "Installed executable changed before worker switch.",
            ));
        }
        let watch = if current.closed() {
            None
        } else {
            Some(current.watch().await.map_err(UpdateError::Worker)?)
        };
        if let Some(watch) = &watch {
            current.stop_watch().await.map_err(UpdateError::Worker)?;
            if watch.state == WatchState::Running
                && let Err(error) = candidate
                    .start_watch(watch.debounce_ms.unwrap_or(1000))
                    .await
            {
                let _ = candidate.stop_watch().await;
                let _ = current.start_watch(watch.debounce_ms.unwrap_or(1000)).await;
                return Err(UpdateError::Worker(error));
            }
        }
        let _ = current.service.close().await;
        std::mem::swap(&mut *current, &mut candidate);
        let mut update = self.update.lock().await;
        update.state = UpdateState::Current;
        update.generation += 1;
        update.error = None;
        update.reconnect_required = false;
        update.attempted = None;
        Ok(())
    }

    async fn forward(
        &self,
        mut request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        self.refresh().await;
        let worker = self.worker.clone().read_owned().await;
        let update = self.update.clone();
        // A caller cancellation must not drop the worker lease mid-mutation.
        // Finish accepted calls; never replay a request whose outcome is unknown.
        let task = tokio::spawn(async move {
            let reporting = matches!(request.name.as_ref(), "help" | "status");
            let upstream = context.meta.get_progress_token();
            request.meta = Some(context.meta.clone());
            let mut routes = worker.forward.0.lock().await;
            let sent = worker
                .service
                .send_cancellable_request(
                    ClientRequest::CallToolRequest(CallToolRequest::new(request)),
                    PeerRequestOptions::no_options(),
                )
                .await;
            let Ok(handle) = sent else {
                return Ok(failure());
            };
            let token = handle.progress_token.clone();
            if let Some(upstream) = upstream {
                routes.insert(token.clone(), (context.peer, upstream));
            }
            drop(routes);
            let response = handle.await_response().await;
            worker.forward.0.lock().await.remove(&token);
            let mut result = match response {
                Ok(ServerResult::CallToolResult(result)) => result,
                Err(rmcp::service::ServiceError::McpError(error)) => return Err(error),
                _ => failure(),
            };
            if reporting
                && let Some(fields) = result
                    .structured_content
                    .as_mut()
                    .and_then(serde_json::Value::as_object_mut)
            {
                let update = update.lock().await;
                if let Some(runtime) = fields
                    .get_mut("runtime")
                    .and_then(serde_json::Value::as_object_mut)
                {
                    if let Some(action) = runtime
                        .get("recommended_action")
                        .and_then(serde_json::Value::as_str)
                    {
                        result
                            .content
                            .retain(|block| block.as_text().is_none_or(|text| text.text != action));
                    }
                    runtime.insert(
                        "reconnect_required".into(),
                        serde_json::json!(update.reconnect_required),
                    );
                    let action = update.error.or_else(|| {
                        (runtime.get("contract_matches") == Some(&serde_json::Value::Bool(false)))
                            .then_some("Use live help for this worker's current parameters; cached skill text describes a different release.")
                    });
                    runtime.insert("recommended_action".into(), serde_json::json!(action));
                    if let Some(action) = action {
                        result.content.push(ContentBlock::text(action));
                    }
                }
                if let Ok(value) = serde_json::to_value(SessionInfo {
                    update: &update,
                    front_process_id: std::process::id(),
                    worker_process_id: worker.pid,
                }) {
                    fields.insert("session".into(), value);
                }
                result.content.push(ContentBlock::text(format!(
                    "Mash session — worker generation {}",
                    update.generation
                )));
                if let Some(error) = update.error {
                    result.content.push(ContentBlock::text(error));
                }
            }
            Ok(result)
        });
        task.await.unwrap_or_else(|_| Ok(failure()))
    }
}

fn failure() -> CallToolResult {
    super::tool_result(Err(AppError::internal(
        "Mash worker connection failed. The request was not retried; inspect status before repeating a mutation.",
    )))
}

impl ServerHandler for Session {
    fn get_info(&self) -> ServerConfig {
        self.info.clone()
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, rmcp::ErrorData> {
        self.refresh().await;
        Ok(ListToolsResult {
            tools: self.worker.read().await.tools.clone(),
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, rmcp::ErrorData> {
        self.forward(request, context).await.map(Into::into)
    }
}

pub(crate) async fn run(paths: Paths, watch: bool, debounce_ms: u64) -> Result<(), AppError> {
    let binary = std::env::current_exe()
        .map_err(|_| AppError::internal("Cannot locate Mash worker executable"))?;
    let session = Session::new(binary, paths)
        .await
        .map_err(AppError::internal)?;
    let background = session.clone();
    let service = session
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|_| AppError::internal("MCP session initialization failed"))?;
    if watch {
        let _ = background
            .worker
            .read()
            .await
            .start_watch(debounce_ms)
            .await;
    }
    let stop = tokio_util::sync::CancellationToken::new();
    let stopping = stop.clone();
    let monitoring = background.clone();
    let monitor = tokio::spawn(async move {
        loop {
            tokio::select! { () = stopping.cancelled() => break, () = tokio::time::sleep(Duration::from_secs(1)) => monitoring.refresh().await }
        }
    });
    let success = service.waiting().await.is_ok();
    stop.cancel();
    let _ = monitor.await;
    let mut worker = background.worker.write().await;
    if !worker.closed() {
        let _ = worker.stop_watch().await;
    }
    let _ = worker.service.close().await;
    if success {
        Ok(())
    } else {
        Err(AppError::internal("MCP session failed"))
    }
}

#[cfg(test)]
mod tests;
