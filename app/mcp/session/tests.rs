use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;

#[derive(Default)]
struct Work {
    started: Notify,
    finish: Notify,
    calls: AtomicUsize,
}

#[derive(Clone)]
struct Mock {
    label: &'static str,
    work: Option<Arc<Work>>,
    watcher: Arc<Mutex<Watch>>,
}

impl ServerHandler for Mock {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            rmcp::model::ServerCapabilities::builder()
                .enable_tools()
                .build(),
        )
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, rmcp::ErrorData> {
        let data = match request.name.as_ref() {
            "index" => {
                if let Some(work) = &self.work {
                    work.calls.fetch_add(1, Ordering::SeqCst);
                    if let Some(token) = context.meta.get_progress_token() {
                        let _ = context
                            .peer
                            .notify_progress(ProgressNotificationParam::new(token, 1.0))
                            .await;
                    }
                    work.started.notify_one();
                    work.finish.notified().await;
                }
                serde_json::json!({"label": self.label})
            }
            "watch_status" => serde_json::to_value(&*self.watcher.lock().await).unwrap(),
            "watch_stop" => {
                self.watcher.lock().await.state = WatchState::Stopped;
                serde_json::json!({})
            }
            "watch_start" => {
                let mut watcher = self.watcher.lock().await;
                watcher.state = WatchState::Running;
                watcher.debounce_ms = request
                    .arguments
                    .as_ref()
                    .and_then(|a| a.get("debounce_ms"))
                    .and_then(serde_json::Value::as_u64);
                serde_json::json!({})
            }
            _ => serde_json::json!({"label":self.label}),
        };
        Ok(CallToolResult::structured(data).into())
    }
}

async fn mock(
    binary: &std::path::Path,
    label: &'static str,
    work: Option<Arc<Work>>,
) -> (Worker, RunningService<RoleServer, Mock>, Arc<Mutex<Watch>>) {
    let watcher = Arc::new(Mutex::new(Watch {
        state: WatchState::Stopped,
        debounce_ms: None,
    }));
    let handler = Mock {
        label,
        work,
        watcher: watcher.clone(),
    };
    let (server_io, client_io) = tokio::io::duplex(128 * 1024);
    let serving = tokio::spawn(async move { handler.serve(server_io).await.unwrap() });
    let forward = Forward::default();
    let service = forward.clone().serve(client_io).await.unwrap();
    let server = serving.await.unwrap();
    service
        .call_tool(CallToolRequestParams::new("help"))
        .await
        .unwrap();
    (
        Worker {
            pid: None,
            service,
            forward,
            stamp: guidance::stamp(binary).unwrap(),
            tools: super::super::Mash::tool_router().list_all(),
            schemas: Schemas {
                silver_schema: 53,
                views_schema: 4,
            },
        },
        server,
        watcher,
    )
}

fn session(binary: PathBuf, worker: Worker) -> Session {
    Session {
        binary,
        paths: Paths::default(),
        worker: Arc::new(RwLock::new(worker)),
        upgrade: Arc::default(),
        info: ServerConfig::new(
            rmcp::model::ServerCapabilities::builder()
                .enable_tools()
                .build(),
        ),
        update: Arc::new(Mutex::new(Update {
            state: UpdateState::Current,
            generation: 1,
            front_build_revision: "test",
            error: None,
            reconnect_required: false,
            attempted: None,
        })),
    }
}

#[derive(Clone, Default)]
struct Progress(Arc<Mutex<Vec<ProgressNotificationParam>>>);

impl ClientHandler for Progress {
    async fn on_progress(
        &self,
        progress: ProgressNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) {
        self.0.lock().await.push(progress);
    }
}

async fn draining(session: &Session) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if matches!(session.update.lock().await.state, UpdateState::Draining) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn worker_interface_requires_identical_names_schemas_and_effect_annotations() {
    let original = super::super::Mash::tool_router().list_all();
    let mut edited = original.clone();
    edited.reverse();
    edited[0].description = Some("Updated live instructions".into());
    assert_eq!(interface(&original), interface(&edited));
    edited[0].name = "renamed".into();
    assert_ne!(interface(&original), interface(&edited));
    let mut edited = original.clone();
    edited[0].input_schema = Arc::new(serde_json::Map::new());
    assert_ne!(interface(&original), interface(&edited));
    let mut edited = original.clone();
    edited[0].annotations = None;
    assert_ne!(interface(&original), interface(&edited));
}

#[tokio::test]
async fn worker_handover_drains_mutations_preserves_watch_and_routes_original_progress() {
    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join("worker");
    std::fs::write(&binary, b"fixture").unwrap();
    let work = Arc::new(Work::default());
    let (old, old_server, old_watch) = mock(&binary, "old", Some(work.clone())).await;
    *old_watch.lock().await = Watch {
        state: WatchState::Running,
        debounce_ms: Some(350),
    };
    let (candidate, new_server, new_watch) = mock(&binary, "new", None).await;
    let session = session(binary, old);
    let (server_io, client_io) = tokio::io::duplex(128 * 1024);
    let handler = session.clone();
    let serving = tokio::spawn(async move { handler.serve(server_io).await.unwrap() });
    let progress = Progress::default();
    let client = progress.clone().serve(client_io).await.unwrap();
    let front = serving.await.unwrap();
    let mut request = CallToolRequestParams::new("index");
    request.meta = Some(rmcp::model::RequestMetaObject::with_progress_token(
        ProgressToken(rmcp::model::NumberOrString::String("upstream".into())),
    ));
    let handle = client
        .send_cancellable_request(
            ClientRequest::CallToolRequest(CallToolRequest::new(request)),
            PeerRequestOptions::no_options(),
        )
        .await
        .unwrap();
    let original_token = handle.progress_token.clone();
    let calling = tokio::spawn(async move {
        let ServerResult::CallToolResult(result) = handle.await_response().await.unwrap() else {
            panic!("expected tool result");
        };
        result
    });
    work.started.notified().await;
    let switching = session.clone();
    let swap = tokio::spawn(async move { switching.handover(candidate).await.unwrap() });
    draining(&session).await;
    assert!(!swap.is_finished());
    assert_eq!(old_watch.lock().await.state, WatchState::Running);
    assert_eq!(new_watch.lock().await.state, WatchState::Stopped);
    work.finish.notify_one();
    let result = calling.await.unwrap();
    assert_eq!(result.structured_content.unwrap()["label"], "old");
    swap.await.unwrap();
    assert_eq!(work.calls.load(Ordering::SeqCst), 1);
    assert_eq!(old_watch.lock().await.state, WatchState::Stopped);
    assert_eq!(new_watch.lock().await.state, WatchState::Running);
    assert_eq!(new_watch.lock().await.debounce_ms, Some(350));
    let result = client
        .call_tool(CallToolRequestParams::new("help"))
        .await
        .unwrap();
    assert_eq!(result.structured_content.unwrap()["label"], "new");
    // The SDK replaces caller-supplied tokens; the front must preserve the token
    // actually assigned to the client request, rather than the backend token.
    assert_eq!(progress.0.lock().await.len(), 1);
    assert_eq!(progress.0.lock().await[0].progress_token, original_token);
    client.cancel().await.unwrap();
    front.waiting().await.unwrap();
    session.worker.write().await.service.close().await.unwrap();
    old_server.waiting().await.unwrap();
    new_server.waiting().await.unwrap();
}

#[tokio::test]
async fn worker_handover_waits_for_cancelled_mutation_without_replaying_it() {
    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join("worker");
    std::fs::write(&binary, b"fixture").unwrap();
    let work = Arc::new(Work::default());
    let (old, old_server, _) = mock(&binary, "old", Some(work.clone())).await;
    let (candidate, new_server, _) = mock(&binary, "new", None).await;
    let session = session(binary, old);
    let (server_io, client_io) = tokio::io::duplex(128 * 1024);
    let handler = session.clone();
    let serving = tokio::spawn(async move { handler.serve(server_io).await.unwrap() });
    let client = ().serve(client_io).await.unwrap();
    let front = serving.await.unwrap();
    let handle = client
        .send_cancellable_request(
            ClientRequest::CallToolRequest(CallToolRequest::new(CallToolRequestParams::new(
                "index",
            ))),
            PeerRequestOptions::no_options(),
        )
        .await
        .unwrap();
    work.started.notified().await;
    handle
        .cancel(Some("caller cancelled".into()))
        .await
        .unwrap();
    let switching = session.clone();
    let swap = tokio::spawn(async move { switching.handover(candidate).await.unwrap() });
    draining(&session).await;
    assert!(!swap.is_finished());
    work.finish.notify_one();
    swap.await.unwrap();
    assert_eq!(work.calls.load(Ordering::SeqCst), 1);
    client.cancel().await.unwrap();
    front.waiting().await.unwrap();
    session.worker.write().await.service.close().await.unwrap();
    old_server.waiting().await.unwrap();
    new_server.waiting().await.unwrap();
}

#[tokio::test]
async fn worker_handover_rejects_changed_storage_formats_and_keeps_current_worker() {
    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join("worker");
    std::fs::write(&binary, b"fixture").unwrap();
    let (old, old_server, old_watch) = mock(&binary, "old", None).await;
    old_watch.lock().await.state = WatchState::Running;
    let (mut candidate, new_server, _) = mock(&binary, "new", None).await;
    candidate.schemas.silver_schema += 1;
    let session = session(binary, old);
    assert!(
        session
            .handover(candidate)
            .await
            .unwrap_err()
            .message()
            .contains("formats changed")
    );
    assert_eq!(old_watch.lock().await.state, WatchState::Running);
    assert!(!session.worker.read().await.service.is_closed());
    session.worker.write().await.service.close().await.unwrap();
    old_server.waiting().await.unwrap();
    new_server.waiting().await.unwrap();
}
