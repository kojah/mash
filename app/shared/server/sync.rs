//! Authenticated `/v1/sync` WebSocket: typed frames, durable checkpoints and
//! application through the existing silver-before-views transaction boundary.
use super::{App, Identity, admission};
use crate::AppError;
use crate::attachments::{Availability, Metadata};
use crate::connectors::Batch;
use crate::shared::protocol::{self, Event};
use crate::shared::silver_store;
use crate::shared::sync::catalog::{self, Begin, BeginOutcome, Catalog, StoredChunk};
use crate::shared::sync::wire::{
    self, Accepted, AttachmentStored, BackfillComplete, Cancelled, Checkpoint, Chunk, CommitState,
    ConnectorReceipt, Credit, Failed, FinishBackfill, FinishUpload, Hello, Outcome, OutcomeState,
    Progress, Published, Resume, ServerFrame, SilverCommitted, TooLateToCancel, Welcome,
    client_frame, server_frame,
};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Extension, State};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const IDLE_TIMEOUT: Duration = Duration::from_secs(600);
/// Abandoned receiving transfers release their spool after this inactivity.
const INCOMPLETE_RETENTION: Duration = Duration::from_hours(24);
/// Hard ceiling on retained spool bytes across every transfer. Four concurrent
/// maximum-size uploads plus headroom, bounded so a stuck set of transfers
/// cannot consume an unbounded disk before expiry reclaims them.
const SPOOL_QUOTA: u64 = 2 * 1024 * 1024 * 1024;
/// How often the server reclaims abandoned spools without a client or restart.
const EXPIRY_SWEEP: Duration = Duration::from_mins(1);
const HEARTBEAT_MS: u32 = 30_000;
const PRUNE_RECEIPTS: usize = 64;

pub(super) type CatalogHandle = Arc<Mutex<Catalog>>;

pub(super) fn open_catalog(root: &std::path::Path) -> Result<Catalog, AppError> {
    Catalog::open_with_limit(root, SPOOL_QUOTA)
}
type Sink = futures_util::stream::SplitSink<WebSocket, Message>;

pub(super) async fn upgrade(
    State(app): State<App>,
    Extension(identity): Extension<Identity>,
    upgrade: WebSocketUpgrade,
) -> Response {
    upgrade
        .max_message_size(wire::MAX_ENVELOPE_BYTES)
        .max_frame_size(wire::MAX_ENVELOPE_BYTES)
        .on_upgrade(move |socket| Box::pin(connection(app, identity, socket)))
}

async fn connection(app: App, identity: Identity, socket: WebSocket) {
    if let Err(error) = run(app, identity, socket).await {
        tracing::warn!(
            kind = ?error.error.kind,
            message = %error.error.message,
            "shared synchronization connection ended"
        );
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one connection state machine keeps frame handling and shutdown together"
)]
async fn run(app: App, identity: Identity, socket: WebSocket) -> Result<(), AppError> {
    // One catalog handle for the whole process: every connection shares it so
    // concurrent lanes queue on the in-process lock instead of contending for
    // SQLite's file lock.
    let catalog = app.catalog.clone();
    // Expire transfers abandoned by earlier sessions before accepting new work.
    let cutoff = epoch_millis()
        .saturating_sub(i64::try_from(INCOMPLETE_RETENTION.as_millis()).unwrap_or(i64::MAX));
    if let Ok(expired) = crate::shared::server::sync::catalog(&catalog, move |catalog| {
        catalog.expire_incomplete(cutoff)
    })
    .await
        && expired > 0
    {
        tracing::info!(expired, "expired abandoned synchronization transfers");
    }
    let (mut sink, mut stream) = socket.split();
    let mut active: Option<String> = None;
    loop {
        let message = match tokio::time::timeout(IDLE_TIMEOUT, stream.next()).await {
            Ok(Some(Ok(message))) => message,
            Ok(Some(Err(_)) | None) => return Ok(()),
            Err(_) => {
                send(
                    &mut sink,
                    "",
                    server_frame::Body::Failed(Failed {
                        phase: "connection".into(),
                        code: "idle_timeout".into(),
                        commit_state: CommitState::Unknown as i32,
                        retryable: true,
                        recovery_action: "reconnect and resume the operation".into(),
                        message: "synchronization connection was idle".into(),
                    }),
                )
                .await?;
                return Ok(());
            }
        };
        let Message::Binary(bytes) = message else {
            if matches!(message, Message::Close(_)) {
                return Ok(());
            }
            continue;
        };
        let Ok(frame) = wire::decode_client(&bytes) else {
            send(
                &mut sink,
                "",
                server_frame::Body::Failed(Failed {
                    phase: "receiving".into(),
                    code: "malformed_frame".into(),
                    commit_state: CommitState::Unknown as i32,
                    retryable: false,
                    recovery_action: String::new(),
                    message: "invalid synchronization frame".into(),
                }),
            )
            .await?;
            return Ok(());
        };
        let operation_id = frame.operation_id.clone();
        let Some(body) = frame.body else {
            continue;
        };
        let close = matches!(body, client_frame::Body::Hello(_));
        match body {
            client_frame::Body::Hello(hello) => {
                handle_hello(&mut sink, &app, &identity, hello).await?;
            }
            client_frame::Body::BeginPublication(begin) => {
                active = Some(operation_id.clone());
                handle_begin(&mut sink, &app, &identity, &catalog, &operation_id, begin).await?;
            }
            client_frame::Body::Chunk(chunk) => {
                require_active(active.as_deref(), &operation_id)?;
                handle_chunk(
                    &mut sink,
                    &catalog,
                    &identity,
                    &operation_id,
                    chunk,
                    app.store.max_node_bytes(),
                )
                .await?;
            }
            client_frame::Body::FinishUpload(finish) => {
                require_active(active.as_deref(), &operation_id)?;
                Box::pin(handle_finish(
                    &mut sink,
                    &app,
                    &identity,
                    &catalog,
                    &operation_id,
                    finish,
                ))
                .await?;
            }
            client_frame::Body::Resume(resume) => {
                active = Some(operation_id.clone());
                handle_resume(&mut sink, &catalog, &identity, &operation_id, resume).await?;
            }
            client_frame::Body::Cancel(_) => {
                handle_cancel(&mut sink, &catalog, &identity, &operation_id).await?;
                active = None;
            }
            client_frame::Body::BeginBackfill(begin) => {
                active = Some(operation_id.clone());
                handle_begin_backfill(&mut sink, &app, &identity, &catalog, &operation_id, begin)
                    .await?;
            }
            client_frame::Body::BeginConnectorBatch(begin) => {
                active = Some(operation_id.clone());
                handle_begin_connector(&mut sink, &app, &identity, &catalog, &operation_id, begin)
                    .await?;
            }
            client_frame::Body::BeginAttachment(begin) => {
                active = Some(operation_id.clone());
                handle_begin_attachment(&mut sink, &app, &identity, &catalog, &operation_id, begin)
                    .await?;
            }
            client_frame::Body::FinishBackfill(finish) => {
                // Completion names its own baseline; the catalog enforces group
                // and node ownership, so it is not bound to the active operation.
                handle_finish_backfill(&mut sink, &catalog, &identity, &operation_id, finish)
                    .await?;
            }
        }
        if close {
            // Hello starts a session; keep the connection open for its operations.
        }
    }
}

/// Test-only crash injection. Debug builds abort at a named point so an
/// integration test can restart the server and prove recovery converges.
/// Release builds ignore the variable entirely.
#[cfg(debug_assertions)]
fn crash_point(name: &str) {
    if std::env::var("MASH_TEST_CRASH_AT").is_ok_and(|value| value == name) {
        std::process::abort();
    }
}

#[cfg(not(debug_assertions))]
fn crash_point(_name: &str) {}

fn epoch_millis() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_millis()),
    )
    .unwrap_or(i64::MAX)
}

fn require_active(active: Option<&str>, operation_id: &str) -> Result<(), AppError> {
    if active == Some(operation_id) {
        Ok(())
    } else {
        Err(AppError::shared(
            "synchronization frame does not belong to the active operation",
        ))
    }
}

async fn catalog<T: Send + 'static>(
    handle: &CatalogHandle,
    work: impl FnOnce(&Catalog) -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    let handle = handle.clone();
    crate::blocking(move || {
        let catalog = handle
            .lock()
            .map_err(|_| AppError::internal("synchronization catalog lock is poisoned"))?;
        work(&catalog)
    })
    .await
}

async fn send(
    sink: &mut Sink,
    operation_id: &str,
    body: server_frame::Body,
) -> Result<(), AppError> {
    let frame = ServerFrame {
        operation_id: operation_id.to_owned(),
        body: Some(body),
    };
    let bytes = wire::encode_server(&frame)?;
    sink.send(Message::Binary(bytes.into()))
        .await
        .map_err(|_| AppError::shared("synchronization connection failed"))
}

async fn handle_hello(
    sink: &mut Sink,
    app: &App,
    identity: &Identity,
    hello: Hello,
) -> Result<(), AppError> {
    if hello.protocol_version != wire::PROTOCOL_VERSION
        || hello.database_instance.is_empty()
        || !hello.zstd_publication_data
    {
        send(
            sink,
            "",
            server_frame::Body::Rejected(Failed {
                phase: "handshake".into(),
                code: "unsupported_protocol".into(),
                commit_state: CommitState::NotCommitted as i32,
                retryable: false,
                recovery_action: "upgrade the node to a matching mash release".into(),
                message: format!(
                    "synchronization requires protocol version {} with zstd publication data",
                    wire::PROTOCOL_VERSION
                ),
            }),
        )
        .await?;
        return Ok(());
    }
    let session = crate::shared::silver_store::token()?;
    let max_decoded = hello
        .max_decoded_bytes
        .min(app.store.max_node_bytes())
        .max(1);
    send(
        sink,
        "",
        server_frame::Body::Welcome(Welcome {
            protocol_version: wire::PROTOCOL_VERSION,
            session_id: session,
            credit_bytes: wire::DEFAULT_CREDIT_BYTES,
            max_envelope_bytes: u32::try_from(wire::MAX_ENVELOPE_BYTES).unwrap_or(u32::MAX),
            max_decoded_bytes: max_decoded,
            heartbeat_ms: HEARTBEAT_MS,
            retained_revision: None,
            views_fingerprint: identity.generation.clone(),
            incomplete_retention_ms: u64::try_from(INCOMPLETE_RETENTION.as_millis())
                .unwrap_or(u64::MAX),
        }),
    )
    .await?;
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "begin validates metadata, mode and manifest before one durable insert"
)]
async fn handle_begin(
    sink: &mut Sink,
    app: &App,
    identity: &Identity,
    handle: &CatalogHandle,
    operation_id: &str,
    begin: wire::BeginPublication,
) -> Result<(), AppError> {
    let Some(start) = begin.start else {
        return reject(
            sink,
            operation_id,
            "begin",
            "missing_publication_metadata",
            "publication requires source metadata",
        )
        .await;
    };
    let Ok(start) = protocol::Start::try_from(start) else {
        return reject(
            sink,
            operation_id,
            "begin",
            "invalid_publication_metadata",
            "publication metadata is not valid",
        )
        .await;
    };
    if start.instance.is_empty() || start.instance.len() > 4096 {
        return reject(
            sink,
            operation_id,
            "begin",
            "invalid_publication_metadata",
            "publication source instance is invalid",
        )
        .await;
    }
    if let Err(error) = start.validate_source_silver() {
        return reject(
            sink,
            operation_id,
            "begin",
            "invalid_publication_metadata",
            &error.error.message,
        )
        .await;
    }
    let mode = match wire::PublicationMode::parse(begin.mode) {
        Some(wire::PublicationMode::Delta) => wire::PublicationMode::Delta,
        Some(wire::PublicationMode::BackfillBatch) => wire::PublicationMode::BackfillBatch,
        Some(wire::PublicationMode::ReplaceSnapshot) => wire::PublicationMode::ReplaceSnapshot,
        _ => {
            return reject(
                sink,
                operation_id,
                "begin",
                "unsupported_mode",
                "publication mode is not supported by this synchronization endpoint",
            )
            .await;
        }
    };
    let snapshot = mode == wire::PublicationMode::ReplaceSnapshot;
    if snapshot != start.snapshot {
        return reject(
            sink,
            operation_id,
            "begin",
            "mode_mismatch",
            "publication mode disagrees with source metadata",
        )
        .await;
    }
    let backfill_id = begin.backfill_id.clone();
    if mode == wire::PublicationMode::BackfillBatch {
        if backfill_id.is_empty() || start.snapshot {
            return reject(
                sink,
                operation_id,
                "begin",
                "backfill_mode_mismatch",
                "backfill batches require a backfill identity and non-snapshot metadata",
            )
            .await;
        }
        let group = identity.group_id.clone();
        let node = identity.node_id.clone();
        let selected = backfill_id.clone();
        let backfill = catalog(handle, move |catalog| {
            catalog.backfill(&selected, &group, &node)
        })
        .await;
        let Ok(backfill) = backfill else {
            return reject(
                sink,
                operation_id,
                "begin",
                "unknown_backfill",
                "backfill identity is not active for this node",
            )
            .await;
        };
        if backfill.state != "ready" && backfill.state != "complete" {
            return reject(
                sink,
                operation_id,
                "begin",
                "backfill_not_ready",
                "backfill manifest is still being transferred",
            )
            .await;
        }
        if begin.source_revision != Some(backfill.baseline_revision) {
            return reject(
                sink,
                operation_id,
                "begin",
                "backfill_revision_mismatch",
                "backfill batch does not match the frozen baseline revision",
            )
            .await;
        }
    } else if !backfill_id.is_empty() {
        return reject(
            sink,
            operation_id,
            "begin",
            "backfill_mode_mismatch",
            "only backfill batches may name a backfill identity",
        )
        .await;
    }
    if begin.manifest_digest.len() != 64
        || !begin.manifest_digest.bytes().all(|b| b.is_ascii_hexdigit())
        || begin.decoded_bytes > app.store.max_node_bytes()
    {
        return reject(
            sink,
            operation_id,
            "begin",
            "invalid_manifest",
            "publication manifest or declared size is invalid",
        )
        .await;
    }
    let start_json = if mode == wire::PublicationMode::BackfillBatch {
        serde_json::to_string(&serde_json::json!({
            "start": Event::Start(start),
            "backfill_id": backfill_id,
        }))
    } else {
        serde_json::to_string(&Event::Start(start))
    };
    let Ok(start_json) = start_json else {
        return reject(
            sink,
            operation_id,
            "begin",
            "invalid_publication_metadata",
            "publication metadata could not be encoded",
        )
        .await;
    };
    let manifest = begin.manifest_digest;
    let decoded_bytes = begin.decoded_bytes;
    let record_count = begin.record_count;
    let group = identity.group_id.clone();
    let node = identity.node_id.clone();
    let operation_id_owned = operation_id.to_owned();
    let result = catalog(handle, move |catalog| {
        catalog.begin(&Begin {
            operation_id: &operation_id_owned,
            group_id: &group,
            node_id: &node,
            kind: "publication",
            manifest_digest: &manifest,
            decoded_bytes,
            record_count,
            start_json: &start_json,
        })
    })
    .await;
    let operation = match result {
        Ok(BeginOutcome::New(operation) | BeginOutcome::Existing(operation)) => operation,
        Err(error) => {
            return send(
                sink,
                operation_id,
                server_frame::Body::Rejected(Failed {
                    phase: "begin".into(),
                    code: "begin_rejected".into(),
                    commit_state: CommitState::NotCommitted as i32,
                    retryable: error.error.retryable,
                    recovery_action: "reconnect and resume with the same operation".into(),
                    message: error.error.message,
                }),
            )
            .await;
        }
    };
    if operation.state == catalog::State::Published {
        return send(
            sink,
            operation_id,
            server_frame::Body::Outcome(outcome(&operation, CommitState::Committed)),
        )
        .await;
    }
    if operation.next_sequence > 0 {
        send(
            sink,
            operation_id,
            server_frame::Body::Checkpoint(Checkpoint {
                sequence: operation.next_sequence - 1,
                decoded_offset: operation.decoded_offset,
            }),
        )
        .await?;
    }
    accepted(sink, operation_id).await
}

async fn finish_begin(
    sink: &mut Sink,
    operation_id: &str,
    operation: catalog::Operation,
) -> Result<(), AppError> {
    if operation.state == catalog::State::Published {
        return send(
            sink,
            operation_id,
            server_frame::Body::Outcome(outcome(&operation, CommitState::Committed)),
        )
        .await;
    }
    if operation.next_sequence > 0 {
        send(
            sink,
            operation_id,
            server_frame::Body::Checkpoint(Checkpoint {
                sequence: operation.next_sequence - 1,
                decoded_offset: operation.decoded_offset,
            }),
        )
        .await?;
    }
    accepted(sink, operation_id).await
}

struct OwnedBegin {
    operation_id: String,
    group_id: String,
    node_id: String,
    kind: String,
    manifest_digest: String,
    decoded_bytes: u64,
    record_count: u32,
    start_json: String,
}

async fn begin_operation(
    sink: &mut Sink,
    handle: &CatalogHandle,
    operation_id: &str,
    begin: OwnedBegin,
) -> Result<(), AppError> {
    let result = catalog(handle, move |catalog| {
        catalog.begin(&Begin {
            operation_id: &begin.operation_id,
            group_id: &begin.group_id,
            node_id: &begin.node_id,
            kind: &begin.kind,
            manifest_digest: &begin.manifest_digest,
            decoded_bytes: begin.decoded_bytes,
            record_count: begin.record_count,
            start_json: &begin.start_json,
        })
    })
    .await;
    match result {
        Ok(BeginOutcome::New(operation) | BeginOutcome::Existing(operation)) => {
            finish_begin(sink, operation_id, operation).await
        }
        Err(error) => {
            send(
                sink,
                operation_id,
                server_frame::Body::Rejected(Failed {
                    phase: "begin".into(),
                    code: "begin_rejected".into(),
                    commit_state: CommitState::NotCommitted as i32,
                    retryable: error.error.retryable,
                    recovery_action: "reconnect and resume with the same operation".into(),
                    message: error.error.message,
                }),
            )
            .await
        }
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "baseline identity, staleness and manifest checks before one durable insert"
)]
async fn handle_begin_backfill(
    sink: &mut Sink,
    app: &App,
    identity: &Identity,
    handle: &CatalogHandle,
    operation_id: &str,
    begin: wire::BeginBackfill,
) -> Result<(), AppError> {
    let valid = !begin.backfill_id.is_empty()
        && begin.backfill_id.len() <= 4096
        && !begin.database_instance.is_empty()
        && begin.baseline_revision >= 0
        && !begin.views_fingerprint.is_empty()
        && begin.manifest_digest.len() == 64
        && begin.manifest_digest.bytes().all(|b| b.is_ascii_hexdigit())
        && begin.total_conversations > 0
        && begin.decoded_bytes <= app.store.max_node_bytes();
    if !valid {
        return reject(
            sink,
            operation_id,
            "begin",
            "invalid_backfill",
            "backfill metadata or manifest is invalid",
        )
        .await;
    }
    let Ok(records) = u32::try_from(begin.total_conversations) else {
        return reject(
            sink,
            operation_id,
            "begin",
            "invalid_backfill",
            "backfill has too many baseline members",
        )
        .await;
    };
    // A baseline frozen at a revision the node has already moved past cannot be
    // merged safely: silver no longer has per-conversation ordering for it.
    let head = match app.store.head_revision(identity).await {
        Ok(head) => head,
        Err(error) => {
            return reject(
                sink,
                operation_id,
                "begin",
                "invalid_backfill",
                &error.error.message,
            )
            .await;
        }
    };
    if head > begin.baseline_revision {
        return reject(
            sink,
            operation_id,
            "begin",
            "backfill_baseline_stale",
            "baseline revision is older than the node's current revision",
        )
        .await;
    }
    let backfill_id = begin.backfill_id.clone();
    let database_instance = begin.database_instance.clone();
    let views_fingerprint = begin.views_fingerprint.clone();
    let manifest_digest = begin.manifest_digest.clone();
    let baseline_revision = begin.baseline_revision;
    let total_conversations = begin.total_conversations;
    let start_json = serde_json::json!({
        "backfill_id": backfill_id,
        "database_instance": database_instance,
        "baseline_revision": baseline_revision,
        "views_fingerprint": views_fingerprint,
    })
    .to_string();
    let group = identity.group_id.clone();
    let node = identity.node_id.clone();
    let (seed_id, seed_group, seed_node) = (backfill_id.clone(), group.clone(), node.clone());
    let (seed_instance, seed_fingerprint, seed_manifest) = (
        database_instance.clone(),
        views_fingerprint.clone(),
        manifest_digest.clone(),
    );
    let opened = catalog(handle, move |catalog| {
        catalog.begin_backfill(&catalog::BackfillBegin {
            backfill_id: &seed_id,
            group_id: &seed_group,
            node_id: &seed_node,
            database_instance: &seed_instance,
            baseline_revision,
            views_fingerprint: &seed_fingerprint,
            manifest_digest: &seed_manifest,
            total_conversations,
        })
    })
    .await;
    if let Err(error) = opened {
        return send(
            sink,
            operation_id,
            server_frame::Body::Rejected(Failed {
                phase: "begin".into(),
                code: "begin_rejected".into(),
                commit_state: CommitState::NotCommitted as i32,
                retryable: error.error.retryable,
                recovery_action: "start a new backfill identity".into(),
                message: error.error.message,
            }),
        )
        .await;
    }
    let owned = OwnedBegin {
        operation_id: operation_id.to_owned(),
        group_id: group,
        node_id: node,
        kind: "backfill_manifest".into(),
        manifest_digest,
        decoded_bytes: begin.decoded_bytes,
        record_count: records,
        start_json,
    };
    begin_operation(sink, handle, operation_id, owned).await
}

async fn handle_begin_connector(
    sink: &mut Sink,
    app: &App,
    identity: &Identity,
    handle: &CatalogHandle,
    operation_id: &str,
    begin: wire::BeginConnectorBatch,
) -> Result<(), AppError> {
    let valid = !begin.source_id.is_empty()
        && begin.source_id.len() <= 256
        && !begin.batch_id.is_empty()
        && begin.batch_id.len() <= 256
        && begin.expected_revision.parse::<u64>().is_ok()
        && begin.manifest_digest.len() == 64
        && begin.manifest_digest.bytes().all(|b| b.is_ascii_hexdigit())
        && begin.decoded_bytes > 0
        && begin.decoded_bytes <= app.store.max_node_bytes();
    if !valid {
        return reject(
            sink,
            operation_id,
            "begin",
            "invalid_connector_batch",
            "connector batch metadata is invalid",
        )
        .await;
    }
    let start_json = serde_json::json!({
        "source_id": begin.source_id,
        "batch_id": begin.batch_id,
        "expected_revision": begin.expected_revision,
    })
    .to_string();
    let owned = OwnedBegin {
        operation_id: operation_id.to_owned(),
        group_id: identity.group_id.clone(),
        node_id: identity.node_id.clone(),
        kind: "connector".into(),
        manifest_digest: begin.manifest_digest,
        decoded_bytes: begin.decoded_bytes,
        record_count: 0,
        start_json,
    };
    begin_operation(sink, handle, operation_id, owned).await
}

async fn handle_begin_attachment(
    sink: &mut Sink,
    app: &App,
    identity: &Identity,
    handle: &CatalogHandle,
    operation_id: &str,
    begin: wire::BeginAttachment,
) -> Result<(), AppError> {
    let Some(attachment) = begin.attachment else {
        return reject(
            sink,
            operation_id,
            "begin",
            "invalid_attachment",
            "attachment metadata is required",
        )
        .await;
    };
    let Ok(metadata) = Metadata::try_from(attachment) else {
        return reject(
            sink,
            operation_id,
            "begin",
            "invalid_attachment",
            "attachment metadata is invalid",
        )
        .await;
    };
    if begin.message_id.is_empty()
        || metadata.id.is_empty()
        || metadata.availability != Availability::Retained
        || metadata.digest.as_deref().is_none_or(|digest| {
            digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit())
        })
        || metadata.bytes.is_none()
        || begin.decoded_bytes != metadata.bytes.unwrap_or_default()
        || begin.decoded_bytes
            > app
                .store
                .max_node_bytes()
                .min(crate::attachments::MAX_BYTES as u64)
        || begin.manifest_digest.len() != 64
        || !begin.manifest_digest.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return reject(
            sink,
            operation_id,
            "begin",
            "invalid_attachment",
            "attachment metadata or size is invalid",
        )
        .await;
    }
    let start_json = serde_json::json!({
        "message_id": begin.message_id,
        "attachment": metadata,
    })
    .to_string();
    let owned = OwnedBegin {
        operation_id: operation_id.to_owned(),
        group_id: identity.group_id.clone(),
        node_id: identity.node_id.clone(),
        kind: "attachment".into(),
        manifest_digest: begin.manifest_digest,
        decoded_bytes: begin.decoded_bytes,
        record_count: 1,
        start_json,
    };
    begin_operation(sink, handle, operation_id, owned).await
}

async fn accepted(sink: &mut Sink, operation_id: &str) -> Result<(), AppError> {
    send(
        sink,
        operation_id,
        server_frame::Body::Accepted(Accepted {
            credit_bytes: wire::DEFAULT_CREDIT_BYTES,
        }),
    )
    .await
}

#[allow(
    clippy::too_many_lines,
    reason = "piece validation, durability and flow control share one receive step"
)]
async fn handle_chunk(
    sink: &mut Sink,
    handle: &CatalogHandle,
    identity: &Identity,
    operation_id: &str,
    chunk: Chunk,
    max_decoded: u64,
) -> Result<(), AppError> {
    let compressed = chunk.compression == wire::Compression::Zstd as i32;
    let uncompressed = chunk.compression == wire::Compression::None as i32;
    if (!compressed && !uncompressed)
        || chunk.decoded_length == 0
        || chunk.decoded_length > max_decoded
        || chunk.data.is_empty()
        || chunk.data.len() > wire::MAX_ENVELOPE_BYTES
    {
        return reject(
            sink,
            operation_id,
            "receiving",
            "invalid_piece",
            "sync piece exceeded its negotiated limits",
        )
        .await;
    }
    let decoded_length = usize::try_from(chunk.decoded_length).unwrap_or(usize::MAX);
    let max_decoded = usize::try_from(max_decoded).unwrap_or(usize::MAX);
    let decoded = if compressed {
        match wire::decompress(&chunk.data, decoded_length, max_decoded) {
            Ok(decoded) => decoded,
            Err(_) => {
                return reject(
                    sink,
                    operation_id,
                    "receiving",
                    "invalid_piece",
                    "sync piece was not valid zstd data",
                )
                .await;
            }
        }
    } else if chunk.data.len() == decoded_length {
        chunk.data.clone()
    } else {
        return reject(
            sink,
            operation_id,
            "receiving",
            "invalid_piece",
            "uncompressed piece length did not match its declaration",
        )
        .await;
    };
    if blake3::hash(&decoded).to_hex().as_str() != chunk.digest {
        return reject(
            sink,
            operation_id,
            "receiving",
            "piece_digest_mismatch",
            "sync piece bytes did not match their digest",
        )
        .await;
    }
    let sequence = chunk.sequence;
    let decoded_offset = chunk.decoded_offset;
    let decoded_length = chunk.decoded_length;
    let encoded_length = chunk.data.len();
    let digest = chunk.digest;
    let encoded = chunk.data;
    let group = identity.group_id.clone();
    let node = identity.node_id.clone();
    let operation = operation_id.to_owned();
    let result = catalog(handle, move |catalog| {
        catalog.append(
            &operation,
            &group,
            &node,
            &StoredChunk {
                sequence,
                decoded_offset,
                decoded_length,
                digest,
                encoded,
                compression: chunk.compression,
            },
        )
    })
    .await;
    match result {
        Ok(operation) => {
            crash_point("piece_after_fsync");
            send(
                sink,
                operation_id,
                server_frame::Body::Checkpoint(Checkpoint {
                    sequence,
                    decoded_offset: operation.decoded_offset,
                }),
            )
            .await?;
            send(
                sink,
                operation_id,
                server_frame::Body::Credit(Credit {
                    additional_bytes: u32::try_from(encoded_length).unwrap_or(u32::MAX),
                }),
            )
            .await
        }
        Err(error) => {
            send(
                sink,
                operation_id,
                server_frame::Body::Rejected(Failed {
                    phase: "receiving".into(),
                    code: "piece_rejected".into(),
                    commit_state: CommitState::NotCommitted as i32,
                    retryable: error.error.retryable,
                    recovery_action: "resume from the last acknowledged checkpoint".into(),
                    message: error.error.message,
                }),
            )
            .await
        }
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one transaction boundary: receipt verification, silver commit and outcome"
)]
async fn handle_finish(
    sink: &mut Sink,
    app: &App,
    identity: &Identity,
    handle: &CatalogHandle,
    operation_id: &str,
    finish: FinishUpload,
) -> Result<(), AppError> {
    let group = identity.group_id.clone();
    let node = identity.node_id.clone();
    let operation = operation_id.to_owned();
    let manifest = finish.manifest_digest;
    let result = catalog(handle, {
        let manifest = manifest.clone();
        move |catalog| catalog.finish(&operation, &group, &node, &manifest)
    })
    .await;
    let (operation, pieces) = match result {
        Ok(receipt) => receipt,
        Err(error) => {
            send(
                sink,
                operation_id,
                server_frame::Body::Rejected(Failed {
                    phase: "receipt".into(),
                    code: "receipt_rejected".into(),
                    commit_state: CommitState::NotCommitted as i32,
                    retryable: error.error.retryable,
                    recovery_action: "resume and resend the missing pieces".into(),
                    message: error.error.message,
                }),
            )
            .await?;
            return Ok(());
        }
    };
    if operation.state == catalog::State::Published {
        return send(
            sink,
            operation_id,
            server_frame::Body::Outcome(outcome(&operation, CommitState::Committed)),
        )
        .await;
    }
    match operation.kind.as_str() {
        "publication" => {}
        "backfill_manifest" => {
            return finish_manifest(
                sink,
                app,
                identity,
                handle,
                operation_id,
                &operation,
                &pieces,
            )
            .await;
        }
        "connector" => {
            return finish_connector(
                sink,
                app,
                identity,
                handle,
                operation_id,
                &operation,
                &pieces,
            )
            .await;
        }
        "attachment" => {
            return finish_attachment(
                sink,
                app,
                identity,
                handle,
                operation_id,
                &operation,
                &pieces,
            )
            .await;
        }
        _ => {
            return reject(
                sink,
                operation_id,
                "validating",
                "unsupported_operation",
                "durable operation kind is not supported",
            )
            .await;
        }
    }
    let max_decoded = usize::try_from(app.store.max_node_bytes()).unwrap_or(usize::MAX);
    let Ok(events) = wire::decode_publication(&pieces, max_decoded) else {
        return reject(
            sink,
            operation_id,
            "validating",
            "invalid_publication",
            "publication bytes were not valid Protobuf records",
        )
        .await;
    };
    if wire::record_count(&events) != operation.record_count {
        return reject(
            sink,
            operation_id,
            "validating",
            "record_count_mismatch",
            "publication record count did not match its manifest",
        )
        .await;
    }
    let Ok((start, backfill_id)) = parse_start(&operation.start_json) else {
        return reject(
            sink,
            operation_id,
            "validating",
            "invalid_publication_metadata",
            "publication metadata is not valid",
        )
        .await;
    };
    let expected_revision = start.revision;
    let mut backfill_commit = None;
    let (start, events) = match backfill_id {
        Some(backfill_id) => {
            let prepared =
                match prepare_backfill(app, identity, handle, &backfill_id, start, events).await {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        return reject(
                            sink,
                            operation_id,
                            "validating",
                            "backfill_rejected",
                            &error.error.message,
                        )
                        .await;
                    }
                };
            match prepared {
                BackfillPreparation::Superseded { members, revision } => {
                    // Every record was superseded by newer live state. Silver
                    // needs no write; the members are proven by that evidence.
                    let group = identity.group_id.clone();
                    let node = identity.node_id.clone();
                    let covered = catalog(handle, {
                        let backfill_id = backfill_id.clone();
                        move |catalog| catalog.cover(&backfill_id, &group, &node, &members)
                    })
                    .await;
                    if let Err(error) = covered {
                        return send(
                            sink,
                            operation_id,
                            server_frame::Body::Failed(Failed {
                                phase: "coverage".into(),
                                code: "coverage_failed".into(),
                                commit_state: CommitState::Committed as i32,
                                retryable: true,
                                recovery_action: "resend FinishUpload to record coverage".into(),
                                message: error.error.message,
                            }),
                        )
                        .await;
                    }
                    let receipt = silver_store::receipt_digest(&manifest, revision);
                    let operation_id_owned = operation_id.to_owned();
                    let receipt_for_state = receipt.clone();
                    let _ = catalog(handle, move |catalog| {
                        catalog.published(&operation_id_owned, revision, &receipt_for_state)
                    })
                    .await;
                    send(
                        sink,
                        operation_id,
                        server_frame::Body::SilverCommitted(SilverCommitted {
                            revision,
                            receipt_digest: receipt,
                        }),
                    )
                    .await?;
                    return send(
                        sink,
                        operation_id,
                        server_frame::Body::Published(Published {
                            revision,
                            views_current: true,
                            coverage_through: expected_revision,
                            connector: None,
                        }),
                    )
                    .await;
                }
                BackfillPreparation::Apply {
                    start,
                    events,
                    members,
                } => {
                    backfill_commit = Some(BackfillCommit {
                        backfill_id,
                        members,
                    });
                    (start, events)
                }
            }
        }
        None => (start, events),
    };
    let Ok(file) = write_events(start, &events) else {
        return reject(
            sink,
            operation_id,
            "validating",
            "invalid_publication",
            "publication could not be staged",
        )
        .await;
    };
    let (silver_tx, mut silver_rx) = tokio::sync::mpsc::channel::<i64>(1);
    let work = commit_publication(
        app,
        identity,
        handle,
        &operation,
        &events,
        CommitInput {
            file,
            backfill: backfill_commit,
            silver: Some(silver_tx),
        },
    );
    tokio::pin!(work);
    // Report the atomic silver+receipt commit as soon as it lands, before
    // certification starts. A failed certification still leaves this frame sent.
    let result = loop {
        tokio::select! {
            signal = silver_rx.recv() => {
                if let Some(revision) = signal {
                    send(
                        sink,
                        operation_id,
                        server_frame::Body::SilverCommitted(SilverCommitted {
                            revision,
                            receipt_digest: silver_store::receipt_digest(&manifest, revision),
                        }),
                    )
                    .await?;
                }
            }
            result = &mut work => break result,
        }
    };
    match result {
        Ok(indexed) => {
            send(
                sink,
                operation_id,
                server_frame::Body::Published(Published {
                    revision: indexed.revision,
                    views_current: true,
                    coverage_through: expected_revision,
                    connector: None,
                }),
            )
            .await?;
            let group = identity.group_id.clone();
            let node = identity.node_id.clone();
            let _ = catalog(handle, move |catalog| {
                catalog.prune(&group, &node, PRUNE_RECEIPTS)
            })
            .await;
            Ok(())
        }
        Err(failure) => {
            if failure.capacity {
                return send(
                    sink,
                    operation_id,
                    server_frame::Body::Failed(Failed {
                        phase: "waiting".into(),
                        code: "capacity_unavailable".into(),
                        commit_state: CommitState::Unknown as i32,
                        retryable: true,
                        recovery_action: "retry after shared activity drains".into(),
                        message: failure.error.error.message,
                    }),
                )
                .await;
            }
            send(
                sink,
                operation_id,
                server_frame::Body::Failed(Failed {
                    phase: "applying".into(),
                    code: failure.code.into(),
                    commit_state: if failure.committed {
                        CommitState::Committed as i32
                    } else {
                        CommitState::NotCommitted as i32
                    },
                    retryable: true,
                    recovery_action: "reconnect and resume to verify the durable outcome".into(),
                    message: failure.error.error.message,
                }),
            )
            .await
        }
    }
}

/// A commit that failed after reserving capacity. `committed` distinguishes a
/// durable silver commit with stale views from no commit at all.
struct CommitFailure {
    error: AppError,
    committed: bool,
    capacity: bool,
    code: &'static str,
}

impl CommitFailure {
    fn new(error: AppError, committed: bool) -> Self {
        Self {
            error,
            committed,
            capacity: false,
            code: "publication_failed",
        }
    }
}

/// Commit one publication and record its receipt. Used by the socket handler,
/// which receives the committed revision on `silver`, and by startup recovery.
struct CommitInput {
    file: tempfile::NamedTempFile,
    backfill: Option<BackfillCommit>,
    silver: Option<tokio::sync::mpsc::Sender<i64>>,
}

#[allow(
    clippy::too_many_lines,
    reason = "reserve, atomic silver commit, coverage evidence, certification and receipt"
)]
async fn commit_publication(
    app: &App,
    identity: &Identity,
    handle: &CatalogHandle,
    operation: &catalog::Operation,
    events: &[Event],
    input: CommitInput,
) -> Result<crate::shared::protocol::Indexed, CommitFailure> {
    let CommitInput {
        file,
        backfill,
        silver,
    } = input;
    let progress = app.activity.start(
        &identity.group_id,
        &identity.node_id,
        crate::shared::activity::Kind::Publication,
    );
    let permit = match admission::reserve(&app.uploads, &app.waiters, &progress).await {
        Ok(permit) => permit,
        Err(error) => {
            return Err(CommitFailure {
                error,
                committed: false,
                capacity: true,
                code: "capacity_unavailable",
            });
        }
    };
    let store = app.store.clone();
    let scope = identity.clone();
    let work_progress = progress.clone();
    let catalog_handle = handle.clone();
    let operation_id = operation.operation_id.clone();
    let manifest_digest = operation.manifest_digest.clone();
    let conversations = conversation_ids(events);
    let outcome = admission::run(permit, async move {
        let outcome: Result<crate::shared::protocol::Indexed, CommitFailure> = async {
            let receipt = silver_store::SyncReceipt {
                operation_id: operation_id.clone(),
                manifest_digest: manifest_digest.clone(),
            };
            // Phase one: atomic silver history and receipt.
            let committed = match store
                .commit_silver(
                    &scope,
                    file,
                    work_progress,
                    Some(&receipt),
                    backfill.is_some(),
                )
                .await
            {
                Ok(committed) => committed,
                Err(error) => {
                    // Silver did not commit; the operation stays terminally
                    // failed until the client retries it.
                    let operation_id = operation_id.clone();
                    let _ = catalog(&catalog_handle, move |catalog| {
                        catalog.set_state(&operation_id, catalog::State::Failed, None, None)
                    })
                    .await;
                    return Err(CommitFailure::new(error, false));
                }
            };
            let revision = committed.revision();
            crash_point("after_silver_commit");
            if let Some(backfill) = backfill {
                let group = scope.group_id.clone();
                let node = scope.node_id.clone();
                let covered = catalog(&catalog_handle, move |catalog| {
                    catalog.cover(&backfill.backfill_id, &group, &node, &backfill.members)
                })
                .await;
                if let Err(error) = covered {
                    set_views_pending(&catalog_handle, &operation_id, revision).await;
                    return Err(CommitFailure {
                        error,
                        committed: true,
                        capacity: false,
                        code: "coverage_failed",
                    });
                }
            } else if !conversations.is_empty()
                && let Err(error) =
                    record_live_evidence(&scope, &catalog_handle, conversations, revision).await
            {
                // The conversation committed but its ordering evidence did not.
                // Fail the receipt so a retry records it rather than letting a
                // later baseline overwrite the live state.
                set_views_pending(&catalog_handle, &operation_id, revision).await;
                return Err(CommitFailure {
                    error,
                    committed: true,
                    capacity: false,
                    code: "coverage_failed",
                });
            }
            // The canonical commit is durable; report it before certification.
            if let Some(silver) = silver {
                let _ = silver.send(revision).await;
            }
            // Phase two: views writes and certification.
            let indexed = match store.certify_views(committed).await {
                Ok(indexed) => indexed,
                Err(error) => {
                    set_views_pending(&catalog_handle, &operation_id, revision).await;
                    return Err(CommitFailure::new(error, true));
                }
            };
            let receipt = silver_store::receipt_digest(&manifest_digest, indexed.revision);
            let _ = catalog(&catalog_handle, move |catalog| {
                catalog.published(&operation_id, indexed.revision, &receipt)
            })
            .await;
            crash_point("after_published");
            Ok(indexed)
        }
        .await;
        Ok::<_, AppError>(outcome)
    })
    .await;
    match outcome {
        Ok(outcome) => outcome,
        Err(error) => Err(CommitFailure::new(error, false)),
    }
}

async fn set_views_pending(handle: &CatalogHandle, operation_id: &str, revision: i64) {
    let operation_id = operation_id.to_owned();
    let _ = catalog(handle, move |catalog| {
        catalog.set_state(
            &operation_id,
            catalog::State::ViewsPending,
            Some(revision),
            None,
        )
    })
    .await;
}

/// A committed baseline batch: which members it proves and its backfill id.
struct BackfillCommit {
    backfill_id: String,
    members: Vec<(String, String)>,
}

/// Record committed live conversations against every active baseline, so a
/// later baseline batch cannot overwrite them and their members are covered.
async fn record_live_evidence(
    identity: &Identity,
    handle: &CatalogHandle,
    conversations: Vec<String>,
    revision: i64,
) -> Result<(), AppError> {
    let group = identity.group_id.clone();
    let node = identity.node_id.clone();
    catalog(handle, move |catalog| {
        let backfills = catalog.active_backfills(&group, &node)?;
        for backfill in &backfills {
            catalog.record_live(backfill, &group, &node, &conversations, revision)?;
        }
        Ok(())
    })
    .await
}

fn conversation_ids(events: &[Event]) -> Vec<String> {
    let mut ids = std::collections::BTreeSet::new();
    for event in events {
        match event {
            Event::Batch(protocol::Batch::Conversations { records }) => {
                ids.extend(records.iter().map(|record| record.id.clone()));
            }
            Event::Batch(protocol::Batch::Tombstones { records }) => {
                ids.extend(records.iter().map(|record| record.conversation_id.clone()));
            }
            Event::Batch(protocol::Batch::Messages { records }) => {
                ids.extend(records.iter().map(|record| record.conversation_id.clone()));
            }
            Event::Remove { .. } | Event::Start(_) | Event::Finish => {}
        }
    }
    ids.into_iter().collect()
}

#[derive(serde::Deserialize)]
struct WrappedStart {
    start: Event,
    backfill_id: String,
}

#[derive(serde::Deserialize)]
struct ManifestStart {
    backfill_id: String,
}

#[derive(serde::Deserialize)]
struct ConnectorStart {
    source_id: String,
    batch_id: String,
    expected_revision: String,
}

#[derive(serde::Deserialize)]
struct AttachmentStart {
    message_id: String,
    attachment: Metadata,
}

fn parse_start(json: &str) -> Result<(protocol::Start, Option<String>), AppError> {
    if let Ok(Event::Start(start)) = serde_json::from_str::<Event>(json) {
        return Ok((start, None));
    }
    let wrapped: WrappedStart = serde_json::from_str(json)
        .map_err(|_| AppError::internal("invalid retained publication metadata"))?;
    let Event::Start(start) = wrapped.start else {
        return Err(AppError::internal("invalid retained publication metadata"));
    };
    Ok((start, Some(wrapped.backfill_id)))
}

/// Baseline evidence carried by one committed backfill batch. Member digests
/// are transfer-level content identities, not canonical silver fingerprints.
fn covered_members(events: &[Event]) -> Result<Vec<(String, String)>, AppError> {
    let mut members = Vec::new();
    for event in events {
        match event {
            Event::Batch(protocol::Batch::Conversations { records }) => {
                for record in records {
                    members.push((record.id.clone(), wire::member_digest(record)?));
                }
            }
            Event::Batch(protocol::Batch::Tombstones { records }) => {
                for record in records {
                    members.push((
                        record.conversation_id.clone(),
                        wire::tombstone_member_digest(&record.conversation_id),
                    ));
                }
            }
            Event::Batch(protocol::Batch::Messages { .. })
            | Event::Remove { .. }
            | Event::Start(_)
            | Event::Finish => {}
        }
    }
    Ok(members)
}

/// Outcome of applying per-conversation ordering to a baseline batch.
enum BackfillPreparation {
    /// A newer live publication covers every record in this batch.
    Superseded {
        members: Vec<(String, String)>,
        revision: i64,
    },
    /// Some conversations are still baseline-only; apply them at the current
    /// node revision so silver's node-level ordering stays monotonic.
    Apply {
        start: protocol::Start,
        events: Vec<Event>,
        members: Vec<(String, String)>,
    },
}

/// Drop records superseded by a newer live publication and rebase the rest.
async fn prepare_backfill(
    app: &App,
    identity: &Identity,
    handle: &CatalogHandle,
    backfill_id: &str,
    start: protocol::Start,
    events: Vec<Event>,
) -> Result<BackfillPreparation, AppError> {
    let members = covered_members(&events)?;
    let revision = start.revision.unwrap_or_default();
    let group = identity.group_id.clone();
    let node = identity.node_id.clone();
    let backfill = backfill_id.to_owned();
    let stale = catalog(handle, move |catalog| {
        catalog.superseded(&backfill, &group, &node, revision)
    })
    .await?;
    let events = filter_superseded(events, &stale);
    let head = app.store.head_revision(identity).await?;
    if events.is_empty() {
        return Ok(BackfillPreparation::Superseded {
            members,
            revision: head,
        });
    }
    let mut start = start;
    // Baseline state merges into current history; it is not a node revision
    // rollback. Per-conversation ordering already removed stale records.
    start.revision = Some(head.saturating_add(1));
    start.base_revision = Some(head);
    Ok(BackfillPreparation::Apply {
        start,
        events,
        members,
    })
}

/// Remove conversation records, their messages and their tombstones for
/// conversations a newer live publication already owns.
fn filter_superseded(events: Vec<Event>, stale: &[String]) -> Vec<Event> {
    let stale = stale
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    if stale.is_empty() {
        return events;
    }
    events
        .into_iter()
        .filter_map(|event| match event {
            Event::Batch(protocol::Batch::Conversations { mut records }) => {
                records.retain(|record| !stale.contains(record.id.as_str()));
                (!records.is_empty())
                    .then_some(Event::Batch(protocol::Batch::Conversations { records }))
            }
            Event::Batch(protocol::Batch::Messages { mut records }) => {
                records.retain(|record| !stale.contains(record.conversation_id.as_str()));
                (!records.is_empty()).then_some(Event::Batch(protocol::Batch::Messages { records }))
            }
            Event::Batch(protocol::Batch::Tombstones { mut records }) => {
                records.retain(|record| !stale.contains(record.conversation_id.as_str()));
                (!records.is_empty())
                    .then_some(Event::Batch(protocol::Batch::Tombstones { records }))
            }
            other => Some(other),
        })
        .collect()
}

/// Decode, install and durably record a baseline manifest. Shared by the
/// socket handler and startup recovery.
async fn apply_manifest(
    app: &App,
    identity: &Identity,
    handle: &CatalogHandle,
    operation: &catalog::Operation,
    pieces: &[wire::Piece],
) -> Result<catalog::Backfill, AppError> {
    let max_decoded = usize::try_from(app.store.max_node_bytes()).unwrap_or(usize::MAX);
    let bytes = wire::decode_bytes(pieces, max_decoded)
        .map_err(|_| AppError::usage("backfill manifest bytes were not valid"))?;
    let limit = usize::try_from(operation.record_count).unwrap_or(usize::MAX);
    let members = wire::decode_backfill_manifest(&bytes, limit)
        .map_err(|_| AppError::usage("backfill manifest did not match its member count"))?;
    let start: ManifestStart = serde_json::from_str(&operation.start_json)
        .map_err(|_| AppError::internal("backfill manifest metadata is not valid"))?;
    let group = identity.group_id.clone();
    let node = identity.node_id.clone();
    let backfill_id = start.backfill_id;
    let stored = catalog(handle, move |catalog| {
        catalog.store_manifest(&backfill_id, &group, &node, &members)
    })
    .await?;
    let operation_id = operation.operation_id.clone();
    let digest = operation.manifest_digest.clone();
    let _ = catalog(handle, move |catalog| {
        catalog.completed(&operation_id, &digest)
    })
    .await;
    Ok(stored)
}

async fn finish_manifest(
    sink: &mut Sink,
    app: &App,
    identity: &Identity,
    handle: &CatalogHandle,
    operation_id: &str,
    operation: &catalog::Operation,
    pieces: &[wire::Piece],
) -> Result<(), AppError> {
    match apply_manifest(app, identity, handle, operation, pieces).await {
        Ok(backfill) => {
            send(
                sink,
                operation_id,
                server_frame::Body::Progress(Progress {
                    phase: "baseline_manifest".into(),
                    completed: backfill.total_conversations,
                    total: backfill.total_conversations,
                }),
            )
            .await?;
            send(
                sink,
                operation_id,
                server_frame::Body::Accepted(Accepted { credit_bytes: 0 }),
            )
            .await
        }
        Err(error) => {
            reject(
                sink,
                operation_id,
                "validating",
                "invalid_manifest",
                &error.error.message,
            )
            .await
        }
    }
}

/// Validate connector bytes against their begin frame, commit silver, and
/// record transport state. Shared by the socket handler and recovery.
async fn apply_connector(
    app: &App,
    identity: &Identity,
    handle: &CatalogHandle,
    operation: &catalog::Operation,
    pieces: &[wire::Piece],
) -> Result<crate::connectors::Receipt, AppError> {
    let start: ConnectorStart = serde_json::from_str(&operation.start_json)
        .map_err(|_| AppError::internal("connector batch metadata is not valid"))?;
    let max_decoded = usize::try_from(app.store.max_node_bytes()).unwrap_or(usize::MAX);
    let bytes = wire::decode_bytes(pieces, max_decoded)
        .map_err(|_| AppError::usage("connector batch bytes were not valid"))?;
    let batch =
        Batch::decode(&bytes).map_err(|_| AppError::usage("connector batch JSON is not valid"))?;
    if batch.source_id.as_str() != start.source_id
        || batch.batch_id.as_str() != start.batch_id
        || batch.expected_revision.to_string() != start.expected_revision
    {
        return Err(AppError::usage(
            "connector batch identity does not match its begin frame",
        ));
    }
    let receipt = app.store.ingest_connector(identity, batch).await?;
    let revision = i64::try_from(receipt.silver_revision).unwrap_or(i64::MAX);
    let receipt_digest = blake3::hash(&bytes).to_hex().to_string();
    let operation_id = operation.operation_id.clone();
    let digest = receipt_digest.clone();
    let _ = catalog(handle, move |catalog| {
        catalog.published(&operation_id, revision, &digest)
    })
    .await;
    Ok(receipt)
}

async fn finish_connector(
    sink: &mut Sink,
    app: &App,
    identity: &Identity,
    handle: &CatalogHandle,
    operation_id: &str,
    operation: &catalog::Operation,
    pieces: &[wire::Piece],
) -> Result<(), AppError> {
    let receipt = match apply_connector(app, identity, handle, operation, pieces).await {
        Ok(receipt) => receipt,
        Err(error) => {
            return send(
                sink,
                operation_id,
                server_frame::Body::Failed(Failed {
                    phase: "applying".into(),
                    code: "connector_failed".into(),
                    commit_state: CommitState::Unknown as i32,
                    retryable: true,
                    recovery_action: "reconnect and resume to verify the durable outcome".into(),
                    message: error.error.message,
                }),
            )
            .await;
        }
    };
    let revision = i64::try_from(receipt.silver_revision).unwrap_or(i64::MAX);
    let receipt_digest = catalog(handle, {
        let operation_id = operation_id.to_owned();
        let group = identity.group_id.clone();
        let node = identity.node_id.clone();
        move |catalog| {
            catalog
                .select(&operation_id, &group, &node)
                .map(|operation| operation.receipt_digest.unwrap_or_default())
        }
    })
    .await
    .unwrap_or_default();
    send(
        sink,
        operation_id,
        server_frame::Body::SilverCommitted(SilverCommitted {
            revision,
            receipt_digest: receipt_digest.clone(),
        }),
    )
    .await?;
    send(
        sink,
        operation_id,
        server_frame::Body::Published(Published {
            revision,
            views_current: receipt.views_ready,
            coverage_through: Some(revision),
            connector: Some(ConnectorReceipt {
                source_id: receipt.source_id.as_str().to_owned(),
                batch_id: receipt.batch_id.as_str().to_owned(),
                source_revision: receipt.source_revision,
                silver_revision: receipt.silver_revision,
            }),
        }),
    )
    .await
}

/// Authorize and durably store attachment bytes. Shared by the socket handler
/// and recovery.
async fn apply_attachment(
    app: &App,
    identity: &Identity,
    handle: &CatalogHandle,
    operation: &catalog::Operation,
    pieces: &[wire::Piece],
) -> Result<String, AppError> {
    let start: AttachmentStart = serde_json::from_str(&operation.start_json)
        .map_err(|_| AppError::internal("attachment metadata is not valid"))?;
    let max_decoded = crate::attachments::MAX_BYTES
        .min(usize::try_from(app.store.max_node_bytes()).unwrap_or(usize::MAX));
    let bytes = wire::decode_bytes(pieces, max_decoded)
        .map_err(|_| AppError::usage("attachment bytes were not valid"))?;
    let digest = start
        .attachment
        .digest
        .clone()
        .ok_or_else(|| AppError::usage("attachment digest is missing"))?;
    let request = crate::attachments::Request {
        node_id: identity.node_id.clone(),
        message_id: start.message_id.clone(),
        attachment_id: Some(start.attachment.id.clone()),
    };
    let known = super::attachments::known(app, identity, &request)
        .await
        .map_err(|super::ApiError(_, error)| error)?;
    if !known.iter().any(|metadata| metadata == &start.attachment) {
        return Err(AppError::usage(
            "upload is not linked to this node's published message",
        ));
    }
    let root = super::attachments::directory(app, identity, &identity.node_id)
        .ok_or_else(|| AppError::usage("shared attachment storage is disabled"))?;
    let metadata = start.attachment;
    let write_metadata = metadata.clone();
    crate::blocking(move || crate::attachments::write_bytes(&root, &write_metadata, &bytes))
        .await?;
    let operation_id = operation.operation_id.clone();
    let receipt = digest.clone();
    let _ = catalog(handle, move |catalog| {
        catalog.completed(&operation_id, &receipt)
    })
    .await;
    Ok(digest)
}

async fn finish_attachment(
    sink: &mut Sink,
    app: &App,
    identity: &Identity,
    handle: &CatalogHandle,
    operation_id: &str,
    operation: &catalog::Operation,
    pieces: &[wire::Piece],
) -> Result<(), AppError> {
    match apply_attachment(app, identity, handle, operation, pieces).await {
        Ok(digest) => {
            send(
                sink,
                operation_id,
                server_frame::Body::AttachmentStored(AttachmentStored { digest }),
            )
            .await
        }
        Err(error) => {
            reject(
                sink,
                operation_id,
                "applying",
                "attachment_rejected",
                &error.error.message,
            )
            .await
        }
    }
}

async fn handle_finish_backfill(
    sink: &mut Sink,
    handle: &CatalogHandle,
    identity: &Identity,
    operation_id: &str,
    finish: FinishBackfill,
) -> Result<(), AppError> {
    let group = identity.group_id.clone();
    let node = identity.node_id.clone();
    let result = catalog(handle, move |catalog| {
        catalog.finish_backfill(
            &finish.backfill_id,
            &group,
            &node,
            finish.baseline_revision,
            &finish.manifest_digest,
        )
    })
    .await;
    match result {
        Ok(backfill) => {
            send(
                sink,
                operation_id,
                server_frame::Body::BackfillComplete(BackfillComplete {
                    backfill_id: backfill.backfill_id,
                    covered_conversations: backfill.covered,
                }),
            )
            .await
        }
        Err(error) => {
            send(
                sink,
                operation_id,
                server_frame::Body::Rejected(Failed {
                    phase: "backfill".into(),
                    code: "backfill_incomplete".into(),
                    commit_state: CommitState::NotCommitted as i32,
                    retryable: true,
                    recovery_action: "transfer remaining baseline members before finishing".into(),
                    message: error.error.message,
                }),
            )
            .await
        }
    }
}

/// Periodic reclamation of abandoned receiving transfers. Runs independently of
/// connections and restarts; the first sweep happens immediately.
pub(super) fn spawn_expiry(
    app: App,
    period: Duration,
    retention: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(period);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            match sweep_expired(&app, retention).await {
                Ok(0) => {}
                Ok(expired) => {
                    tracing::info!(expired, "expired abandoned synchronization transfers");
                }
                Err(error) => {
                    tracing::warn!(
                        kind = ?error.error.kind,
                        message = %error.error.message,
                        "synchronization expiry sweep failed"
                    );
                }
            }
        }
    })
}

/// Production sweep interval and retention used by the server.
pub(super) fn spawn_periodic_expiry(app: App) -> tokio::task::JoinHandle<()> {
    spawn_expiry(app, EXPIRY_SWEEP, INCOMPLETE_RETENTION)
}

/// How often completed-but-unapplied transfers are retried without a client.
const APPLY_RETRY: Duration = Duration::from_secs(30);

/// Retry durable apply work so a queued row never waits for a restart or a
/// reconnect. Idempotent with startup recovery and with a client resending
/// `FinishUpload`.
pub(super) fn spawn_apply_recovery(app: App) -> tokio::task::JoinHandle<()> {
    spawn_apply_retry(app, APPLY_RETRY)
}

/// Apply retry with an explicit period, used by tests.
pub(super) fn spawn_apply_retry(app: App, period: Duration) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(period);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Startup recovery already covered the initial tick.
        interval.tick().await;
        loop {
            interval.tick().await;
            match recover(&app).await {
                Ok(0) => {}
                Ok(resumed) => {
                    tracing::info!(resumed, "retried durable synchronization work");
                }
                Err(error) => {
                    tracing::warn!(
                        kind = ?error.error.kind,
                        message = %error.error.message,
                        "synchronization apply retry failed"
                    );
                }
            }
        }
    })
}

async fn sweep_expired(app: &App, retention: Duration) -> Result<usize, AppError> {
    let handle: CatalogHandle = app.catalog.clone();
    let cutoff =
        epoch_millis().saturating_sub(i64::try_from(retention.as_millis()).unwrap_or(i64::MAX));
    catalog(&handle, move |catalog| catalog.expire_incomplete(cutoff)).await
}

/// Resume operations that reached `queued_for_apply`, `applying` or
/// `views_pending` before the process stopped. Runs before the listener serves
/// requests, so a client resume cannot race the same operation.
///
/// A views failure leaves the row in `views_pending`; a retry re-applies the
/// retained pieces. Silver duplicate detection makes that a views-only repair,
/// and the receipt on the silver head proves whether history committed.
pub(super) async fn recover(app: &App) -> Result<usize, AppError> {
    let handle: CatalogHandle = app.catalog.clone();
    let cutoff = epoch_millis()
        .saturating_sub(i64::try_from(INCOMPLETE_RETENTION.as_millis()).unwrap_or(i64::MAX));
    let expired = catalog(&handle, move |catalog| catalog.expire_incomplete(cutoff)).await?;
    if expired > 0 {
        tracing::info!(expired, "expired abandoned synchronization transfers");
    }
    let pending = catalog(&handle, crate::shared::sync::catalog::Catalog::recoverable).await?;
    let mut resumed = 0usize;
    for row in pending {
        let Some(identity) = app
            .identities
            .values()
            .find(|identity| identity.group_id == row.group_id && identity.node_id == row.node_id)
            .cloned()
        else {
            tracing::warn!(
                operation = %row.operation_id,
                "durable synchronization operation belongs to an unconfigured node"
            );
            continue;
        };
        let loaded = catalog(&handle, {
            let operation_id = row.operation_id.clone();
            let group = row.group_id.clone();
            let node = row.node_id.clone();
            move |catalog| catalog.recover_pieces(&operation_id, &group, &node)
        })
        .await;
        let (operation, pieces) = match loaded {
            Ok(loaded) => loaded,
            Err(error) => {
                tracing::warn!(
                    operation = %row.operation_id,
                    message = %error.error.message,
                    "durable synchronization operation has no retained pieces"
                );
                continue;
            }
        };
        let result = match operation.kind.as_str() {
            "publication" => {
                recover_publication(app, &identity, &handle, &operation, &pieces).await
            }
            "backfill_manifest" => apply_manifest(app, &identity, &handle, &operation, &pieces)
                .await
                .map(|_| ()),
            "connector" => apply_connector(app, &identity, &handle, &operation, &pieces)
                .await
                .map(|_| ()),
            "attachment" => apply_attachment(app, &identity, &handle, &operation, &pieces)
                .await
                .map(|_| ()),
            other => {
                tracing::warn!(
                    operation = %row.operation_id,
                    kind = other,
                    "durable synchronization operation has an unknown kind"
                );
                continue;
            }
        };
        match result {
            Ok(()) => {
                resumed = resumed.saturating_add(1);
                tracing::info!(
                    operation = %row.operation_id,
                    kind = %row.kind,
                    "resumed durable synchronization operation"
                );
            }
            Err(error) => {
                tracing::warn!(
                    operation = %row.operation_id,
                    kind = %row.kind,
                    message = %error.error.message,
                    "durable synchronization operation could not be resumed"
                );
            }
        }
    }
    Ok(resumed)
}

async fn recover_publication(
    app: &App,
    identity: &Identity,
    handle: &CatalogHandle,
    operation: &catalog::Operation,
    pieces: &[wire::Piece],
) -> Result<(), AppError> {
    let max_decoded = usize::try_from(app.store.max_node_bytes()).unwrap_or(usize::MAX);
    let events = wire::decode_publication(pieces, max_decoded)
        .map_err(|_| AppError::internal("retained publication bytes are invalid"))?;
    if wire::record_count(&events) != operation.record_count {
        return Err(AppError::internal(
            "retained publication record count does not match its manifest",
        ));
    }
    let (start, backfill_id) = parse_start(&operation.start_json)?;
    let (start, events, backfill) = match backfill_id {
        Some(backfill_id) => {
            match prepare_backfill(app, identity, handle, &backfill_id, start, events).await? {
                BackfillPreparation::Superseded { members, revision } => {
                    let group = identity.group_id.clone();
                    let node = identity.node_id.clone();
                    let covered = catalog(handle, {
                        let backfill_id = backfill_id.clone();
                        move |catalog| catalog.cover(&backfill_id, &group, &node, &members)
                    })
                    .await?;
                    let _ = covered;
                    let receipt =
                        silver_store::receipt_digest(&operation.manifest_digest, revision);
                    let operation_id = operation.operation_id.clone();
                    let _ = catalog(handle, move |catalog| {
                        catalog.published(&operation_id, revision, &receipt)
                    })
                    .await;
                    return Ok(());
                }
                BackfillPreparation::Apply {
                    start,
                    events,
                    members,
                } => (
                    start,
                    events,
                    Some(BackfillCommit {
                        backfill_id,
                        members,
                    }),
                ),
            }
        }
        None => (start, events, None),
    };
    let file = write_events(start, &events)?;
    match commit_publication(
        app,
        identity,
        handle,
        operation,
        &events,
        CommitInput {
            file,
            backfill,
            silver: None,
        },
    )
    .await
    {
        Ok(_) => Ok(()),
        Err(failure) => Err(failure.error),
    }
}

fn write_events(
    start: protocol::Start,
    events: &[Event],
) -> Result<tempfile::NamedTempFile, AppError> {
    use std::io::Write;
    let file =
        tempfile::NamedTempFile::new().map_err(|_| AppError::internal("stage publication"))?;
    let mut writer = std::io::BufWriter::with_capacity(256 * 1024, file.as_file());
    let redaction = crate::redaction::Policy::new(false);
    crate::shared::write_event(&mut writer, &Event::Start(start), redaction)?;
    for event in events {
        crate::shared::write_event(&mut writer, event, redaction)?;
    }
    crate::shared::write_event(&mut writer, &Event::Finish, redaction)?;
    writer
        .flush()
        .map_err(|_| AppError::internal("stage publication"))?;
    drop(writer);
    Ok(file)
}

async fn handle_resume(
    sink: &mut Sink,
    handle: &CatalogHandle,
    identity: &Identity,
    operation_id: &str,
    resume: Resume,
) -> Result<(), AppError> {
    let group = identity.group_id.clone();
    let node = identity.node_id.clone();
    let operation = operation_id.to_owned();
    let result = catalog(handle, move |catalog| {
        let operation = catalog.select(&operation, &group, &node)?;
        if operation.manifest_digest != resume.manifest_digest {
            return Err(crate::shared::validation::conflict(
                "sync resume manifest does not match the retained operation",
            ));
        }
        Ok(operation)
    })
    .await;
    match result {
        Ok(operation) => {
            send(
                sink,
                operation_id,
                server_frame::Body::Outcome(outcome(
                    &operation,
                    match operation.state {
                        catalog::State::Published | catalog::State::ViewsPending => {
                            CommitState::Committed
                        }
                        catalog::State::Receiving
                        | catalog::State::Failed
                        | catalog::State::Cancelled
                        | catalog::State::Expired => CommitState::NotCommitted,
                        catalog::State::QueuedForApply | catalog::State::Applying => {
                            CommitState::Unknown
                        }
                    },
                )),
            )
            .await
        }
        Err(_) => {
            // An operation unknown to this server starts normally with Begin.
            send(
                sink,
                operation_id,
                server_frame::Body::Outcome(Outcome {
                    state: OutcomeState::Unknown as i32,
                    commit_state: CommitState::NotCommitted as i32,
                    checkpoint_sequence: 0,
                    revision: None,
                    receipt_digest: String::new(),
                }),
            )
            .await
        }
    }
}

async fn handle_cancel(
    sink: &mut Sink,
    handle: &CatalogHandle,
    identity: &Identity,
    operation_id: &str,
) -> Result<(), AppError> {
    let group = identity.group_id.clone();
    let node = identity.node_id.clone();
    let operation = operation_id.to_owned();
    let result = catalog(handle, move |catalog| {
        let operation = catalog.select(&operation, &group, &node)?;
        if operation.state == catalog::State::Published {
            return Ok(false);
        }
        catalog.cancel(&operation.operation_id)?;
        Ok(true)
    })
    .await;
    match result {
        Ok(true) => {
            send(
                sink,
                operation_id,
                server_frame::Body::Cancelled(Cancelled {}),
            )
            .await
        }
        Ok(false) => {
            send(
                sink,
                operation_id,
                server_frame::Body::TooLateToCancel(TooLateToCancel {}),
            )
            .await
        }
        Err(error) => {
            send(
                sink,
                operation_id,
                server_frame::Body::Rejected(Failed {
                    phase: "cancel".into(),
                    code: "cancel_rejected".into(),
                    commit_state: CommitState::NotCommitted as i32,
                    retryable: false,
                    recovery_action: String::new(),
                    message: error.error.message,
                }),
            )
            .await
        }
    }
}

async fn reject(
    sink: &mut Sink,
    operation_id: &str,
    phase: &str,
    code: &str,
    message: &str,
) -> Result<(), AppError> {
    send(
        sink,
        operation_id,
        server_frame::Body::Rejected(Failed {
            phase: phase.into(),
            code: code.into(),
            commit_state: CommitState::NotCommitted as i32,
            retryable: false,
            recovery_action: String::new(),
            message: message.into(),
        }),
    )
    .await
}

fn outcome(operation: &catalog::Operation, commit: CommitState) -> Outcome {
    Outcome {
        state: match operation.state {
            catalog::State::Receiving => OutcomeState::Receiving as i32,
            catalog::State::QueuedForApply => OutcomeState::QueuedForApply as i32,
            catalog::State::Applying => OutcomeState::Applying as i32,
            catalog::State::ViewsPending => OutcomeState::ViewsPending as i32,
            catalog::State::Published => OutcomeState::Published as i32,
            catalog::State::Cancelled => OutcomeState::Cancelled as i32,
            catalog::State::Failed => OutcomeState::Failed as i32,
            catalog::State::Expired => OutcomeState::Expired as i32,
        },
        commit_state: commit as i32,
        checkpoint_sequence: operation.next_sequence.saturating_sub(1),
        revision: operation.revision,
        receipt_digest: operation.receipt_digest.clone().unwrap_or_default(),
    }
}
