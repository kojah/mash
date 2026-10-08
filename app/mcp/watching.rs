//! One process-local watcher shared by startup flags and MCP lifecycle tools.
use super::{Mash, WatchEvent, WatchIndex, core};
use crate::{AppError, progress};
use rmcp::model::{CallToolResult, ContentBlock};
use serde::Serialize;
use tokio::sync::watch;

#[derive(Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum State {
    #[default]
    Stopped,
    Running,
    Stopping,
}

#[derive(Default, Serialize)]
pub(super) struct Watcher {
    state: State,
    debounce_ms: Option<u64>,
    indexing: bool,
    progress: Option<progress::Event>,
    last_refresh_at: Option<u64>,
    last_result: Option<core::Response>,
    last_error: Option<AppError>,
    #[serde(skip)]
    stop: Option<watch::Sender<bool>>,
    #[serde(skip)]
    task: Option<tokio::task::JoinHandle<()>>,
    #[serde(skip)]
    closed: bool,
}

impl Watcher {
    pub(super) fn response(&self) -> CallToolResult {
        match (serde_json::to_value(self), self.text()) {
            (Ok(value), Ok(text)) => {
                let mut result = CallToolResult::structured(value);
                result.content = vec![ContentBlock::text(text)];
                result
            }
            _ => super::tool_result(Err(AppError::internal("serialize watcher status"))),
        }
    }

    fn text(&self) -> Result<String, serde_json::Error> {
        let mut text = format!(
            "Mash watcher — {}\nIndexing: {}\nDebounce (ms): {}\nLast refresh (Unix ms): {}\n",
            serde_json::to_string(&self.state)?,
            self.indexing,
            self.debounce_ms
                .map_or_else(|| "unset".into(), |value| value.to_string()),
            self.last_refresh_at
                .map_or_else(|| "never".into(), |value| value.to_string()),
        );
        if let Some(progress) = &self.progress {
            text.push_str("Progress: ");
            text.push_str(&progress::display(progress));
            text.push('\n');
        }
        if let Some(result) = &self.last_result {
            let json = core::output::serialize(result, core::output::Detail::Compact)?;
            let summary = core::output::text::render(result, core::output::Detail::Compact, &json)?;
            text.push_str("Last indexing result:\n");
            text.push_str(&summary);
        }
        if let Some(error) = &self.last_error {
            text.push_str("Last indexing error:\n");
            text.push_str(&core::output::text::error(error)?);
        }
        Ok(text)
    }

    fn stop(&mut self) {
        if self.state == State::Running {
            self.state = State::Stopping;
            if let Some(stop) = &self.stop {
                let _ = stop.send(true);
            }
        }
    }
}

pub(super) async fn start(server: Mash, debounce_ms: Option<u64>) -> CallToolResult {
    if debounce_ms.is_some_and(|value| !(50..=60_000).contains(&value)) {
        return super::tool_result(Err(AppError::usage("debounce_ms must be 50–60000")));
    }
    let mut watcher = server.watcher.lock().await;
    if watcher.closed || watcher.state == State::Stopping {
        return super::tool_result(Err(AppError::usage(
            "watcher is stopping; wait until stopped",
        )));
    }
    if watcher.state == State::Running {
        if debounce_ms.is_some_and(|value| Some(value) != watcher.debounce_ms) {
            return super::tool_result(Err(AppError::usage(
                "stop watching before changing debounce_ms",
            )));
        }
        return watcher.response();
    }
    let debounce_ms = debounce_ms.unwrap_or(1000);
    let (stop, shutdown) = watch::channel(false);
    watcher.state = State::Running;
    watcher.debounce_ms = Some(debounce_ms);
    watcher.progress = None;
    watcher.last_error = None;
    watcher.stop = Some(stop);
    let background = server.clone();
    watcher.task = Some(tokio::spawn(async move {
        let request = core::Request {
            paths: background.paths.as_ref().clone(),
            operation: core::Operation::Index(core::IndexRequest::default()),
        };
        let worker = background.clone();
        let result = Box::pin(crate::watch::run(
            request,
            debounce_ms,
            shutdown,
            move || refresh(worker.clone()),
        ))
        .await;
        let mut watcher = background.watcher.lock().await;
        if let Err(error) = result {
            let _ = crate::write_json(&mut std::io::stderr(), &error);
            watcher.last_error = Some(error);
        }
        watcher.indexing = false;
        watcher.state = State::Stopped;
        watcher.stop = None;
    }));
    watcher.response()
}

pub(super) async fn stop(server: &Mash) -> CallToolResult {
    let mut watcher = server.watcher.lock().await;
    watcher.stop();
    watcher.response()
}

pub(super) async fn shutdown(server: &Mash) {
    let task = {
        let mut watcher = server.watcher.lock().await;
        watcher.closed = true;
        watcher.stop();
        watcher.task.take()
    };
    if let Some(task) = task {
        let _ = task.await;
    }
}

async fn refresh(server: Mash) -> Result<bool, AppError> {
    {
        let mut watcher = server.watcher.lock().await;
        watcher.indexing = true;
        watcher.progress = None;
    }
    let (sender, mut events) = tokio::sync::mpsc::channel(64);
    let work = progress::scope(sender, async {
        progress::phase(progress::Phase::WaitingForIndex, 0);
        server
            .perform_response(core::Operation::Index(core::IndexRequest::default()))
            .await
    });
    let updates = async {
        while let Some(event) = events.recv().await {
            server.watcher.lock().await.progress = Some(event);
        }
    };
    let (result, ()) = tokio::join!(work, updates);
    let retry = result.as_ref().map_or(true, core::Response::needs_retry);
    let mut watcher = server.watcher.lock().await;
    watcher.indexing = false;
    watcher.last_refresh_at = Some(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX),
    );
    match result {
        Ok(result) => {
            watcher.last_result = Some(result);
            watcher.last_error = None;
        }
        Err(error) => {
            watcher.last_result = None;
            watcher.last_error = Some(error);
        }
    }
    // Keep the existing structured stderr record for startup-watch users.
    let result = if let Some(error) = &watcher.last_error {
        serde_json::to_value(error)
    } else {
        serde_json::to_value(&watcher.last_result)
    }
    .map_err(|_| AppError::internal("serialize watched indexing result"))?;
    drop(watcher);
    crate::write_json(
        &mut std::io::stderr(),
        &WatchIndex {
            event: WatchEvent::Index,
            result: if retry {
                CallToolResult::structured_error(result)
            } else {
                CallToolResult::structured(result)
            },
        },
    )?;
    Ok(retry)
}
