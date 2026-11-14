//! Typed Protobuf wire contracts and bounded zstd publication batches.
//!
//! Transfer digests authenticate exact encoded bytes. Canonical history
//! fingerprints remain in silver; Protobuf serialization is never used as a
//! canonical history identity.
//!
//! Publication, attachment and progressive-backfill transfers share the
//! negotiated contract.
#![allow(dead_code)]
use crate::AppError;
use crate::shared::protocol::{self, Event};
use prost::Message;
use std::io::Read;

// Version 4 requires native record batches, including the final record phase
// of progressive backfill. Older peers must reject before accepting a transfer.
pub const PROTOCOL_VERSION: u32 = 4;
pub(crate) const MAX_CONTROL_BYTES: usize = 64 * 1024;
pub const MAX_ENVELOPE_BYTES: usize = 4 * 1024 * 1024;
pub const DEFAULT_CREDIT_BYTES: u32 = 4 * 1024 * 1024;
pub(crate) const NORMAL_BATCH_TARGET: usize = 1024 * 1024;
pub(crate) const MAX_BATCH_CHATS: usize = 128;
pub(crate) const OVERSIZED_PIECE_TARGET: usize = 256 * 1024;
/// The largest decoded publication batch a client builds and asks a server to
/// accept; servers cap it at their own `max_upload_mib`. Batches group many
/// chats up to the normal target, so only one very long chat can exceed it:
/// real agent sessions reach hundreds of megabytes of text, and a chat that
/// cannot fit would keep its node's shared publication stale forever.
pub const DEFAULT_MAX_DECODED_BYTES: usize = 1024 * 1024 * 1024;
/// Why one atomic publication cannot be sent; publishers then use a backfill.
pub(crate) const BATCH_TOO_LARGE: &str = "publication batch exceeds decoded-size limit";

#[derive(Clone, PartialEq, Message)]
pub struct ClientFrame {
    #[prost(string, tag = "1")]
    pub operation_id: String,
    #[prost(oneof = "client_frame::Body", tags = "2, 3, 4, 6, 7, 8, 9, 10, 11")]
    pub body: Option<client_frame::Body>,
}

pub mod client_frame {
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum Body {
        #[prost(message, tag = "2")]
        Hello(super::Hello),
        #[prost(message, tag = "3")]
        BeginBackfill(super::BeginBackfill),
        #[prost(message, tag = "4")]
        BeginPublication(super::BeginPublication),
        #[prost(message, tag = "6")]
        BeginAttachment(super::BeginAttachment),
        #[prost(message, tag = "7")]
        Chunk(super::Chunk),
        #[prost(message, tag = "8")]
        FinishUpload(super::FinishUpload),
        #[prost(message, tag = "9")]
        FinishBackfill(super::FinishBackfill),
        #[prost(message, tag = "10")]
        Resume(super::Resume),
        #[prost(message, tag = "11")]
        Cancel(super::Cancel),
    }
}

#[derive(Clone, PartialEq, Message)]
pub struct ServerFrame {
    #[prost(string, tag = "1")]
    pub operation_id: String,
    #[prost(
        oneof = "server_frame::Body",
        tags = "2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17"
    )]
    pub body: Option<server_frame::Body>,
}

pub mod server_frame {
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum Body {
        #[prost(message, tag = "2")]
        Welcome(super::Welcome),
        #[prost(message, tag = "3")]
        Queued(super::Queued),
        #[prost(message, tag = "4")]
        Accepted(super::Accepted),
        #[prost(message, tag = "5")]
        Credit(super::Credit),
        #[prost(message, tag = "6")]
        Checkpoint(super::Checkpoint),
        #[prost(message, tag = "7")]
        Progress(super::Progress),
        #[prost(message, tag = "8")]
        SilverCommitted(super::SilverCommitted),
        #[prost(message, tag = "9")]
        Published(super::Published),
        #[prost(message, tag = "10")]
        AttachmentStored(super::AttachmentStored),
        #[prost(message, tag = "11")]
        BackfillComplete(super::BackfillComplete),
        #[prost(message, tag = "12")]
        Outcome(super::Outcome),
        #[prost(message, tag = "13")]
        Rejected(super::Failed),
        #[prost(message, tag = "14")]
        Failed(super::Failed),
        #[prost(message, tag = "15")]
        Cancelled(super::Cancelled),
        #[prost(message, tag = "16")]
        TooLateToCancel(super::TooLateToCancel),
        #[prost(message, tag = "17")]
        Draining(super::Draining),
    }
}

#[derive(Clone, PartialEq, Message)]
pub struct Hello {
    #[prost(uint32, tag = "1")]
    pub protocol_version: u32,
    #[prost(string, tag = "2")]
    pub database_instance: String,
    #[prost(bool, tag = "3")]
    pub zstd_publication_data: bool,
    #[prost(uint64, tag = "4")]
    pub max_decoded_bytes: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct Welcome {
    #[prost(uint32, tag = "1")]
    pub protocol_version: u32,
    #[prost(string, tag = "2")]
    pub session_id: String,
    #[prost(uint32, tag = "3")]
    pub credit_bytes: u32,
    #[prost(uint32, tag = "4")]
    pub max_envelope_bytes: u32,
    #[prost(uint64, tag = "5")]
    pub max_decoded_bytes: u64,
    #[prost(uint32, tag = "6")]
    pub heartbeat_ms: u32,
    #[prost(int64, optional, tag = "7")]
    pub retained_revision: Option<i64>,
    #[prost(string, tag = "8")]
    pub views_fingerprint: String,
    /// Inactivity budget after which an abandoned receiving transfer is
    /// expired and its spool released.
    #[prost(uint64, tag = "9")]
    pub incomplete_retention_ms: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct BeginBackfill {
    #[prost(string, tag = "1")]
    pub backfill_id: String,
    #[prost(string, tag = "2")]
    pub database_instance: String,
    #[prost(int64, tag = "3")]
    pub baseline_revision: i64,
    #[prost(string, tag = "4")]
    pub views_fingerprint: String,
    #[prost(string, tag = "5")]
    pub manifest_digest: String,
    #[prost(uint64, tag = "6")]
    pub total_chats: u64,
    #[prost(uint64, tag = "7")]
    pub decoded_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub enum PublicationMode {
    Unspecified = 0,
    BackfillBatch = 1,
    Delta = 2,
    ReplaceSnapshot = 3,
}

impl PublicationMode {
    pub const fn parse(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Unspecified),
            1 => Some(Self::BackfillBatch),
            2 => Some(Self::Delta),
            3 => Some(Self::ReplaceSnapshot),
            _ => None,
        }
    }
}

#[derive(Clone, PartialEq, Message)]
pub struct BeginPublication {
    #[prost(message, optional, tag = "1")]
    pub start: Option<PublicationStart>,
    #[prost(enumeration = "PublicationMode", tag = "2")]
    pub mode: i32,
    #[prost(string, tag = "3")]
    pub backfill_id: String,
    #[prost(int64, optional, tag = "4")]
    pub source_revision: Option<i64>,
    #[prost(int64, optional, tag = "5")]
    pub journal_from_exclusive: Option<i64>,
    #[prost(int64, optional, tag = "6")]
    pub journal_through_inclusive: Option<i64>,
    #[prost(string, tag = "7")]
    pub manifest_digest: String,
    #[prost(uint64, tag = "8")]
    pub decoded_bytes: u64,
    #[prost(uint32, tag = "9")]
    pub record_count: u32,
}

#[derive(Clone, PartialEq, Message)]
pub struct PublicationStart {
    #[prost(message, optional, tag = "1")]
    pub views_build: Option<ViewsBuild>,
    #[prost(message, optional, tag = "2")]
    pub source_silver: Option<SourceCertificate>,
    #[prost(string, tag = "3")]
    pub instance: String,
    #[prost(int64, optional, tag = "4")]
    pub revision: Option<i64>,
    #[prost(int64, optional, tag = "5")]
    pub base_revision: Option<i64>,
    #[prost(bool, tag = "6")]
    pub snapshot: bool,
    #[prost(string, repeated, tag = "7")]
    pub forgets: Vec<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct BeginAttachment {
    #[prost(string, tag = "1")]
    pub message_id: String,
    #[prost(message, optional, tag = "2")]
    pub attachment: Option<Attachment>,
    #[prost(string, tag = "3")]
    pub manifest_digest: String,
    #[prost(uint64, tag = "4")]
    pub decoded_bytes: u64,
}

/// One baseline member in a backfill manifest. `digest` identifies the
/// chat content the client froze; a tombstone digest is produced by
/// [`tombstone_member_digest`].
#[derive(Clone, PartialEq, Message)]
pub struct BackfillMember {
    #[prost(string, tag = "1")]
    pub chat_id: String,
    #[prost(string, tag = "2")]
    pub digest: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub enum Compression {
    None = 0,
    Zstd = 1,
}

impl Compression {
    pub(crate) const fn parse(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::Zstd),
            _ => None,
        }
    }
}

#[derive(Clone, PartialEq, Message)]
pub struct Chunk {
    #[prost(uint64, tag = "1")]
    pub sequence: u64,
    #[prost(uint64, tag = "2")]
    pub decoded_offset: u64,
    #[prost(enumeration = "Compression", tag = "3")]
    pub compression: i32,
    #[prost(uint64, tag = "4")]
    pub decoded_length: u64,
    #[prost(string, tag = "5")]
    pub digest: String,
    #[prost(bytes = "vec", tag = "6")]
    pub data: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
pub struct FinishUpload {
    #[prost(string, tag = "1")]
    pub manifest_digest: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct FinishBackfill {
    #[prost(string, tag = "1")]
    pub backfill_id: String,
    #[prost(int64, tag = "2")]
    pub baseline_revision: i64,
    #[prost(string, tag = "3")]
    pub manifest_digest: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct Resume {
    #[prost(string, tag = "1")]
    pub manifest_digest: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct Cancel {}

#[derive(Clone, PartialEq, Message)]
pub struct Queued {
    #[prost(uint32, tag = "1")]
    pub position: u32,
}

#[derive(Clone, PartialEq, Message)]
pub struct Accepted {
    #[prost(uint32, tag = "1")]
    pub credit_bytes: u32,
}

#[derive(Clone, PartialEq, Message)]
pub struct Credit {
    #[prost(uint32, tag = "1")]
    pub additional_bytes: u32,
}

#[derive(Clone, PartialEq, Message)]
pub struct Checkpoint {
    #[prost(uint64, tag = "1")]
    pub sequence: u64,
    #[prost(uint64, tag = "2")]
    pub decoded_offset: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct Progress {
    #[prost(string, tag = "1")]
    pub phase: String,
    #[prost(uint64, tag = "2")]
    pub completed: u64,
    #[prost(uint64, tag = "3")]
    pub total: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct SilverCommitted {
    #[prost(int64, tag = "1")]
    pub revision: i64,
    #[prost(string, tag = "2")]
    pub receipt_digest: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct Published {
    #[prost(int64, tag = "1")]
    pub revision: i64,
    #[prost(bool, tag = "2")]
    pub views_current: bool,
    #[prost(int64, optional, tag = "3")]
    pub coverage_through: Option<i64>,
}

#[derive(Clone, PartialEq, Message)]
pub struct AttachmentStored {
    #[prost(string, tag = "1")]
    pub digest: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct BackfillComplete {
    #[prost(string, tag = "1")]
    pub backfill_id: String,
    #[prost(uint64, tag = "2")]
    pub covered_chats: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub enum OutcomeState {
    Unspecified = 0,
    Receiving = 1,
    QueuedForApply = 2,
    Applying = 3,
    ViewsPending = 4,
    Published = 5,
    Cancelled = 6,
    Failed = 7,
    Expired = 8,
    Unknown = 9,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub enum CommitState {
    Unspecified = 0,
    NotCommitted = 1,
    Committed = 2,
    Unknown = 3,
}

#[derive(Clone, PartialEq, Message)]
pub struct Outcome {
    #[prost(enumeration = "OutcomeState", tag = "1")]
    pub state: i32,
    #[prost(enumeration = "CommitState", tag = "2")]
    pub commit_state: i32,
    #[prost(uint64, tag = "3")]
    pub checkpoint_sequence: u64,
    #[prost(int64, optional, tag = "4")]
    pub revision: Option<i64>,
    #[prost(string, tag = "5")]
    pub receipt_digest: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct Failed {
    #[prost(string, tag = "1")]
    pub phase: String,
    #[prost(string, tag = "2")]
    pub code: String,
    #[prost(enumeration = "CommitState", tag = "3")]
    pub commit_state: i32,
    #[prost(bool, tag = "4")]
    pub retryable: bool,
    #[prost(string, tag = "5")]
    pub recovery_action: String,
    #[prost(string, tag = "6")]
    pub message: String,
    /// The server error's kind as named in JSON errors; empty from older
    /// servers, which clients read as a shared-index failure.
    #[prost(string, tag = "7")]
    pub kind: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct Cancelled {}
#[derive(Clone, PartialEq, Message)]
pub struct TooLateToCancel {}
#[derive(Clone, PartialEq, Message)]
pub struct Draining {}

#[derive(Clone, PartialEq, Message)]
pub struct ViewsBuild {
    #[prost(uint32, tag = "1")]
    pub retrieval_unit: u32,
    #[prost(uint32, tag = "2")]
    pub dense_content: u32,
    #[prost(uint32, tag = "3")]
    pub text_preparation: u32,
    #[prost(uint32, tag = "4")]
    pub representations: u32,
    #[prost(uint32, tag = "5")]
    pub embedding_text: u32,
    #[prost(uint32, optional, tag = "6")]
    pub max_embedding_chars: Option<u32>,
}

#[derive(Clone, PartialEq, Message)]
pub struct SourceCertificate {
    #[prost(uint64, tag = "1")]
    pub revision: u64,
    #[prost(uint32, tag = "2")]
    pub interpretation_version: u32,
    #[prost(uint32, tag = "3")]
    pub completeness: u32,
    #[prost(uint64, tag = "4")]
    pub rejected_sources: u64,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct NativeRecord {
    #[prost(oneof = "native_record::Body", tags = "1, 2, 3, 4, 5, 6")]
    pub body: Option<native_record::Body>,
}

pub(crate) mod native_record {
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub(crate) enum Body {
        #[prost(message, tag = "1")]
        Chat(super::Chat),
        #[prost(message, tag = "2")]
        Message(super::NativeMessage),
        #[prost(message, tag = "3")]
        Tombstone(super::Tombstone),
        #[prost(string, tag = "4")]
        Remove(String),
        /// Canonical typed recall row, validated on decoding and application.
        #[prost(bytes, tag = "5")]
        Recall(Vec<u8>),
        #[prost(message, tag = "6")]
        Amend(super::Amend),
    }
}

/// One chat a delta amends; see `protocol::Amendment`.
#[derive(Clone, PartialEq, Message)]
pub(crate) struct Amend {
    #[prost(message, optional, tag = "1")]
    pub chat: Option<Chat>,
    #[prost(string, repeated, tag = "2")]
    pub replaced: Vec<String>,
    #[prost(uint64, tag = "3")]
    pub messages: u64,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct Chat {
    #[prost(message, optional, tag = "1")]
    pub enrichment: Option<Enrichment>,
    #[prost(string, tag = "2")]
    pub id: String,
    #[prost(string, tag = "3")]
    pub provider: String,
    #[prost(string, optional, tag = "4")]
    pub title: Option<String>,
    #[prost(string, optional, tag = "5")]
    pub project: Option<String>,
    #[prost(string, optional, tag = "6")]
    pub repository: Option<String>,
    #[prost(int64, optional, tag = "7")]
    pub created_at: Option<i64>,
    #[prost(int64, optional, tag = "8")]
    pub updated_at: Option<i64>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct NativeMessage {
    #[prost(message, repeated, tag = "1")]
    pub attachments: Vec<Attachment>,
    #[prost(string, tag = "2")]
    pub id: String,
    #[prost(string, tag = "3")]
    pub chat_id: String,
    #[prost(int64, tag = "4")]
    pub ordinal: i64,
    #[prost(message, optional, tag = "5")]
    pub context: Option<MessageContext>,
    #[prost(string, tag = "6")]
    pub role: String,
    #[prost(string, tag = "7")]
    pub content: String,
    #[prost(string, tag = "8")]
    pub projection: String,
    #[prost(int64, optional, tag = "9")]
    pub created_at: Option<i64>,
    #[prost(string, optional, tag = "10")]
    pub generation: Option<String>,
    #[prost(message, optional, tag = "11")]
    pub vector: Option<Vector>,
    #[prost(message, optional, tag = "12")]
    pub tools: Option<ToolActivity>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct Vector {
    #[prost(double, repeated, tag = "1")]
    pub values: Vec<f64>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct MessageContext {
    #[prost(uint32, tag = "1")]
    pub kind: u32,
    // Tag 2 belonged to ordinal ancestry in protocol 1.
    #[prost(string, optional, tag = "3")]
    pub parent_id: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct Attachment {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, tag = "2")]
    pub name: String,
    #[prost(string, tag = "3")]
    pub media_type: String,
    #[prost(uint32, tag = "4")]
    pub availability: u32,
    #[prost(uint64, optional, tag = "5")]
    pub bytes: Option<u64>,
    #[prost(string, optional, tag = "6")]
    pub digest: Option<String>,
    #[prost(bool, tag = "7")]
    pub redacted: bool,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct ToolActivity {
    #[prost(message, repeated, tag = "1")]
    pub calls: Vec<ToolCall>,
    #[prost(message, repeated, tag = "2")]
    pub results: Vec<ToolResult>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct ToolCall {
    #[prost(string, optional, tag = "1")]
    pub name: Option<String>,
    #[prost(string, optional, tag = "2")]
    pub call_id: Option<String>,
    /// Opaque provider JSON, not a generic model for known protocol fields.
    #[prost(string, optional, tag = "3")]
    pub input_json: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct ToolResult {
    #[prost(string, optional, tag = "1")]
    pub call_id: Option<String>,
    #[prost(bool, optional, tag = "2")]
    pub is_error: Option<bool>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct Tombstone {
    #[prost(string, tag = "1")]
    pub provider: String,
    #[prost(string, tag = "2")]
    pub chat_id: String,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct Enrichment {
    #[prost(uint32, tag = "1")]
    pub version: u32,
    #[prost(message, optional, tag = "2")]
    pub title: Option<EnrichmentOrigin>,
    #[prost(message, optional, tag = "3")]
    pub project: Option<EnrichmentOrigin>,
    #[prost(message, optional, tag = "4")]
    pub repository: Option<EnrichmentOrigin>,
    #[prost(message, optional, tag = "5")]
    pub timestamps: Option<EnrichmentTimestamps>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct EnrichmentOrigin {
    #[prost(oneof = "enrichment_origin::Kind", tags = "1, 2, 3")]
    pub kind: Option<enrichment_origin::Kind>,
}

pub(crate) mod enrichment_origin {
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub(crate) enum Kind {
        #[prost(bool, tag = "1")]
        Unknown(bool),
        #[prost(string, tag = "2")]
        SourceField(String),
        #[prost(message, tag = "3")]
        FirstVisible(super::FirstVisibleMessage),
    }
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct FirstVisibleMessage {
    #[prost(string, tag = "1")]
    pub message_id: String,
    #[prost(uint32, tag = "2")]
    pub rule_version: u32,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct EnrichmentTimestamps {
    #[prost(message, optional, tag = "1")]
    pub created_at: Option<TimestampOrigin>,
    #[prost(message, optional, tag = "2")]
    pub updated_at: Option<TimestampOrigin>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct TimestampOrigin {
    #[prost(oneof = "timestamp_origin::Kind", tags = "1, 2, 3")]
    pub kind: Option<timestamp_origin::Kind>,
}

pub(crate) mod timestamp_origin {
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub(crate) enum Kind {
        #[prost(bool, tag = "1")]
        Unknown(bool),
        #[prost(message, tag = "2")]
        Source(super::SourceTimestamp),
        #[prost(message, tag = "3")]
        Boundary(super::MessageBoundary),
    }
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct SourceTimestamp {
    #[prost(string, tag = "1")]
    pub field: String,
    #[prost(uint32, tag = "2")]
    pub rule_version: u32,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct MessageBoundary {
    #[prost(string, tag = "1")]
    pub message_id: String,
    #[prost(bool, tag = "2")]
    pub maximum: bool,
    #[prost(uint32, tag = "3")]
    pub rule_version: u32,
}

fn invalid(message: &'static str) -> AppError {
    AppError::usage(message)
}

pub fn encode_client(frame: &ClientFrame) -> Result<Vec<u8>, AppError> {
    encode_frame(
        frame,
        matches!(frame.body, Some(client_frame::Body::Chunk(_))),
    )
}

pub fn encode_server(frame: &ServerFrame) -> Result<Vec<u8>, AppError> {
    encode_frame(frame, false)
}

fn encode_frame(frame: &impl Message, data: bool) -> Result<Vec<u8>, AppError> {
    let limit = if data {
        MAX_ENVELOPE_BYTES
    } else {
        MAX_CONTROL_BYTES
    };
    let size = frame.encoded_len();
    if size > limit {
        return Err(invalid("sync frame exceeds negotiated limits"));
    }
    let mut bytes = Vec::with_capacity(size);
    frame
        .encode(&mut bytes)
        .map_err(|_| AppError::internal("encode sync frame"))?;
    Ok(bytes)
}

pub fn decode_client(bytes: &[u8]) -> Result<ClientFrame, AppError> {
    if bytes.len() > MAX_ENVELOPE_BYTES {
        return Err(invalid("sync frame exceeds limits"));
    }
    let frame = ClientFrame::decode(bytes).map_err(|_| invalid("invalid sync Protobuf frame"))?;
    if frame.body.is_none() {
        return Err(invalid("empty sync frame"));
    }
    if !matches!(frame.body, Some(client_frame::Body::Chunk(_))) && bytes.len() > MAX_CONTROL_BYTES
    {
        return Err(invalid("sync control frame exceeds limits"));
    }
    Ok(frame)
}

pub fn decode_server(bytes: &[u8]) -> Result<ServerFrame, AppError> {
    if bytes.len() > MAX_CONTROL_BYTES {
        return Err(invalid("sync control frame exceeds limits"));
    }
    let frame = ServerFrame::decode(bytes).map_err(|_| invalid("invalid sync Protobuf frame"))?;
    if frame.body.is_none() {
        return Err(invalid("empty sync frame"));
    }
    Ok(frame)
}

pub fn compress(bytes: &[u8]) -> Result<Vec<u8>, AppError> {
    zstd::stream::encode_all(bytes, 1).map_err(|_| AppError::internal("compress publication batch"))
}

#[allow(
    clippy::similar_names,
    reason = "decoded length and decoded bytes are distinct concepts in the wire contract"
)]
pub fn decompress(bytes: &[u8], decoded_length: usize, limit: usize) -> Result<Vec<u8>, AppError> {
    if decoded_length > limit {
        return Err(invalid("publication piece exceeds decoded-size limit"));
    }
    let decoder = zstd::stream::read::Decoder::new(bytes)
        .map_err(|_| invalid("invalid zstd publication piece"))?;
    let mut decoded = Vec::with_capacity(decoded_length.min(1024 * 1024));
    decoder
        .take((limit as u64).saturating_add(1))
        .read_to_end(&mut decoded)
        .map_err(|_| invalid("invalid zstd publication piece"))?;
    if decoded.len() != decoded_length || decoded.len() > limit {
        return Err(invalid("publication piece decoded length mismatch"));
    }
    Ok(decoded)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    pub sequence: u64,
    pub decoded_offset: u64,
    pub decoded_length: u64,
    pub digest: String,
    pub encoded: Vec<u8>,
    /// [`Compression`] applied to `encoded`; set per operation kind.
    pub compression: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedBatch {
    pub decoded_bytes: u64,
    pub record_count: u32,
    pub chat_count: u32,
    pub manifest_digest: String,
    pub pieces: Vec<Piece>,
}

impl EncodedBatch {
    pub fn chunk(piece: &Piece) -> Chunk {
        Chunk {
            sequence: piece.sequence,
            decoded_offset: piece.decoded_offset,
            compression: piece.compression,
            decoded_length: piece.decoded_length,
            digest: piece.digest.clone(),
            data: piece.encoded.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ManifestPiece<'a> {
    pub sequence: u64,
    pub decoded_offset: u64,
    pub decoded_length: u64,
    pub digest: &'a str,
}

/// Digest over ordered pieces, decoded size and record count.
///
/// Both preparation and receipt verification use this function so a retry
/// cannot change ordering, offsets or payload bytes under the same manifest.
pub fn manifest_digest(
    decoded_bytes: u64,
    record_count: u32,
    pieces: &[ManifestPiece<'_>],
) -> String {
    let mut manifest = blake3::Hasher::new();
    manifest.update(b"mash-sync-manifest-v1\0");
    manifest.update(&decoded_bytes.to_le_bytes());
    manifest.update(&record_count.to_le_bytes());
    for piece in pieces {
        manifest.update(&piece.sequence.to_le_bytes());
        manifest.update(&piece.decoded_offset.to_le_bytes());
        manifest.update(&piece.decoded_length.to_le_bytes());
        manifest.update(piece.digest.as_bytes());
    }
    manifest.finalize().to_hex().to_string()
}

#[allow(
    clippy::too_many_lines,
    reason = "one bounded encoder that counts every publication event kind the same way"
)]
pub fn encode_publication(events: &[Event], max_decoded: usize) -> Result<EncodedBatch, AppError> {
    let mut decoded = Vec::new();
    let mut records = 0u32;
    let mut chats = 0u32;
    for event in events {
        match event {
            Event::Batch(protocol::Batch::Records { records }) => {
                for record in records {
                    NativeRecord {
                        body: Some(native_record::Body::Recall(
                            serde_json::to_vec(record)
                                .map_err(|_| AppError::internal("encode recall record"))?,
                        )),
                    }
                    .encode_length_delimited(&mut decoded)
                    .map_err(|_| AppError::internal("encode recall envelope"))?;
                }
            }
            Event::Batch(protocol::Batch::Chats { records }) => {
                for record in records {
                    NativeRecord::from(record)
                        .encode_length_delimited(&mut decoded)
                        .map_err(|_| AppError::internal("encode chat record"))?;
                    chats = chats
                        .checked_add(1)
                        .ok_or_else(|| invalid("too many publication records"))?;
                }
            }
            Event::Batch(protocol::Batch::Messages { records }) => {
                for record in records {
                    NativeRecord::try_from(record)?
                        .encode_length_delimited(&mut decoded)
                        .map_err(|_| AppError::internal("encode message record"))?;
                }
            }
            Event::Batch(protocol::Batch::Tombstones { records }) => {
                for record in records {
                    NativeRecord::from(record)
                        .encode_length_delimited(&mut decoded)
                        .map_err(|_| AppError::internal("encode tombstone record"))?;
                }
            }
            Event::Remove { id } => NativeRecord {
                body: Some(native_record::Body::Remove(id.clone())),
            }
            .encode_length_delimited(&mut decoded)
            .map_err(|_| AppError::internal("encode removal record"))?,
            Event::Amend(amendment) => {
                NativeRecord::from(amendment.as_ref())
                    .encode_length_delimited(&mut decoded)
                    .map_err(|_| AppError::internal("encode amendment record"))?;
                chats = chats
                    .checked_add(1)
                    .ok_or_else(|| invalid("too many publication records"))?;
            }
            Event::Start(_) | Event::Finish => {
                return Err(invalid("publication data cannot contain stream metadata"));
            }
        }
        records = records
            .checked_add(event_len(event))
            .ok_or_else(|| invalid("too many publication records"))?;
        if decoded.len() > max_decoded {
            return Err(invalid(BATCH_TOO_LARGE));
        }
    }
    let piece_target = if decoded.len() > NORMAL_BATCH_TARGET {
        OVERSIZED_PIECE_TARGET
    } else {
        decoded.len().max(1)
    };
    let mut pieces = Vec::new();
    for (index, bytes) in decoded.chunks(piece_target).enumerate() {
        let decoded_offset = pieces
            .iter()
            .map(|piece: &Piece| piece.decoded_length)
            .sum();
        let encoded = compress(bytes)?;
        if encoded.len() > MAX_ENVELOPE_BYTES {
            return Err(invalid(
                "compressed publication piece exceeds envelope limit",
            ));
        }
        pieces.push(Piece {
            sequence: index as u64,
            decoded_offset,
            decoded_length: bytes.len() as u64,
            digest: blake3::hash(bytes).to_hex().to_string(),
            encoded,
            compression: Compression::Zstd as i32,
        });
    }
    let entries = pieces
        .iter()
        .map(|piece| ManifestPiece {
            sequence: piece.sequence,
            decoded_offset: piece.decoded_offset,
            decoded_length: piece.decoded_length,
            digest: &piece.digest,
        })
        .collect::<Vec<_>>();
    Ok(EncodedBatch {
        decoded_bytes: decoded.len() as u64,
        record_count: records,
        chat_count: chats,
        manifest_digest: manifest_digest(decoded.len() as u64, records, &entries),
        pieces,
    })
}

fn event_len(event: &Event) -> u32 {
    match event {
        Event::Batch(batch) => u32::try_from(batch.len()).unwrap_or(u32::MAX),
        Event::Remove { .. } | Event::Amend(_) => 1,
        Event::Start(_) | Event::Finish => 0,
    }
}

/// Records in decoded publication events; verification uses the same counting
/// rule as preparation.
pub fn record_count(events: &[Event]) -> u32 {
    events
        .iter()
        .fold(0u32, |total, event| total.saturating_add(event_len(event)))
}

pub fn decode_publication(pieces: &[Piece], max_decoded: usize) -> Result<Vec<Event>, AppError> {
    decode_records(&decode_bytes(pieces, max_decoded)?)
}

/// Assemble independently compressed pieces into one bounded byte buffer.
pub fn decode_bytes(pieces: &[Piece], max_decoded: usize) -> Result<Vec<u8>, AppError> {
    let total = pieces
        .iter()
        .try_fold(0usize, |total, piece| {
            usize::try_from(piece.decoded_length)
                .ok()
                .and_then(|length| total.checked_add(length))
        })
        .ok_or_else(|| invalid("transfer size overflow"))?;
    if total > max_decoded {
        return Err(invalid("transfer exceeds decoded-size limit"));
    }
    let mut decoded = Vec::with_capacity(total);
    for (index, piece) in pieces.iter().enumerate() {
        if piece.sequence != index as u64 || piece.decoded_offset != decoded.len() as u64 {
            return Err(invalid("non-contiguous transfer pieces"));
        }
        let decoded_length = usize::try_from(piece.decoded_length)
            .map_err(|_| invalid("transfer piece is too large"))?;
        let bytes = match Compression::parse(piece.compression) {
            Some(Compression::Zstd) => decompress(&piece.encoded, decoded_length, max_decoded)?,
            Some(Compression::None) if piece.encoded.len() == decoded_length => {
                piece.encoded.clone()
            }
            _ => return Err(invalid("invalid transfer piece compression")),
        };
        if blake3::hash(&bytes).to_hex().as_str() != piece.digest {
            return Err(invalid("transfer piece integrity mismatch"));
        }
        decoded.extend(bytes);
    }
    Ok(decoded)
}

/// Baseline membership digest for a chat record.
pub fn member_digest(record: &protocol::ChatRecord) -> Result<String, AppError> {
    let mut bytes = Vec::new();
    NativeRecord::from(record)
        .encode_length_delimited(&mut bytes)
        .map_err(|_| AppError::internal("encode backfill member"))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

/// Baseline membership digest for a tombstone, independent of the changed
/// chat record that may arrive later.
pub fn tombstone_member_digest(chat_id: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"mash-backfill-tombstone-v1\0");
    hasher.update(chat_id.as_bytes());
    hasher.finalize().to_hex().to_string()
}

pub fn decode_backfill_manifest(
    bytes: &[u8],
    max_members: usize,
) -> Result<Vec<BackfillMember>, AppError> {
    let mut input = bytes;
    let mut members = Vec::new();
    while !input.is_empty() {
        if members.len() == max_members {
            return Err(invalid("backfill manifest exceeds its member limit"));
        }
        let member = BackfillMember::decode_length_delimited(&mut input)
            .map_err(|_| invalid("invalid backfill manifest member"))?;
        if member.chat_id.is_empty()
            || member.digest.len() != 64
            || !member.digest.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(invalid("invalid backfill manifest member"));
        }
        members.push(member);
    }
    Ok(members)
}

fn decode_records(mut input: &[u8]) -> Result<Vec<Event>, AppError> {
    let mut events = Vec::new();
    let mut pending: Option<Event> = None;
    while !input.is_empty() {
        let record = NativeRecord::decode_length_delimited(&mut input)
            .map_err(|_| invalid("invalid native publication record"))?;
        let event = match record
            .body
            .ok_or_else(|| invalid("empty native publication record"))?
        {
            native_record::Body::Chat(record) => Event::Batch(protocol::Batch::Chats {
                records: vec![record.try_into()?],
            }),
            native_record::Body::Message(record) => Event::Batch(protocol::Batch::Messages {
                records: vec![record.try_into()?],
            }),
            native_record::Body::Tombstone(record) => Event::Batch(protocol::Batch::Tombstones {
                records: vec![record.try_into()?],
            }),
            native_record::Body::Remove(id) => Event::Remove { id },
            native_record::Body::Recall(row) => Event::Batch(protocol::Batch::Records {
                records: vec![
                    serde_json::from_slice(&row).map_err(|_| invalid("invalid recall record"))?,
                ],
            }),
            native_record::Body::Amend(amend) => Event::Amend(Box::new(protocol::Amendment {
                chat: amend
                    .chat
                    .ok_or_else(|| invalid("amendment without a chat record"))?
                    .try_into()?,
                replaced: amend.replaced,
                messages: amend.messages,
            })),
        };
        merge_event(&mut pending, event, &mut events);
    }
    if let Some(event) = pending {
        events.push(event);
    }
    Ok(events)
}

fn merge_event(pending: &mut Option<Event>, next: Event, output: &mut Vec<Event>) {
    let merged = match (pending.as_mut(), &next) {
        (
            Some(Event::Batch(protocol::Batch::Records { records: left })),
            Event::Batch(protocol::Batch::Records { records: right }),
        ) if left.len() < 128 => {
            left.extend(right.iter().cloned());
            true
        }
        (
            Some(Event::Batch(protocol::Batch::Chats { records: left })),
            Event::Batch(protocol::Batch::Chats { records: right }),
        ) if left.len() < 128 => {
            left.extend(right.iter().cloned());
            true
        }
        (
            Some(Event::Batch(protocol::Batch::Messages { records: left })),
            Event::Batch(protocol::Batch::Messages { records: right }),
        ) if left.len() < 128 => {
            left.extend(right.iter().cloned());
            true
        }
        (
            Some(Event::Batch(protocol::Batch::Tombstones { records: left })),
            Event::Batch(protocol::Batch::Tombstones { records: right }),
        ) if left.len() < 128 => {
            left.extend(right.iter().cloned());
            true
        }
        _ => false,
    };
    if merged {
        return;
    }
    if let Some(previous) = pending.replace(next) {
        output.push(previous);
    }
}

impl From<&protocol::ChatRecord> for NativeRecord {
    fn from(record: &protocol::ChatRecord) -> Self {
        Self {
            body: Some(native_record::Body::Chat(Chat {
                enrichment: record.enrichment.as_ref().map(Enrichment::from),
                id: record.id.clone(),
                provider: record.provider.as_str().to_owned(),
                title: record.title.clone(),
                project: record.project.clone(),
                repository: record.repository.clone(),
                created_at: record.created_at,
                updated_at: record.updated_at,
            })),
        }
    }
}

impl From<&protocol::Amendment> for NativeRecord {
    fn from(amendment: &protocol::Amendment) -> Self {
        let chat = match NativeRecord::from(&amendment.chat).body {
            Some(native_record::Body::Chat(chat)) => Some(chat),
            _ => None,
        };
        Self {
            body: Some(native_record::Body::Amend(Amend {
                chat,
                replaced: amendment.replaced.clone(),
                messages: amendment.messages,
            })),
        }
    }
}

impl TryFrom<Chat> for protocol::ChatRecord {
    type Error = AppError;
    fn try_from(record: Chat) -> Result<Self, Self::Error> {
        Ok(Self {
            enrichment: record.enrichment.map(TryInto::try_into).transpose()?,
            id: record.id,
            provider: record.provider.parse()?,
            title: record.title,
            project: record.project,
            repository: record.repository,
            created_at: record.created_at,
            updated_at: record.updated_at,
        })
    }
}

impl TryFrom<&protocol::MessageRecord> for NativeRecord {
    type Error = AppError;
    fn try_from(record: &protocol::MessageRecord) -> Result<Self, Self::Error> {
        Ok(Self {
            body: Some(native_record::Body::Message(NativeMessage {
                attachments: record.attachments.iter().map(Attachment::from).collect(),
                id: record.id.clone(),
                chat_id: record.chat_id.clone(),
                ordinal: record.ordinal,
                context: Some(MessageContext::from(&record.context)),
                role: record.role.as_str().to_owned(),
                content: record.content.clone(),
                projection: record.projection.clone(),
                created_at: record.created_at,
                generation: record.generation.clone(),
                vector: record.vector.as_ref().map(|values| Vector {
                    values: values.clone(),
                }),
                tools: Some(ToolActivity::try_from(&record.tools)?),
            })),
        })
    }
}

impl TryFrom<NativeMessage> for protocol::MessageRecord {
    type Error = AppError;
    fn try_from(record: NativeMessage) -> Result<Self, Self::Error> {
        Ok(Self {
            attachments: record
                .attachments
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<_, _>>()?,
            id: record.id,
            chat_id: record.chat_id,
            ordinal: record.ordinal,
            context: record
                .context
                .ok_or_else(|| invalid("missing message context"))?
                .try_into()?,
            role: record.role.into(),
            content: record.content,
            projection: record.projection,
            created_at: record.created_at,
            generation: record.generation,
            vector: record.vector.map(|vector| vector.values),
            tools: record
                .tools
                .map_or_else(|| Ok(crate::tools::Activity::default()), TryInto::try_into)?,
        })
    }
}

impl From<&protocol::Tombstone> for NativeRecord {
    fn from(record: &protocol::Tombstone) -> Self {
        Self {
            body: Some(native_record::Body::Tombstone(Tombstone {
                provider: record.provider.as_str().to_owned(),
                chat_id: record.chat_id.clone(),
            })),
        }
    }
}

impl TryFrom<Tombstone> for protocol::Tombstone {
    type Error = AppError;
    fn try_from(record: Tombstone) -> Result<Self, Self::Error> {
        Ok(Self {
            provider: record.provider.parse()?,
            chat_id: record.chat_id,
        })
    }
}

impl From<&crate::message_context::MessageContext> for MessageContext {
    fn from(context: &crate::message_context::MessageContext) -> Self {
        match context {
            crate::message_context::MessageContext::Linear => Self {
                kind: 0,
                parent_id: None,
            },
            crate::message_context::MessageContext::Root => Self {
                kind: 1,
                parent_id: None,
            },
            crate::message_context::MessageContext::Parent { id } => Self {
                kind: 2,
                parent_id: Some(id.clone()),
            },
        }
    }
}

impl TryFrom<MessageContext> for crate::message_context::MessageContext {
    type Error = AppError;
    fn try_from(context: MessageContext) -> Result<Self, Self::Error> {
        match (context.kind, context.parent_id) {
            (0, None) => Ok(Self::Linear),
            (1, None) => Ok(Self::Root),
            (2, Some(id)) if !id.is_empty() => Ok(Self::Parent { id }),
            _ => Err(invalid("invalid message context")),
        }
    }
}

impl From<&crate::attachments::Metadata> for Attachment {
    fn from(record: &crate::attachments::Metadata) -> Self {
        use crate::attachments::Availability;
        Self {
            id: record.id.clone(),
            name: record.name.clone(),
            media_type: record.media_type.clone(),
            availability: match record.availability {
                Availability::NotUploaded => 0,
                Availability::Retained => 1,
                Availability::Missing => 2,
                Availability::NotAllowed => 3,
                Availability::TooLarge => 4,
                Availability::Invalid => 5,
                Availability::External => 6,
            },
            bytes: record.bytes,
            digest: record.digest.clone(),
            redacted: record.redacted,
        }
    }
}

impl TryFrom<Attachment> for crate::attachments::Metadata {
    type Error = AppError;
    fn try_from(record: Attachment) -> Result<Self, Self::Error> {
        use crate::attachments::Availability;
        Ok(Self {
            id: record.id,
            name: record.name,
            media_type: record.media_type,
            availability: match record.availability {
                0 => Availability::NotUploaded,
                1 => Availability::Retained,
                2 => Availability::Missing,
                3 => Availability::NotAllowed,
                4 => Availability::TooLarge,
                5 => Availability::Invalid,
                6 => Availability::External,
                _ => return Err(invalid("invalid attachment availability")),
            },
            bytes: record.bytes,
            digest: record.digest,
            redacted: record.redacted,
        })
    }
}

impl TryFrom<&crate::tools::Activity> for ToolActivity {
    type Error = AppError;
    fn try_from(activity: &crate::tools::Activity) -> Result<Self, Self::Error> {
        Ok(Self {
            calls: activity
                .tool_calls
                .iter()
                .map(|call| {
                    Ok(ToolCall {
                        name: call.name.clone(),
                        call_id: call.call_id.clone(),
                        input_json: call
                            .input
                            .as_ref()
                            .map(serde_json::to_string)
                            .transpose()
                            .map_err(|_| AppError::internal("encode opaque tool input"))?,
                    })
                })
                .collect::<Result<_, AppError>>()?,
            results: activity
                .tool_results
                .iter()
                .map(|result| ToolResult {
                    call_id: result.call_id.clone(),
                    is_error: result.is_error,
                })
                .collect(),
        })
    }
}

impl TryFrom<ToolActivity> for crate::tools::Activity {
    type Error = AppError;
    fn try_from(activity: ToolActivity) -> Result<Self, Self::Error> {
        Ok(Self {
            tool_calls: activity
                .calls
                .into_iter()
                .map(|call| {
                    Ok(crate::tools::ToolCall {
                        name: call.name,
                        call_id: call.call_id,
                        input: call
                            .input_json
                            .map(|json| {
                                serde_json::from_str(&json)
                                    .map_err(|_| invalid("invalid opaque tool JSON"))
                            })
                            .transpose()?,
                    })
                })
                .collect::<Result<_, AppError>>()?,
            tool_results: activity
                .results
                .into_iter()
                .map(|result| crate::tools::ToolResult {
                    call_id: result.call_id,
                    is_error: result.is_error,
                })
                .collect(),
        })
    }
}

impl From<&crate::views::config::Build> for ViewsBuild {
    fn from(build: &crate::views::config::Build) -> Self {
        use crate::views::config::{
            DenseContent, EmbeddingText, Representations, RetrievalUnit, TextPreparation,
        };
        Self {
            retrieval_unit: match build.retrieval_unit {
                RetrievalUnit::Message => 0,
                RetrievalUnit::ParentAndMessage => 1,
            },
            dense_content: match build.dense_content {
                DenseContent::AllVisible => 0,
                DenseContent::UserMessages => 1,
            },
            text_preparation: match build.text_preparation {
                TextPreparation::Preserve => 0,
                TextPreparation::CollapseWhitespace => 1,
            },
            representations: match build.representations {
                Representations::LexicalAndDense => 0,
                Representations::LexicalOnly => 1,
            },
            embedding_text: match build.embedding_text {
                EmbeddingText::Dialogue => 0,
                EmbeddingText::TitleAndDialogue => 1,
            },
            max_embedding_chars: build.max_embedding_chars.map(Into::into),
        }
    }
}

impl TryFrom<ViewsBuild> for crate::views::config::Build {
    type Error = AppError;
    fn try_from(build: ViewsBuild) -> Result<Self, Self::Error> {
        use crate::views::config::{
            DenseContent, EmbeddingText, Representations, RetrievalUnit, TextPreparation,
        };
        Ok(Self {
            retrieval_unit: match build.retrieval_unit {
                0 => RetrievalUnit::Message,
                1 => RetrievalUnit::ParentAndMessage,
                _ => return Err(invalid("invalid retrieval unit")),
            },
            dense_content: match build.dense_content {
                0 => DenseContent::AllVisible,
                1 => DenseContent::UserMessages,
                _ => return Err(invalid("invalid dense content")),
            },
            text_preparation: match build.text_preparation {
                0 => TextPreparation::Preserve,
                1 => TextPreparation::CollapseWhitespace,
                _ => return Err(invalid("invalid text preparation")),
            },
            representations: match build.representations {
                0 => Representations::LexicalAndDense,
                1 => Representations::LexicalOnly,
                _ => return Err(invalid("invalid representations")),
            },
            embedding_text: match build.embedding_text {
                0 => EmbeddingText::Dialogue,
                1 => EmbeddingText::TitleAndDialogue,
                _ => return Err(invalid("invalid embedding text")),
            },
            max_embedding_chars: build
                .max_embedding_chars
                .map(TryInto::try_into)
                .transpose()
                .map_err(|_| invalid("invalid embedding text budget"))?,
        })
    }
}

impl From<&crate::silver::certificate::Certificate> for SourceCertificate {
    fn from(certificate: &crate::silver::certificate::Certificate) -> Self {
        use crate::silver::certificate::Completeness;
        Self {
            revision: certificate.revision,
            interpretation_version: certificate.interpretation_version,
            completeness: match certificate.completeness {
                Completeness::Unverified => 0,
                Completeness::Pending => 1,
                Completeness::Incomplete => 2,
                Completeness::Complete => 3,
            },
            rejected_sources: certificate.rejected_sources,
        }
    }
}

impl TryFrom<SourceCertificate> for crate::silver::certificate::Certificate {
    type Error = AppError;
    fn try_from(certificate: SourceCertificate) -> Result<Self, Self::Error> {
        use crate::silver::certificate::Completeness;
        Ok(Self {
            revision: certificate.revision,
            interpretation_version: certificate.interpretation_version,
            completeness: match certificate.completeness {
                0 => Completeness::Unverified,
                1 => Completeness::Pending,
                2 => Completeness::Incomplete,
                3 => Completeness::Complete,
                _ => return Err(invalid("invalid source completeness")),
            },
            rejected_sources: certificate.rejected_sources,
        })
    }
}

impl From<&protocol::Start> for PublicationStart {
    fn from(start: &protocol::Start) -> Self {
        Self {
            views_build: start.views_build.as_ref().map(ViewsBuild::from),
            source_silver: start.source_silver.as_ref().map(SourceCertificate::from),
            instance: start.instance.clone(),
            revision: start.revision,
            base_revision: start.base_revision,
            snapshot: start.snapshot,
            forgets: start.forgets.clone(),
        }
    }
}

impl TryFrom<PublicationStart> for protocol::Start {
    type Error = AppError;
    fn try_from(start: PublicationStart) -> Result<Self, Self::Error> {
        Ok(Self {
            views_build: start.views_build.map(TryInto::try_into).transpose()?,
            source_silver: start.source_silver.map(TryInto::try_into).transpose()?,
            instance: start.instance,
            revision: start.revision,
            base_revision: start.base_revision,
            snapshot: start.snapshot,
            forgets: start.forgets,
        })
    }
}

impl From<&crate::silver::enrichment::Evidence> for Enrichment {
    fn from(evidence: &crate::silver::enrichment::Evidence) -> Self {
        Self {
            version: evidence.version,
            title: evidence.title.as_ref().map(EnrichmentOrigin::from),
            project: evidence.project.as_ref().map(EnrichmentOrigin::from),
            repository: evidence.repository.as_ref().map(EnrichmentOrigin::from),
            timestamps: evidence
                .timestamps
                .as_ref()
                .map(|times| EnrichmentTimestamps {
                    created_at: times.created_at.as_ref().map(TimestampOrigin::from),
                    updated_at: times.updated_at.as_ref().map(TimestampOrigin::from),
                }),
        }
    }
}

impl TryFrom<Enrichment> for crate::silver::enrichment::Evidence {
    type Error = AppError;
    fn try_from(evidence: Enrichment) -> Result<Self, Self::Error> {
        Ok(Self {
            version: evidence.version,
            title: evidence.title.map(TryInto::try_into).transpose()?,
            project: evidence.project.map(TryInto::try_into).transpose()?,
            repository: evidence.repository.map(TryInto::try_into).transpose()?,
            timestamps: evidence
                .timestamps
                .map(|times| {
                    Ok(crate::silver::enrichment::Timestamps {
                        created_at: times.created_at.map(TryInto::try_into).transpose()?,
                        updated_at: times.updated_at.map(TryInto::try_into).transpose()?,
                    })
                })
                .transpose()?,
        })
    }
}

impl From<&crate::silver::enrichment::Origin> for EnrichmentOrigin {
    fn from(origin: &crate::silver::enrichment::Origin) -> Self {
        use crate::silver::enrichment::{Origin, SourceField};
        let kind = match origin {
            Origin::Unknown => enrichment_origin::Kind::Unknown(true),
            Origin::SourceMetadata { field } => enrichment_origin::Kind::SourceField(
                match field {
                    SourceField::Cwd => "cwd",
                    SourceField::SessionTitle => "session.title",
                    SourceField::ThreadTitle => "threads.title",
                    SourceField::SessionDirectory => "session.directory",
                    SourceField::ProjectWorktree => "project.worktree",
                }
                .to_owned(),
            ),
            Origin::FirstVisibleUserMessage {
                message_id,
                rule_version,
            } => enrichment_origin::Kind::FirstVisible(FirstVisibleMessage {
                message_id: message_id.clone(),
                rule_version: *rule_version,
            }),
        };
        Self { kind: Some(kind) }
    }
}

impl TryFrom<EnrichmentOrigin> for crate::silver::enrichment::Origin {
    type Error = AppError;
    fn try_from(origin: EnrichmentOrigin) -> Result<Self, Self::Error> {
        use crate::silver::enrichment::SourceField;
        match origin
            .kind
            .ok_or_else(|| invalid("missing enrichment origin"))?
        {
            enrichment_origin::Kind::Unknown(true) => Ok(Self::Unknown),
            enrichment_origin::Kind::SourceField(field) => Ok(Self::SourceMetadata {
                field: match field.as_str() {
                    "cwd" => SourceField::Cwd,
                    "session.title" => SourceField::SessionTitle,
                    "threads.title" => SourceField::ThreadTitle,
                    "session.directory" => SourceField::SessionDirectory,
                    "project.worktree" => SourceField::ProjectWorktree,
                    _ => return Err(invalid("invalid enrichment source field")),
                },
            }),
            enrichment_origin::Kind::FirstVisible(message) => Ok(Self::FirstVisibleUserMessage {
                message_id: message.message_id,
                rule_version: message.rule_version,
            }),
            enrichment_origin::Kind::Unknown(_) => Err(invalid("invalid enrichment origin")),
        }
    }
}

impl From<&crate::silver::enrichment::TimestampOrigin> for TimestampOrigin {
    fn from(origin: &crate::silver::enrichment::TimestampOrigin) -> Self {
        use crate::silver::enrichment::{Boundary, TimestampField, TimestampOrigin as Origin};
        let kind = match origin {
            Origin::Unknown => timestamp_origin::Kind::Unknown(true),
            Origin::SourceMetadata {
                field,
                rule_version,
            } => timestamp_origin::Kind::Source(SourceTimestamp {
                field: match field {
                    TimestampField::SessionTimeCreated => "session.time_created",
                    TimestampField::SessionTimeUpdated => "session.time_updated",
                    TimestampField::ThreadCreatedAtMs => "threads.created_at_ms",
                    TimestampField::ThreadUpdatedAtMs => "threads.updated_at_ms",
                    TimestampField::ThreadCreatedAt => "threads.created_at",
                    TimestampField::ThreadUpdatedAt => "threads.updated_at",
                }
                .to_owned(),
                rule_version: *rule_version,
            }),
            Origin::MessageBoundary {
                message_id,
                selection,
                rule_version,
            } => timestamp_origin::Kind::Boundary(MessageBoundary {
                message_id: message_id.clone(),
                maximum: *selection == Boundary::Maximum,
                rule_version: *rule_version,
            }),
        };
        Self { kind: Some(kind) }
    }
}

impl TryFrom<TimestampOrigin> for crate::silver::enrichment::TimestampOrigin {
    type Error = AppError;
    fn try_from(origin: TimestampOrigin) -> Result<Self, Self::Error> {
        use crate::silver::enrichment::{Boundary, TimestampField};
        match origin
            .kind
            .ok_or_else(|| invalid("missing timestamp origin"))?
        {
            timestamp_origin::Kind::Unknown(true) => Ok(Self::Unknown),
            timestamp_origin::Kind::Source(source) => Ok(Self::SourceMetadata {
                field: match source.field.as_str() {
                    "session.time_created" => TimestampField::SessionTimeCreated,
                    "session.time_updated" => TimestampField::SessionTimeUpdated,
                    "threads.created_at_ms" => TimestampField::ThreadCreatedAtMs,
                    "threads.updated_at_ms" => TimestampField::ThreadUpdatedAtMs,
                    "threads.created_at" => TimestampField::ThreadCreatedAt,
                    "threads.updated_at" => TimestampField::ThreadUpdatedAt,
                    _ => return Err(invalid("invalid timestamp source field")),
                },
                rule_version: source.rule_version,
            }),
            timestamp_origin::Kind::Boundary(boundary) => Ok(Self::MessageBoundary {
                message_id: boundary.message_id,
                selection: if boundary.maximum {
                    Boundary::Maximum
                } else {
                    Boundary::Minimum
                },
                rule_version: boundary.rule_version,
            }),
            timestamp_origin::Kind::Unknown(_) => Err(invalid("invalid timestamp origin")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Arbitrary bytes must be rejected, never panic or over-allocate.
        #[test]
        fn arbitrary_frames_are_rejected_without_panicking(
            bytes in prop::collection::vec(any::<u8>(), 0..4096),
        ) {
            let _ = decode_client(&bytes);
            let _ = decode_server(&bytes);
        }

        /// A successful decompression always honours its declared and maximum size.
        #[test]
        fn decompression_never_exceeds_the_requested_output(
            data in prop::collection::vec(any::<u8>(), 0..2048),
            declared in 0usize..65_536,
            limit in 0usize..65_536,
        ) {
            if let Ok(out) = decompress(&data, declared, limit) {
                prop_assert_eq!(out.len(), declared);
                prop_assert!(out.len() <= limit);
            }
        }

        /// Manifest decoding either fails or stays within its member bound.
        #[test]
        fn manifests_reject_arbitrary_bytes(
            bytes in prop::collection::vec(any::<u8>(), 0..2048),
        ) {
            if let Ok(members) = decode_backfill_manifest(&bytes, 32) {
                prop_assert!(members.len() <= 32);
            }
        }
    }

    fn events() -> Vec<Event> {
        vec![
            Event::Batch(protocol::Batch::Chats {
                records: vec![protocol::ChatRecord {
                    enrichment: None,
                    id: "chat".into(),
                    provider: crate::provider::Provider::Codex,
                    title: Some("title".into()),
                    project: None,
                    repository: None,
                    created_at: Some(1),
                    updated_at: Some(2),
                }],
            }),
            Event::Batch(protocol::Batch::Messages {
                records: vec![protocol::MessageRecord {
                    attachments: Vec::new(),
                    id: "message".into(),
                    chat_id: "chat".into(),
                    ordinal: 0,
                    context: crate::message_context::MessageContext::Linear,
                    role: crate::role::Role::User,
                    content: "repeat repeat repeat repeat".into(),
                    projection: "repeat repeat repeat repeat".into(),
                    created_at: Some(1),
                    generation: Some("model".into()),
                    vector: Some(vec![0.25; 384]),
                    tools: crate::tools::Activity::default(),
                }],
            }),
            Event::Remove { id: "old".into() },
        ]
    }

    #[test]
    fn recall_publication_round_trip_preserves_owner_revision_and_provenance() {
        use crate::records::{Evidence, EvidenceUse, transfer::Row};
        let events = vec![Event::Batch(protocol::Batch::Records {
            records: vec![
                Row::Owner { id: "owner".into() },
                Row::Evidence {
                    id: "record/owner/choice".into(),
                    revision: 2,
                    position: 0,
                    source: Evidence {
                        chat_id: "chat".into(),
                        message_id: "approval".into(),
                        fingerprint: "a".repeat(64),
                        quote: "yes".into(),
                        purpose: Some(EvidenceUse::Endorsement),
                        source_role: Some(crate::role::Role::User),
                    },
                },
            ],
        })];
        let encoded = encode_publication(&events, DEFAULT_MAX_DECODED_BYTES).unwrap();
        assert_eq!(encoded.record_count, 2);
        assert_eq!(encoded.chat_count, 0);
        let decoded = decode_publication(&encoded.pieces, DEFAULT_MAX_DECODED_BYTES).unwrap();
        assert_eq!(
            serde_json::to_value(decoded).unwrap(),
            serde_json::to_value(&events).unwrap()
        );
        assert!(encode_publication(&events, 16).is_err());
    }

    #[test]
    fn typed_publication_round_trip_preserves_records_and_optional_fields() {
        let encoded = encode_publication(&events(), DEFAULT_MAX_DECODED_BYTES).unwrap();
        assert_eq!(encoded.record_count, 3);
        assert_eq!(encoded.chat_count, 1);
        let decoded = decode_publication(&encoded.pieces, DEFAULT_MAX_DECODED_BYTES).unwrap();
        assert_eq!(decoded.len(), 3); // removal separates the two record groups
        let rendered = serde_json::to_value(decoded).unwrap();
        let expected = serde_json::to_value(events()).unwrap();
        assert_eq!(rendered, expected);
    }

    #[test]
    fn oversized_batches_use_independently_compressed_bounded_pieces() {
        let mut records = Vec::new();
        for index in 0..200 {
            let mut event = events();
            let Event::Batch(protocol::Batch::Messages { records: messages }) = event.remove(1)
            else {
                panic!()
            };
            let mut message = messages.into_iter().next().unwrap();
            message.id = format!("message-{index}");
            message.content = format!("{index}:{}", "x".repeat(16384));
            message.projection = message.content.clone();
            records.push(message);
        }
        let encoded = encode_publication(
            &[Event::Batch(protocol::Batch::Messages { records })],
            DEFAULT_MAX_DECODED_BYTES,
        )
        .unwrap();
        assert!(encoded.pieces.len() > 1);
        assert!(
            encoded
                .pieces
                .iter()
                .all(|piece| piece.decoded_length <= OVERSIZED_PIECE_TARGET as u64)
        );
        assert!(decode_publication(&encoded.pieces, DEFAULT_MAX_DECODED_BYTES).is_ok());
    }

    #[test]
    fn one_chat_longer_than_the_old_limit_still_publishes() {
        // Real agent sessions hold well over 64 MiB of text in one chat.
        let mut event = events();
        let Event::Batch(protocol::Batch::Messages { records }) = event.remove(1) else {
            panic!()
        };
        let mut message = records.into_iter().next().unwrap();
        message.content = "y".repeat(40 * 1024 * 1024);
        message.projection = message.content.clone();
        let encoded = encode_publication(
            &[Event::Batch(protocol::Batch::Messages {
                records: vec![message],
            })],
            DEFAULT_MAX_DECODED_BYTES,
        )
        .unwrap();
        assert!(encoded.decoded_bytes > 64 * 1024 * 1024);
        assert!(decode_publication(&encoded.pieces, DEFAULT_MAX_DECODED_BYTES).is_ok());
    }

    #[test]
    fn corrupt_or_changed_pieces_are_rejected() {
        let encoded = encode_publication(&events(), DEFAULT_MAX_DECODED_BYTES).unwrap();
        let mut changed = encoded.pieces.clone();
        changed[0].encoded[0] ^= 1;
        assert!(decode_publication(&changed, DEFAULT_MAX_DECODED_BYTES).is_err());
        let mut changed = encoded.pieces.clone();
        changed[0].digest = "0".repeat(64);
        assert!(decode_publication(&changed, DEFAULT_MAX_DECODED_BYTES).is_err());
        assert!(decode_publication(&encoded.pieces, 1).is_err());
    }

    #[test]
    fn control_and_data_frames_have_separate_limits() {
        let hello = ClientFrame {
            operation_id: String::new(),
            body: Some(client_frame::Body::Hello(Hello {
                protocol_version: PROTOCOL_VERSION,
                database_instance: "instance".into(),
                zstd_publication_data: true,
                max_decoded_bytes: DEFAULT_MAX_DECODED_BYTES as u64,
            })),
        };
        let bytes = encode_client(&hello).unwrap();
        assert_eq!(decode_client(&bytes).unwrap(), hello);
        assert!(decode_client(&vec![0; MAX_ENVELOPE_BYTES + 1]).is_err());
        assert!(decode_server(&vec![0; MAX_CONTROL_BYTES + 1]).is_err());
    }
}
