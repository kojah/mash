//! Experimental external-ingestion contract, independent of internal silver tables.
//!
//! These types define the boundary being prepared for external connectors. They
//! do not yet submit batches or modify indexes. See `connectors/README.md` for
//! the proposed Python connector interface and its implementation status.
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Maximum encoded size of an ingestion batch, before JSON decoding.
pub const MAX_BATCH_BYTES: usize = 8 * 1024 * 1024;
/// Maximum number of changes accepted in one atomic silver batch.
pub const MAX_CHANGES: usize = 512;
/// Maximum UTF-8 byte length of one message's canonical text.
pub const MAX_CONTENT_BYTES: usize = 1024 * 1024;
/// Maximum encoded size of one opaque source metadata object.
pub const MAX_METADATA_BYTES: usize = 256 * 1024;

/// Deliberately opaque source-specific properties, never interpreted as instructions.
/// Common searchable fields belong to the typed records instead.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct Metadata(BTreeMap<String, serde_json::Value>);

impl From<BTreeMap<String, serde_json::Value>> for Metadata {
    fn from(value: BTreeMap<String, serde_json::Value>) -> Self {
        Self(value)
    }
}

/// Stable opaque identity. Source, batch and record IDs use the same alphabet.
/// Record IDs are scoped by source; they must not be regenerated on retries.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct Id(String);

impl JsonSchema for Id {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ConnectorId".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // The negative pattern also excludes a terminal newline on regex engines
        // whose `$` anchor matches before it. IDs use the same alphabet everywhere.
        schemars::json_schema!({"type":"string","minLength":1,"maxLength":256,
            "pattern":"^[A-Za-z0-9_][A-Za-z0-9._:/@+\\-]{0,255}$",
            "not":{"pattern":"[^A-Za-z0-9._:/@+\\-]"}})
    }
}

impl Id {
    /// Original identity supplied by the connector.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Id {
    type Error = Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty()
            || value.len() > 256
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.:/@+-".contains(&b))
            || !value.as_bytes()[0].is_ascii_alphanumeric() && value.as_bytes()[0] != b'_'
        {
            return Err(Error::InvalidIdentity);
        }
        Ok(Self(value))
    }
}

impl From<Id> for String {
    fn from(value: Id) -> Self {
        value.0
    }
}

/// Only contract version 1 is currently defined. Unknown versions fail closed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct Version;

impl JsonSchema for Version {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ConnectorVersion".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"integer","const":1})
    }
}

impl TryFrom<u16> for Version {
    type Error = Error;
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        if value == 1 {
            Ok(Self)
        } else {
            Err(Error::UnsupportedVersion)
        }
    }
}

impl From<Version> for u16 {
    fn from(_: Version) -> Self {
        1
    }
}

/// A complete metadata replacement; omitted optional fields become unknown.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Chat {
    pub id: Id,
    pub title: Option<String>,
    pub url: Option<String>,
    /// Unix milliseconds, or unknown. Never inferred from ingestion time.
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
    pub metadata: Option<Metadata>,
}

/// Authorship is separate from agent dialogue roles.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Author {
    pub id: Id,
    pub name: Option<String>,
    pub kind: AuthorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AuthorKind {
    Human,
    Bot,
    Unknown,
}

/// A complete message replacement. Missing messages in a batch are untouched.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub id: Id,
    /// The chat containing this message; chat boundaries belong to the connector.
    pub chat_id: Id,
    pub author: Option<Author>,
    /// Parent/reply message identity within the same source.
    pub reply_to: Option<Id>,
    pub content: String,
    pub url: Option<String>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
    pub metadata: Option<Metadata>,
    /// References only; ingestion must not fetch URLs or execute attachments.
    #[serde(default)]
    #[schemars(length(max = 32))]
    pub attachments: Vec<Attachment>,
}

/// A source attachment reference. Binary retention is a separate explicit operation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Attachment {
    pub id: Id,
    pub name: String,
    pub media_type: String,
    pub url: String,
    pub bytes: Option<u64>,
}

/// Explicit mutations. No implicit deletion from an incomplete enumeration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "op", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Change {
    UpsertChat(Chat),
    UpsertMessage(Message),
    DeleteChat { id: Id },
    DeleteMessage { id: Id },
}

/// Transport-independent batch. Applications must validate decoded instances too.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Batch {
    pub schema_version: Version,
    /// Stable account/dataset identity shared across copies on different machines.
    pub source_id: Id,
    pub batch_id: Id,
    /// Optimistic ordering against the last acknowledged source revision.
    pub expected_revision: u64,
    #[schemars(length(min = 1, max = 512))]
    pub changes: Vec<Change>,
}

/// Receipt for a durably accepted silver batch. Gold readiness is separate.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema_version: Version,
    pub source_id: Id,
    pub batch_id: Id,
    /// Next expected revision for this source at this ingestion destination.
    pub source_revision: u64,
    /// Database-wide canonical silver revision, distinct from source ordering.
    pub silver_revision: u64,
    pub gold_ready: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Error {
    InvalidIdentity,
    UnsupportedVersion,
    InvalidJson,
    BatchTooLarge,
    InvalidChangeCount,
    DuplicateMutation,
    ConflictingChatDeletion,
    SelfReply,
    FieldTooLarge,
    InvalidUrl,
    InvalidAttachment,
    MetadataTooLarge,
    CyclicHierarchy,
    CrossChatReply,
    ConflictingMessageDeletion,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Do not echo source contents or credentials in decoder errors.
        write!(f, "invalid connector batch: {self:?}")
    }
}
impl std::error::Error for Error {}

impl Batch {
    /// Decode bounded input and validate the contract before any storage work.
    ///
    /// # Errors
    /// Returns a content-free error for malformed, oversized or invalid batches.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_BATCH_BYTES {
            return Err(Error::BatchTooLarge);
        }
        let batch: Self = serde_json::from_slice(bytes).map_err(|_| Error::InvalidJson)?;
        batch.validate()?;
        Ok(batch)
    }

    /// Validate semantic constraints on a decoded or directly constructed batch.
    ///
    /// # Errors
    /// Rejects ambiguous mutations and invalid record values; never writes data.
    pub fn validate(&self) -> Result<(), Error> {
        if self.changes.is_empty() || self.changes.len() > MAX_CHANGES {
            return Err(Error::InvalidChangeCount);
        }
        let mut messages = BTreeSet::new();
        let mut chats = BTreeSet::new();
        let mut deleted = BTreeSet::new();
        for change in &self.changes {
            change.validate()?;
            match change {
                Change::UpsertChat(value) => {
                    if !chats.insert(&value.id) {
                        return Err(Error::DuplicateMutation);
                    }
                }
                Change::DeleteChat { id } => {
                    if !chats.insert(id) {
                        return Err(Error::DuplicateMutation);
                    }
                    deleted.insert(id);
                }
                Change::UpsertMessage(value) => {
                    if !messages.insert(&value.id) {
                        return Err(Error::DuplicateMutation);
                    }
                }
                Change::DeleteMessage { id } => {
                    if !messages.insert(id) {
                        return Err(Error::DuplicateMutation);
                    }
                }
            }
        }
        if self.changes.iter().any(|change| {
            matches!(change,
            Change::UpsertMessage(value) if deleted.contains(&value.chat_id))
        }) {
            return Err(Error::ConflictingChatDeletion);
        }
        validate_replies(&self.changes)?;
        serde_json::to_writer(SizeLimit::new(MAX_BATCH_BYTES), self)
            .map_err(|_| Error::BatchTooLarge)
    }
}

impl Change {
    fn validate(&self) -> Result<(), Error> {
        match self {
            Self::UpsertChat(value) => {
                text_field(value.title.as_deref())?;
                url(value.url.as_deref())?;
                metadata(value.metadata.as_ref())
            }
            Self::UpsertMessage(value) => {
                if value.reply_to.as_ref() == Some(&value.id) {
                    return Err(Error::SelfReply);
                }
                if value.content.len() > MAX_CONTENT_BYTES {
                    return Err(Error::FieldTooLarge);
                }
                url(value.url.as_deref())?;
                if let Some(author) = &value.author {
                    text_field(author.name.as_deref())?;
                }
                attachments(&value.attachments)?;
                metadata(value.metadata.as_ref())
            }
            Self::DeleteMessage { .. } | Self::DeleteChat { .. } => Ok(()),
        }
    }
}

/// Check relationships visible in this batch. Ingestion must separately check
/// the final stored graph, including parents supplied by earlier batches.
fn validate_replies(changes: &[Change]) -> Result<(), Error> {
    let mut messages = BTreeMap::new();
    let mut replies = BTreeMap::new();
    let mut deleted_messages = BTreeSet::new();
    for change in changes {
        match change {
            Change::UpsertMessage(message) => {
                messages.insert(&message.id, message);
                replies.insert(&message.id, message.reply_to.as_ref());
            }
            Change::DeleteMessage { id } => {
                deleted_messages.insert(id);
            }
            Change::UpsertChat(_) | Change::DeleteChat { .. } => {}
        }
    }
    for message in messages.values() {
        if let Some(parent) = &message.reply_to {
            if deleted_messages.contains(parent) {
                return Err(Error::ConflictingMessageDeletion);
            }
            if messages
                .get(parent)
                .is_some_and(|parent| parent.chat_id != message.chat_id)
            {
                return Err(Error::CrossChatReply);
            }
        }
    }
    acyclic(&replies)
}

/// Iterative traversal avoids recursion on adversarially deep reply chains.
fn acyclic(parents: &BTreeMap<&Id, Option<&Id>>) -> Result<(), Error> {
    let mut complete = BTreeSet::new();
    for start in parents.keys() {
        let mut path = BTreeSet::new();
        let mut current = Some(*start);
        while let Some(id) = current {
            if complete.contains(id) {
                break;
            }
            if !path.insert(id) {
                return Err(Error::CyclicHierarchy);
            }
            current = parents.get(id).copied().flatten();
        }
        complete.extend(path);
    }
    Ok(())
}

fn attachments(values: &[Attachment]) -> Result<(), Error> {
    let mut ids = BTreeSet::new();
    if values.len() > 32 {
        return Err(Error::InvalidAttachment);
    }
    for value in values {
        if !ids.insert(&value.id)
            || value.media_type.is_empty()
            || value.media_type.len() > 128
            || !value.media_type.contains('/')
            || !value.media_type.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(Error::InvalidAttachment);
        }
        text_field(Some(&value.name))?;
        url(Some(&value.url))?;
    }
    Ok(())
}

/// Count canonical wire bytes without allocating a second copy of the batch.
struct SizeLimit {
    bytes: usize,
    limit: usize,
}

impl SizeLimit {
    fn new(limit: usize) -> Self {
        Self { bytes: 0, limit }
    }
}

impl std::io::Write for SizeLimit {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes = self.bytes.saturating_add(bytes.len());
        if self.bytes > self.limit {
            return Err(std::io::Error::other("connector batch exceeds byte budget"));
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn metadata(value: Option<&Metadata>) -> Result<(), Error> {
    if let Some(value) = value {
        serde_json::to_writer(SizeLimit::new(MAX_METADATA_BYTES), value)
            .map_err(|_| Error::MetadataTooLarge)?;
    }
    Ok(())
}

fn text_field(value: Option<&str>) -> Result<(), Error> {
    if value.is_some_and(|v| v.len() > 4096) {
        Err(Error::FieldTooLarge)
    } else {
        Ok(())
    }
}

fn url(value: Option<&str>) -> Result<(), Error> {
    if let Some(value) = value {
        if value.len() > 4096 {
            return Err(Error::FieldTooLarge);
        }
        let parsed = reqwest::Url::parse(value).map_err(|_| Error::InvalidUrl)?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(Error::InvalidUrl);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
