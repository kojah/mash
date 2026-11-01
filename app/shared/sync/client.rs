//! Native publication client for the authenticated `/v1/sync` WebSocket.
//!
//! Preparation still uses the existing consistent silver snapshot and NDJSON
//! spool. This module turns that sealed batch into typed Protobuf records,
//! compresses every piece with zstd, and resumes from the server's durable
//! checkpoint instead of restarting a whole upload.
use super::wire::{
    self, AttachmentStored, BeginAttachment, BeginBackfill, BeginPublication, ClientFrame,
    FinishBackfill, FinishUpload, Hello, PublicationMode, PublicationStart, Resume, ServerFrame,
    client_frame, server_frame,
};
use crate::AppError;
use crate::config::SharedConfig;
use crate::shared::protocol::{self, Event};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{self, Message, http::HeaderValue};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
const OPERATION_TIMEOUT: Duration = Duration::from_mins(15);

fn endpoint(api_url: &str) -> Result<String, AppError> {
    let url = api_url.trim_end_matches('/');
    let websocket = if let Some(rest) = url.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = url.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        return Err(AppError::configuration(
            "shared api_url must use http or https for WebSocket synchronization",
        ));
    };
    Ok(format!("{websocket}/v1/sync"))
}

fn token(config: &SharedConfig) -> Result<String, AppError> {
    let token = std::env::var(&config.token_env).map_err(|_| {
        AppError::configuration(format!("set {} to the node's API token", config.token_env))
    })?;
    // Header safety only: the server remains authoritative for token length and
    // compares the credential against its configured node tokens.
    if token.is_empty() || token.len() > 4096 || !token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(AppError::configuration(
            "API tokens require 1..4096 printable ASCII bytes",
        ));
    }
    Ok(token)
}

fn request(
    config: &SharedConfig,
    token: &str,
) -> Result<tungstenite::handshake::client::Request, AppError> {
    let mut request = endpoint(&config.api_url)?
        .into_client_request()
        .map_err(|_| AppError::shared("invalid shared synchronization endpoint"))?;
    let headers = request.headers_mut();
    headers.insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| AppError::configuration("API token is not a valid header value"))?,
    );
    for (name, value) in [
        ("x-mash-group", &config.group_id),
        ("x-mash-node", &config.node_id),
    ] {
        headers.insert(
            name,
            HeaderValue::from_str(value).map_err(|_| {
                AppError::configuration("shared node identity is not a header value")
            })?,
        );
    }
    Ok(request)
}

/// Read the sealed publication spool produced by the existing preparation path.
fn read_events(file: &tempfile::NamedTempFile) -> Result<(protocol::Start, Vec<Event>), AppError> {
    use std::io::BufRead;
    let lines = std::io::BufReader::new(file.reopen().map_err(AppError::io)?).lines();
    let mut start = None;
    let mut events = Vec::new();
    let mut finished = false;
    for line in lines {
        let line = line.map_err(|_| AppError::usage("invalid index stream"))?;
        let event: Event =
            serde_json::from_str(&line).map_err(|_| AppError::usage("invalid index event"))?;
        match event {
            Event::Start(input) => {
                if start.replace(input).is_some() || !events.is_empty() {
                    return Err(AppError::usage("duplicate index start"));
                }
            }
            Event::Finish => finished = true,
            event => {
                if start.is_none() || finished {
                    return Err(AppError::usage("index stream metadata is out of order"));
                }
                events.push(event);
            }
        }
    }
    let start = start.ok_or_else(|| AppError::usage("empty index stream"))?;
    if !finished {
        return Err(AppError::usage("index stream did not finish"));
    }
    Ok((start, events))
}

async fn send(
    sink: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    frame: &ClientFrame,
) -> Result<(), AppError> {
    let bytes =
        wire::encode_client(frame).map_err(|_| AppError::shared("sync frame is too large"))?;
    sink.send(Message::Binary(bytes.into()))
        .await
        .map_err(|_| AppError::shared("shared synchronization connection failed"))
}

async fn receive(
    stream: &mut futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
) -> Result<ServerFrame, AppError> {
    loop {
        let message = tokio::time::timeout(OPERATION_TIMEOUT, stream.next())
            .await
            .map_err(|_| AppError::shared("shared synchronization timed out"))?
            .ok_or_else(|| AppError::shared("shared synchronization closed unexpectedly"))?
            .map_err(|_| AppError::shared("shared synchronization connection failed"))?;
        match message {
            Message::Binary(bytes) => {
                return wire::decode_server(&bytes)
                    .map_err(|_| AppError::shared("invalid synchronization response"));
            }
            Message::Close(_) => {
                return Err(AppError::shared(
                    "shared synchronization closed before acknowledgement",
                ));
            }
            // Tungstenite answers pings; ignore unsolicited text and pongs.
            Message::Text(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
        }
    }
}

fn outcome_error(failed: &wire::Failed) -> AppError {
    let retryable = failed.retryable;
    let message = format!(
        "shared synchronization {} failed: {} ({}){}",
        failed.phase,
        failed.code,
        failed.message,
        if failed.recovery_action.is_empty() {
            String::new()
        } else {
            format!("; {}", failed.recovery_action)
        }
    );
    let mut error = AppError::shared(message);
    error.error.retryable = retryable;
    error
}

/// Upload one sealed publication batch through the typed WebSocket transport.
#[allow(
    clippy::too_many_lines,
    reason = "one resumable protocol session with explicit server outcomes"
)]
pub(crate) async fn upload(
    config: &SharedConfig,
    file: &tempfile::NamedTempFile,
) -> Result<protocol::Indexed, AppError> {
    upload_with_token(config, file, token(config)?).await
}

/// Same publisher with an explicit credential, so tests do not mutate process env.
/// Publisher sessions are reused across publications from the same process and
/// destination. Watch and MCP publish many small batches; the handshake would
/// otherwise cost more than the saved bytes.
const SESSION_IDLE: Duration = Duration::from_secs(30);

type SessionSink = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    Message,
>;
type SessionStream = futures_util::stream::SplitStream<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
>;
type SessionSlot = std::sync::Arc<tokio::sync::Mutex<Option<PublisherSession>>>;

struct PublisherSession {
    sink: SessionSink,
    stream: SessionStream,
    /// Remaining encoded-byte credit for the next operation.
    credit: usize,
    last_used: tokio::time::Instant,
}

static SESSIONS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, SessionSlot>>,
> = std::sync::OnceLock::new();

fn session_slots() -> &'static std::sync::Mutex<std::collections::HashMap<String, SessionSlot>> {
    SESSIONS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// One session per endpoint, node and database instance. A destination or
/// identity change must negotiate again rather than reuse another's stream.
fn session_key(config: &SharedConfig, instance: &str) -> String {
    format!(
        "{}\0{}\0{}\0{instance}",
        config.api_url.trim_end_matches('/'),
        config.group_id,
        config.node_id
    )
}

fn session_slot(key: &str) -> SessionSlot {
    let mut slots = session_slots()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    slots
        .entry(key.to_owned())
        .or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(None)))
        .clone()
}

impl PublisherSession {
    /// Connect and negotiate; the session is reusable until it idles out or an
    /// operation fails on it.
    async fn connect(config: &SharedConfig, token: &str, instance: &str) -> Result<Self, AppError> {
        let request = request(config, token)?;
        let (socket, _) =
            tokio::time::timeout(HANDSHAKE_TIMEOUT, tokio_tungstenite::connect_async(request))
                .await
                .map_err(|_| AppError::shared("shared synchronization handshake timed out"))?
                .map_err(|_| {
                    AppError::shared("could not connect to the shared synchronization endpoint")
                })?;
        let (mut sink, mut stream) = socket.split();
        send(
            &mut sink,
            &ClientFrame {
                operation_id: String::new(),
                body: Some(client_frame::Body::Hello(Hello {
                    protocol_version: wire::PROTOCOL_VERSION,
                    database_instance: instance.to_owned(),
                    zstd_publication_data: true,
                    max_decoded_bytes: wire::DEFAULT_MAX_DECODED_BYTES as u64,
                })),
            },
        )
        .await?;
        let welcome = match receive(&mut stream).await?.body {
            Some(server_frame::Body::Welcome(welcome))
                if welcome.protocol_version == wire::PROTOCOL_VERSION
                    && welcome.max_envelope_bytes > 0 =>
            {
                welcome
            }
            Some(server_frame::Body::Rejected(failed) | server_frame::Body::Failed(failed)) => {
                return Err(outcome_error(&failed));
            }
            _ => {
                return Err(AppError::shared(
                    "shared synchronization did not accept this protocol version",
                ));
            }
        };
        Ok(Self {
            sink,
            stream,
            credit: usize::try_from(welcome.credit_bytes).unwrap_or(0),
            last_used: tokio::time::Instant::now(),
        })
    }
}

/// Reuse the live session when it is fresh, otherwise negotiate a new one.
async fn ensure_session<'a>(
    slot: &'a mut Option<PublisherSession>,
    config: &SharedConfig,
    token: &str,
    instance: &str,
) -> Result<&'a mut PublisherSession, AppError> {
    if slot
        .as_ref()
        .is_some_and(|session| session.last_used.elapsed() > SESSION_IDLE)
    {
        *slot = None;
    }
    if slot.is_none() {
        *slot = Some(PublisherSession::connect(config, token, instance).await?);
    }
    slot.as_mut()
        .ok_or_else(|| AppError::internal("publisher session disappeared"))
}

#[allow(
    clippy::too_many_lines,
    reason = "one resumable protocol session with explicit server outcomes"
)]
pub(crate) async fn upload_with_token(
    config: &SharedConfig,
    file: &tempfile::NamedTempFile,
    token: String,
) -> Result<protocol::Indexed, AppError> {
    let (start, events) = read_events(file)?;
    let encoded = wire::encode_publication(&events, wire::DEFAULT_MAX_DECODED_BYTES)?;
    if encoded.manifest_digest.is_empty() {
        return Err(AppError::internal("publication manifest is empty"));
    }
    upload_encoded_with_token(config, token, &start, &encoded).await
}

/// Publish an already-encoded batch through the shared publisher session. The
/// spool-reading entry point above and the benchmarks both land here.
#[allow(
    clippy::too_many_lines,
    reason = "one resumable protocol session with explicit server outcomes"
)]
pub(crate) async fn upload_encoded_with_token(
    config: &SharedConfig,
    token: String,
    start: &protocol::Start,
    encoded: &wire::EncodedBatch,
) -> Result<protocol::Indexed, AppError> {
    let operation_id = format!("publication:{}", encoded.manifest_digest);
    let slot = session_slot(&session_key(config, &start.instance));
    let mut guard = slot.lock_owned().await;
    let reused = guard.is_some();
    let result = async {
        let session = ensure_session(&mut guard, config, &token, &start.instance).await?;
        publish_on_session(session, &operation_id, start, encoded).await
    }
    .await;
    match result {
        Ok(indexed) => Ok(indexed),
        Err(first) => {
            // The cached stream may have been closed by the peer; the operation
            // is resumable, so negotiate once more before surfacing an error.
            *guard = None;
            if !reused {
                return Err(first);
            }
            let retry = async {
                let session = ensure_session(&mut guard, config, &token, &start.instance).await?;
                publish_on_session(session, &operation_id, start, encoded).await
            }
            .await;
            if retry.is_err() {
                *guard = None;
            }
            retry
        }
    }
}

/// Run one publication on an established session.
#[allow(
    clippy::too_many_lines,
    reason = "one resumable protocol operation with explicit server outcomes"
)]
async fn publish_on_session(
    session: &mut PublisherSession,
    operation_id: &str,
    start: &protocol::Start,
    encoded: &wire::EncodedBatch,
) -> Result<protocol::Indexed, AppError> {
    let mut checkpoint = 0u64;

    // Ask for persisted state before resending. An unknown operation is normal.
    send(
        &mut session.sink,
        &ClientFrame {
            operation_id: operation_id.to_owned(),
            body: Some(client_frame::Body::Resume(Resume {
                manifest_digest: encoded.manifest_digest.clone(),
            })),
        },
    )
    .await?;
    match receive(&mut session.stream).await?.body {
        Some(server_frame::Body::Outcome(outcome)) => {
            if outcome.state == wire::OutcomeState::Published as i32 {
                return Ok(indexed(outcome.revision.unwrap_or_default()));
            }
            checkpoint = outcome.checkpoint_sequence;
            if outcome.state == wire::OutcomeState::Applying as i32
                || outcome.state == wire::OutcomeState::ViewsPending as i32
            {
                // The server owns the commit; re-begin with the same operation
                // and send FinishUpload so it can report the durable outcome.
                checkpoint = encoded.pieces.len().saturating_sub(1) as u64;
            }
        }
        Some(server_frame::Body::Credit(grant)) => {
            session.credit = session
                .credit
                .saturating_add(grant.additional_bytes as usize);
        }
        _ => {}
    }

    send(
        &mut session.sink,
        &ClientFrame {
            operation_id: operation_id.to_owned(),
            body: Some(client_frame::Body::BeginPublication(BeginPublication {
                start: Some(PublicationStart::from(start)),
                mode: if start.snapshot {
                    PublicationMode::ReplaceSnapshot as i32
                } else {
                    PublicationMode::Delta as i32
                },
                backfill_id: String::new(),
                source_revision: start.revision,
                journal_from_exclusive: None,
                journal_through_inclusive: None,
                manifest_digest: encoded.manifest_digest.clone(),
                decoded_bytes: encoded.decoded_bytes,
                record_count: encoded.record_count,
            })),
        },
    )
    .await?;
    loop {
        match receive(&mut session.stream).await?.body {
            Some(server_frame::Body::Accepted(accepted)) => {
                session.credit = session
                    .credit
                    .saturating_add(accepted.credit_bytes as usize);
                break;
            }
            Some(server_frame::Body::Outcome(outcome))
                if outcome.state == wire::OutcomeState::Published as i32 =>
            {
                return Ok(indexed(outcome.revision.unwrap_or_default()));
            }
            Some(server_frame::Body::Credit(grant)) => {
                session.credit = session
                    .credit
                    .saturating_add(grant.additional_bytes as usize);
            }
            Some(server_frame::Body::Rejected(failed) | server_frame::Body::Failed(failed)) => {
                return Err(outcome_error(&failed));
            }
            Some(_) => {}
            None => return Err(AppError::shared("empty synchronization response")),
        }
    }

    let mut next = usize::try_from(checkpoint)
        .ok()
        .filter(|checkpoint| *checkpoint > 0)
        .map_or(0, |checkpoint| checkpoint + 1);
    if next > encoded.pieces.len() {
        return Err(AppError::shared(
            "server resumption checkpoint exceeds the prepared batch",
        ));
    }
    let mut acknowledged = next.saturating_sub(1) as u64;
    let mut finish_sent = false;
    loop {
        while next < encoded.pieces.len() {
            let piece = &encoded.pieces[next];
            let size = piece.encoded.len();
            if size > session.credit {
                break;
            }
            send(
                &mut session.sink,
                &ClientFrame {
                    operation_id: operation_id.to_owned(),
                    body: Some(client_frame::Body::Chunk(wire::EncodedBatch::chunk(piece))),
                },
            )
            .await?;
            session.credit -= size;
            next += 1;
        }
        if next == encoded.pieces.len() && acknowledged + 1 >= next as u64 && !finish_sent {
            send(
                &mut session.sink,
                &ClientFrame {
                    operation_id: operation_id.to_owned(),
                    body: Some(client_frame::Body::FinishUpload(FinishUpload {
                        manifest_digest: encoded.manifest_digest.clone(),
                    })),
                },
            )
            .await?;
            finish_sent = true;
        }
        match receive(&mut session.stream).await?.body {
            Some(server_frame::Body::Credit(grant)) => {
                session.credit = session
                    .credit
                    .saturating_add(grant.additional_bytes as usize);
            }
            Some(server_frame::Body::Checkpoint(checkpoint)) => {
                acknowledged = acknowledged.max(checkpoint.sequence);
            }
            Some(server_frame::Body::Published(published)) => {
                session.last_used = tokio::time::Instant::now();
                return Ok(indexed(published.revision));
            }
            Some(server_frame::Body::Rejected(failed) | server_frame::Body::Failed(failed)) => {
                return Err(outcome_error(&failed));
            }
            Some(server_frame::Body::Cancelled(_)) => {
                return Err(AppError::shared(
                    "shared synchronization operation was cancelled",
                ));
            }
            Some(server_frame::Body::Outcome(outcome))
                if outcome.state == wire::OutcomeState::Published as i32 =>
            {
                session.last_used = tokio::time::Instant::now();
                return Ok(indexed(outcome.revision.unwrap_or_default()));
            }
            Some(_) => {}
            None => return Err(AppError::shared("empty synchronization response")),
        }
    }
}

/// Upload one attachment as bounded uncompressed pieces over `/v1/sync`.
pub(crate) async fn upload_attachment(
    config: &SharedConfig,
    message_id: &str,
    metadata: &crate::attachments::Metadata,
    bytes: &[u8],
) -> Result<(), AppError> {
    upload_attachment_with_token(config, token(config)?, message_id, metadata, bytes).await
}

/// Same transport with an explicit credential, so tests do not mutate process env.
#[allow(
    clippy::too_many_lines,
    reason = "one handshake, begin, chunk and acknowledgement sequence"
)]
pub(crate) async fn upload_attachment_with_token(
    config: &SharedConfig,
    token: String,
    message_id: &str,
    metadata: &crate::attachments::Metadata,
    bytes: &[u8],
) -> Result<(), AppError> {
    let mut pieces = Vec::new();
    let mut offset = 0u64;
    for (index, chunk) in bytes.chunks(wire::NORMAL_BATCH_TARGET.max(1)).enumerate() {
        pieces.push(wire::Piece {
            sequence: index as u64,
            decoded_offset: offset,
            decoded_length: chunk.len() as u64,
            digest: blake3::hash(chunk).to_hex().to_string(),
            encoded: chunk.to_vec(),
            // Attachments are binary and already compressed on disk.
            compression: wire::Compression::None as i32,
        });
        offset = offset.saturating_add(chunk.len() as u64);
    }
    if pieces.is_empty() {
        return Err(AppError::usage("attachment upload is empty"));
    }
    let entries = pieces
        .iter()
        .map(|piece| wire::ManifestPiece {
            sequence: piece.sequence,
            decoded_offset: piece.decoded_offset,
            decoded_length: piece.decoded_length,
            digest: &piece.digest,
        })
        .collect::<Vec<_>>();
    let manifest = wire::manifest_digest(bytes.len() as u64, 1, &entries);
    let operation_id = format!("attachment:{manifest}");
    let request = request(config, &token)?;
    let (socket, _) =
        tokio::time::timeout(HANDSHAKE_TIMEOUT, tokio_tungstenite::connect_async(request))
            .await
            .map_err(|_| AppError::shared("shared synchronization handshake timed out"))?
            .map_err(|_| {
                AppError::shared("could not connect to the shared synchronization endpoint")
            })?;
    let (mut sink, mut stream) = socket.split();
    send(
        &mut sink,
        &ClientFrame {
            operation_id: String::new(),
            body: Some(client_frame::Body::Hello(Hello {
                protocol_version: wire::PROTOCOL_VERSION,
                database_instance: config.node_id.clone(),
                zstd_publication_data: true,
                max_decoded_bytes: wire::DEFAULT_MAX_DECODED_BYTES as u64,
            })),
        },
    )
    .await?;
    let welcome = match stream.next().await {
        Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(bytes))) => {
            match wire::decode_server(&bytes)?.body {
                Some(server_frame::Body::Welcome(welcome)) => welcome,
                Some(server_frame::Body::Rejected(failed) | server_frame::Body::Failed(failed)) => {
                    return Err(outcome_error(&failed));
                }
                _ => {
                    return Err(AppError::shared(
                        "shared synchronization did not accept this protocol version",
                    ));
                }
            }
        }
        _ => return Err(AppError::shared("shared synchronization handshake failed")),
    };
    let mut credit = usize::try_from(welcome.credit_bytes).unwrap_or(0);
    send(
        &mut sink,
        &ClientFrame {
            operation_id: operation_id.clone(),
            body: Some(client_frame::Body::BeginAttachment(BeginAttachment {
                message_id: message_id.to_owned(),
                attachment: Some(wire::Attachment::from(metadata)),
                manifest_digest: manifest.clone(),
                decoded_bytes: bytes.len() as u64,
            })),
        },
    )
    .await?;
    let mut next = 0usize;
    loop {
        match receive(&mut stream).await?.body {
            Some(server_frame::Body::Accepted(accepted)) => {
                credit = credit.saturating_add(accepted.credit_bytes as usize);
                break;
            }
            Some(server_frame::Body::Checkpoint(checkpoint)) => {
                next = usize::try_from(checkpoint.sequence)
                    .unwrap_or(usize::MAX)
                    .saturating_add(1);
            }
            Some(server_frame::Body::Outcome(outcome))
                if outcome.state == wire::OutcomeState::Published as i32 =>
            {
                return Ok(());
            }
            Some(server_frame::Body::Credit(grant)) => {
                credit = credit.saturating_add(grant.additional_bytes as usize);
            }
            Some(server_frame::Body::Rejected(failed) | server_frame::Body::Failed(failed)) => {
                return Err(outcome_error(&failed));
            }
            Some(_) => {}
            None => return Err(AppError::shared("empty synchronization response")),
        }
    }
    let mut finish_sent = false;
    loop {
        while next < pieces.len() {
            let piece = &pieces[next];
            if piece.encoded.len() > credit {
                break;
            }
            let chunk = wire::Chunk {
                sequence: piece.sequence,
                decoded_offset: piece.decoded_offset,
                compression: wire::Compression::None as i32,
                decoded_length: piece.decoded_length,
                digest: piece.digest.clone(),
                data: piece.encoded.clone(),
            };
            send(
                &mut sink,
                &ClientFrame {
                    operation_id: operation_id.clone(),
                    body: Some(client_frame::Body::Chunk(chunk)),
                },
            )
            .await?;
            credit -= piece.encoded.len();
            next += 1;
        }
        if next == pieces.len() && !finish_sent {
            send(
                &mut sink,
                &ClientFrame {
                    operation_id: operation_id.clone(),
                    body: Some(client_frame::Body::FinishUpload(FinishUpload {
                        manifest_digest: manifest.clone(),
                    })),
                },
            )
            .await?;
            finish_sent = true;
        }
        match receive(&mut stream).await?.body {
            Some(server_frame::Body::AttachmentStored(AttachmentStored { .. })) => return Ok(()),
            Some(server_frame::Body::Credit(grant)) => {
                credit = credit.saturating_add(grant.additional_bytes as usize);
            }
            Some(server_frame::Body::Outcome(outcome))
                if outcome.state == wire::OutcomeState::Published as i32 =>
            {
                return Ok(());
            }
            Some(server_frame::Body::Rejected(failed) | server_frame::Body::Failed(failed)) => {
                return Err(outcome_error(&failed));
            }
            Some(_) => {}
            None => return Err(AppError::shared("empty synchronization response")),
        }
    }
}

/// Progressive-backfill manifest membership derived from one frozen batch.
fn backfill_members(events: &[Event]) -> Result<Vec<wire::BackfillMember>, AppError> {
    let mut members = std::collections::BTreeMap::new();
    for event in events {
        match event {
            Event::Batch(protocol::Batch::Conversations { records }) => {
                for record in records {
                    members.insert(record.id.clone(), wire::member_digest(record)?);
                }
            }
            Event::Batch(protocol::Batch::Tombstones { records }) => {
                for record in records {
                    members.insert(
                        record.conversation_id.clone(),
                        wire::tombstone_member_digest(&record.conversation_id),
                    );
                }
            }
            Event::Batch(protocol::Batch::Messages { .. })
            | Event::Remove { .. }
            | Event::Start(_)
            | Event::Finish => {}
        }
    }
    Ok(members
        .into_iter()
        .map(|(conversation_id, digest)| wire::BackfillMember {
            conversation_id,
            digest,
        })
        .collect())
}

/// Encode, compress and digest a baseline manifest as one transfer piece.
fn encode_backfill_manifest(
    members: &[wire::BackfillMember],
) -> Result<(wire::Piece, String, u64), AppError> {
    use prost::Message as _;
    let mut bytes = Vec::new();
    for member in members {
        member
            .encode_length_delimited(&mut bytes)
            .map_err(|_| AppError::internal("encode backfill manifest"))?;
    }
    if bytes.len() > wire::DEFAULT_MAX_DECODED_BYTES {
        return Err(AppError::usage("backfill manifest exceeds its size limit"));
    }
    let encoded = wire::compress(&bytes)?;
    let piece = wire::Piece {
        sequence: 0,
        decoded_offset: 0,
        decoded_length: bytes.len() as u64,
        digest: blake3::hash(&bytes).to_hex().to_string(),
        encoded,
        compression: wire::Compression::Zstd as i32,
    };
    let records = u32::try_from(members.len())
        .map_err(|_| AppError::usage("backfill manifest has too many members"))?;
    let entry = wire::ManifestPiece {
        sequence: 0,
        decoded_offset: 0,
        decoded_length: piece.decoded_length,
        digest: &piece.digest,
    };
    let digest = wire::manifest_digest(piece.decoded_length, records, &[entry]);
    Ok((piece, digest, bytes.len() as u64))
}

/// Send one baseline with its manifest, batch and completion over `/v1/sync`.
///
/// The sealed spool must describe a non-snapshot batch whose `revision` is the
/// frozen baseline. The server installs the manifest, applies the batch with
/// per-conversation ordering, records coverage, and completes the baseline.
pub(crate) async fn upload_backfill(
    config: &SharedConfig,
    file: &tempfile::NamedTempFile,
    backfill_id: &str,
) -> Result<protocol::Indexed, AppError> {
    upload_backfill_with_token(config, token(config)?, file, backfill_id).await
}

/// Same sender with an explicit credential, so tests do not mutate process env.
#[allow(
    clippy::too_many_lines,
    reason = "one baseline session: handshake, manifest, batch, completion"
)]
pub(crate) async fn upload_backfill_with_token(
    config: &SharedConfig,
    token: String,
    file: &tempfile::NamedTempFile,
    backfill_id: &str,
) -> Result<protocol::Indexed, AppError> {
    let (start, events) = read_events(file)?;
    let baseline_revision = start
        .revision
        .ok_or_else(|| AppError::usage("a backfill batch requires a frozen revision"))?;
    if start.snapshot {
        return Err(AppError::usage(
            "a backfill batch is not a snapshot; send deltas from the frozen baseline",
        ));
    }
    let members = backfill_members(&events)?;
    if members.is_empty() {
        return Err(AppError::usage(
            "a backfill batch must carry at least one conversation",
        ));
    }
    let (manifest_piece, manifest_digest, manifest_bytes) = encode_backfill_manifest(&members)?;
    let encoded = wire::encode_publication(&events, wire::DEFAULT_MAX_DECODED_BYTES)?;
    let views_fingerprint = start.views_build.as_ref().map_or_else(
        || "unspecified".to_owned(),
        |build| serde_json::to_string(build).unwrap_or_default(),
    );

    let request = request(config, &token)?;
    let (socket, _) =
        tokio::time::timeout(HANDSHAKE_TIMEOUT, tokio_tungstenite::connect_async(request))
            .await
            .map_err(|_| AppError::shared("shared synchronization handshake timed out"))?
            .map_err(|_| {
                AppError::shared("could not connect to the shared synchronization endpoint")
            })?;
    let (mut sink, mut stream) = socket.split();
    send(
        &mut sink,
        &ClientFrame {
            operation_id: String::new(),
            body: Some(client_frame::Body::Hello(Hello {
                protocol_version: wire::PROTOCOL_VERSION,
                database_instance: start.instance.clone(),
                zstd_publication_data: true,
                max_decoded_bytes: wire::DEFAULT_MAX_DECODED_BYTES as u64,
            })),
        },
    )
    .await?;
    let welcome = match receive(&mut stream).await?.body {
        Some(server_frame::Body::Welcome(welcome))
            if welcome.protocol_version == wire::PROTOCOL_VERSION =>
        {
            welcome
        }
        Some(server_frame::Body::Rejected(failed) | server_frame::Body::Failed(failed)) => {
            return Err(outcome_error(&failed));
        }
        _ => {
            return Err(AppError::shared(
                "shared synchronization did not accept this protocol version",
            ));
        }
    };
    let mut credit = usize::try_from(welcome.credit_bytes).unwrap_or(0);

    // 1. Install the baseline manifest.
    let manifest_operation = format!("backfill-manifest:{manifest_digest}");
    send(
        &mut sink,
        &ClientFrame {
            operation_id: manifest_operation.clone(),
            body: Some(client_frame::Body::BeginBackfill(BeginBackfill {
                backfill_id: backfill_id.to_owned(),
                database_instance: start.instance.clone(),
                baseline_revision,
                views_fingerprint,
                manifest_digest: manifest_digest.clone(),
                total_conversations: members.len() as u64,
                decoded_bytes: manifest_bytes,
            })),
        },
    )
    .await?;
    expect_accepted(&mut stream, &mut credit).await?;
    send(
        &mut sink,
        &ClientFrame {
            operation_id: manifest_operation.clone(),
            body: Some(client_frame::Body::Chunk(wire::EncodedBatch::chunk(
                &manifest_piece,
            ))),
        },
    )
    .await?;
    let mut manifest_finished = false;
    loop {
        match receive(&mut stream).await?.body {
            Some(server_frame::Body::Accepted(_)) => break,
            Some(server_frame::Body::Credit(grant)) => {
                credit = credit.saturating_add(grant.additional_bytes as usize);
            }
            Some(server_frame::Body::Checkpoint(_)) => {
                if !manifest_finished {
                    send(
                        &mut sink,
                        &ClientFrame {
                            operation_id: manifest_operation.clone(),
                            body: Some(client_frame::Body::FinishUpload(FinishUpload {
                                manifest_digest: manifest_digest.clone(),
                            })),
                        },
                    )
                    .await?;
                    manifest_finished = true;
                }
            }
            Some(server_frame::Body::Rejected(failed) | server_frame::Body::Failed(failed)) => {
                return Err(outcome_error(&failed));
            }
            Some(_) => {}
            None => return Err(AppError::shared("empty synchronization response")),
        }
    }

    // 2. Send the frozen batch as a backfill publication.
    let operation = format!("publication:{}", encoded.manifest_digest);
    send(
        &mut sink,
        &ClientFrame {
            operation_id: operation.clone(),
            body: Some(client_frame::Body::BeginPublication(BeginPublication {
                start: Some(PublicationStart::from(&start)),
                mode: PublicationMode::BackfillBatch as i32,
                backfill_id: backfill_id.to_owned(),
                source_revision: Some(baseline_revision),
                journal_from_exclusive: None,
                journal_through_inclusive: None,
                manifest_digest: encoded.manifest_digest.clone(),
                decoded_bytes: encoded.decoded_bytes,
                record_count: encoded.record_count,
            })),
        },
    )
    .await?;
    expect_accepted(&mut stream, &mut credit).await?;
    for piece in &encoded.pieces {
        send(
            &mut sink,
            &ClientFrame {
                operation_id: operation.clone(),
                body: Some(client_frame::Body::Chunk(wire::EncodedBatch::chunk(piece))),
            },
        )
        .await?;
    }
    send(
        &mut sink,
        &ClientFrame {
            operation_id: operation.clone(),
            body: Some(client_frame::Body::FinishUpload(FinishUpload {
                manifest_digest: encoded.manifest_digest.clone(),
            })),
        },
    )
    .await?;
    let indexed = loop {
        match receive(&mut stream).await?.body {
            Some(server_frame::Body::Published(published)) => break indexed(published.revision),
            Some(server_frame::Body::Rejected(failed) | server_frame::Body::Failed(failed)) => {
                return Err(outcome_error(&failed));
            }
            Some(_) => {}
            None => return Err(AppError::shared("empty synchronization response")),
        }
    };

    // 3. Verify complete baseline coverage.
    send(
        &mut sink,
        &ClientFrame {
            operation_id: format!("finish-backfill:{backfill_id}"),
            body: Some(client_frame::Body::FinishBackfill(FinishBackfill {
                backfill_id: backfill_id.to_owned(),
                baseline_revision,
                manifest_digest,
            })),
        },
    )
    .await?;
    loop {
        match receive(&mut stream).await?.body {
            Some(server_frame::Body::BackfillComplete(_)) => break,
            Some(server_frame::Body::Rejected(failed) | server_frame::Body::Failed(failed)) => {
                return Err(outcome_error(&failed));
            }
            Some(_) => {}
            None => return Err(AppError::shared("empty synchronization response")),
        }
    }
    Ok(indexed)
}

/// Wait for the first `Accepted`, honouring intervening credit/checkpoint frames.
async fn expect_accepted(
    stream: &mut futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
    credit: &mut usize,
) -> Result<(), AppError> {
    loop {
        match receive(stream).await?.body {
            Some(server_frame::Body::Accepted(accepted)) => {
                *credit = credit.saturating_add(accepted.credit_bytes as usize);
                return Ok(());
            }
            Some(server_frame::Body::Credit(grant)) => {
                *credit = credit.saturating_add(grant.additional_bytes as usize);
            }
            Some(server_frame::Body::Rejected(failed) | server_frame::Body::Failed(failed)) => {
                return Err(outcome_error(&failed));
            }
            Some(_) => {}
            None => return Err(AppError::shared("empty synchronization response")),
        }
    }
}

fn indexed(revision: i64) -> protocol::Indexed {
    protocol::Indexed {
        counts: crate::storage::Counts::default(),
        revision,
        removed_messages: 0,
        purged_conversations: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One publication spool with a conversation record.
    fn spool(instance: &str) -> tempfile::NamedTempFile {
        use std::io::Write as _;
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut writer = std::io::BufWriter::new(file.as_file());
        let start = crate::shared::protocol::Start {
            views_build: None,
            source_silver: None,
            instance: instance.into(),
            revision: Some(1),
            base_revision: None,
            snapshot: true,
            forgets: Vec::new(),
        };
        let record = crate::shared::protocol::ConversationRecord {
            enrichment: None,
            id: "reuse-chat".into(),
            provider: crate::provider::Provider::Codex,
            title: Some("reuse".into()),
            project: None,
            repository: None,
            created_at: Some(1),
            updated_at: Some(2),
        };
        for event in [
            Event::Start(start),
            Event::Batch(crate::shared::protocol::Batch::Conversations {
                records: vec![record],
            }),
            Event::Finish,
        ] {
            serde_json::to_writer(&mut writer, &event).unwrap();
            writer.write_all(b"\n").unwrap();
        }
        writer.flush().unwrap();
        drop(writer);
        file
    }

    fn server_replies(
        frame: &wire::ClientFrame,
        publications: &std::sync::atomic::AtomicUsize,
    ) -> Vec<wire::server_frame::Body> {
        match &frame.body {
            Some(client_frame::Body::Hello(_)) => {
                vec![server_frame::Body::Welcome(wire::Welcome {
                    protocol_version: wire::PROTOCOL_VERSION,
                    session_id: "session".into(),
                    credit_bytes: wire::DEFAULT_CREDIT_BYTES,
                    max_envelope_bytes: u32::try_from(wire::MAX_ENVELOPE_BYTES).unwrap(),
                    max_decoded_bytes: u64::try_from(wire::DEFAULT_MAX_DECODED_BYTES).unwrap(),
                    heartbeat_ms: 30_000,
                    retained_revision: None,
                    views_fingerprint: "generation".into(),
                    incomplete_retention_ms: 24 * 60 * 60 * 1000,
                })]
            }
            Some(client_frame::Body::Resume(_)) => {
                vec![server_frame::Body::Outcome(wire::Outcome {
                    state: wire::OutcomeState::Unknown as i32,
                    commit_state: wire::CommitState::NotCommitted as i32,
                    checkpoint_sequence: 0,
                    revision: None,
                    receipt_digest: String::new(),
                })]
            }
            Some(client_frame::Body::BeginPublication(_)) => {
                vec![server_frame::Body::Accepted(wire::Accepted {
                    credit_bytes: 0,
                })]
            }
            Some(client_frame::Body::Chunk(chunk)) => vec![
                server_frame::Body::Checkpoint(wire::Checkpoint {
                    sequence: chunk.sequence,
                    decoded_offset: chunk.decoded_offset + chunk.decoded_length,
                }),
                server_frame::Body::Credit(wire::Credit {
                    additional_bytes: u32::try_from(chunk.data.len()).unwrap_or(u32::MAX),
                }),
            ],
            Some(client_frame::Body::FinishUpload(_)) => {
                publications.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                vec![server_frame::Body::Published(wire::Published {
                    revision: 1,
                    views_current: true,
                    coverage_through: None,
                })]
            }
            _ => Vec::new(),
        }
    }

    type Counters = (
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
    );

    /// Fake `/v1/sync` server: counts connections, serves any number of
    /// publications per connection, and optionally closes after the first.
    async fn start_server(
        close_after_first: bool,
    ) -> (String, Counters, tokio::task::JoinHandle<()>) {
        use axum::Router;
        use axum::extract::State;
        use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
        use futures_util::{SinkExt, StreamExt};

        async fn serve(mut socket: WebSocket, counters: Counters, close_after_first: bool) {
            let (connections, publications) = &counters;
            connections.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            while let Some(Ok(message)) = socket.next().await {
                let WsMessage::Binary(bytes) = message else {
                    continue;
                };
                let Ok(frame) = wire::decode_client(&bytes) else {
                    break;
                };
                let finished = matches!(frame.body, Some(client_frame::Body::FinishUpload(_)));
                for body in server_replies(&frame, publications) {
                    let reply = ServerFrame {
                        operation_id: frame.operation_id.clone(),
                        body: Some(body),
                    };
                    if socket
                        .send(WsMessage::Binary(
                            wire::encode_server(&reply).unwrap().into(),
                        ))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                if close_after_first && finished {
                    let _ = socket.close().await;
                    return;
                }
            }
        }

        let counters: Counters = (
            std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let state = counters.clone();
        let task = tokio::spawn(async move {
            let router = Router::new()
                .route(
                    "/v1/sync",
                    axum::routing::get(
                        move |upgrade: WebSocketUpgrade, State(counters): State<Counters>| async move {
                            upgrade.on_upgrade(move |socket| serve(socket, counters, close_after_first))
                        },
                    ),
                )
                .with_state(state);
            axum::serve(listener, router).await.unwrap();
        });
        (format!("http://{address}"), counters, task)
    }

    fn session_config(api_url: String) -> SharedConfig {
        SharedConfig {
            group_id: "group".into(),
            node_id: "node".into(),
            api_url,
            token_env: "unused".into(),
            publication_redaction: crate::redaction::Policy::new(false),
        }
    }

    #[tokio::test]
    async fn publications_reuse_one_publisher_session() {
        let (url, counters, server) = start_server(false).await;
        let config = session_config(url);
        let file = spool("reuse-instance");
        for _ in 0..3 {
            crate::shared::sync::client::upload_with_token(&config, &file, "test-token".to_owned())
                .await
                .expect("publication");
        }
        assert_eq!(
            counters.0.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "three publications share one handshake"
        );
        assert_eq!(
            counters.1.load(std::sync::atomic::Ordering::SeqCst),
            3,
            "each publication reached the server"
        );
        server.abort();
    }

    #[tokio::test]
    async fn a_closed_session_is_replaced_on_the_next_publication() {
        let (url, counters, server) = start_server(true).await;
        let config = session_config(url);
        let file = spool("reuse-instance");
        crate::shared::sync::client::upload_with_token(&config, &file, "test-token".to_owned())
            .await
            .expect("first publication");
        crate::shared::sync::client::upload_with_token(&config, &file, "test-token".to_owned())
            .await
            .expect("second publication");
        assert_eq!(
            counters.0.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "the server closed the first session, so the second reconnects"
        );
        assert_eq!(counters.1.load(std::sync::atomic::Ordering::SeqCst), 2);
        server.abort();
    }

    #[test]
    fn websocket_endpoint_is_derived_without_credentials() {
        assert_eq!(
            endpoint("https://example.test/mash/").unwrap(),
            "wss://example.test/mash/v1/sync"
        );
        assert_eq!(
            endpoint("http://127.0.0.1:8787").unwrap(),
            "ws://127.0.0.1:8787/v1/sync"
        );
        assert!(endpoint("ftp://example.test").is_err());
    }
}
