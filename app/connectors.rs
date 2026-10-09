//! Experimental external-ingestion contract, independent of internal silver tables.
//!
//! These types define the boundary being prepared for external connectors. They
//! do not yet submit batches or modify indexes. See `connectors/README.md` for
//! the proposed Python connector interface and its implementation status.
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub mod silver;

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

/// Contract version 2: generic objects. Other versions fail closed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct Version;

impl JsonSchema for Version {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ConnectorVersion".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"integer","const":2})
    }
}

impl TryFrom<u16> for Version {
    type Error = Error;
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        if value == 2 {
            Ok(Self)
        } else {
            Err(Error::UnsupportedVersion)
        }
    }
}

impl From<Version> for u16 {
    fn from(_: Version) -> Self {
        2
    }
}

/// What Mash may do with an object, whatever its source calls it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    /// Groups results and owns context, such as a chat, channel or thread.
    Container,
    /// A structural part of a container, such as a turn or review: it gives
    /// context and ancestry, but results still group under the container.
    Segment,
    /// Searchable text with an author, such as a message or comment.
    Entry,
    /// Standalone searchable text, such as a note; never a parent.
    Document,
}

/// A namespaced source label such as `discord:reply-thread`. Mash shows and
/// filters on it but gives it no behaviour; `role` carries the semantics.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct Kind(String);

impl JsonSchema for Kind {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ConnectorKind".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"string","minLength":3,"maxLength":97,
            "pattern":"^[a-z0-9][a-z0-9-]{0,31}:[a-z0-9][a-z0-9-]{0,63}$",
            "not":{"pattern":"[^a-z0-9:-]"}})
    }
}

impl Kind {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Kind {
    type Error = Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let label = |part: &str, limit: usize| {
            !part.is_empty()
                && part.len() <= limit
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && part.as_bytes()[0] != b'-'
        };
        match value.split_once(':') {
            Some((namespace, name)) if label(namespace, 32) && label(name, 64) => Ok(Self(value)),
            _ => Err(Error::InvalidKind),
        }
    }
}

impl From<Kind> for String {
    fn from(value: Kind) -> Self {
        value.0
    }
}

/// A complete object replacement; omitted optional fields become unknown.
/// Which fields apply depends on `role`; see [`Object::validate`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Object {
    pub id: Id,
    pub role: Role,
    pub kind: Kind,
    /// The containing object within this source; required for entries.
    /// Deleting a parent deletes its descendants.
    pub parent: Option<Id>,
    pub title: Option<String>,
    /// Entries and documents only; required for them.
    pub content: Option<String>,
    /// Entries only.
    pub author: Option<Author>,
    /// Entries only: the entry this replies to, within the same source.
    pub reply_to: Option<Id>,
    pub url: Option<String>,
    /// Unix milliseconds, or unknown. Never inferred from ingestion time.
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
    pub metadata: Option<Metadata>,
    /// References only; ingestion must not fetch URLs or execute attachments.
    /// Entries and documents only.
    #[serde(default)]
    #[schemars(length(max = 32))]
    pub attachments: Vec<Attachment>,
}

/// Authorship is separate from agent dialogue roles.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Author {
    pub id: Id,
    pub name: Option<String>,
    pub kind: AuthorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AuthorKind {
    Human,
    Bot,
    Unknown,
}

/// The topology a source commits to. Once declared, every object of the
/// source must use a declared kind with its role and an allowed parent kind.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Schema {
    #[schemars(length(min = 1, max = 256))]
    pub kinds: Vec<KindRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KindRule {
    pub kind: Kind,
    pub role: Role,
    /// Declared kinds this kind may sit directly under.
    #[serde(default)]
    #[schemars(length(max = 256))]
    pub parents: Vec<Kind>,
    /// Whether objects of this kind may have no parent.
    #[serde(default)]
    pub root: bool,
}

impl Schema {
    /// A schema must be self-consistent before it can constrain objects.
    ///
    /// # Errors
    /// Rejects duplicate or undeclared kinds, document parents, and entries or
    /// segments that could sit outside every container.
    pub fn validate(&self) -> Result<(), Error> {
        let mut roles = BTreeMap::new();
        if self.kinds.is_empty() || self.kinds.len() > 256 {
            return Err(Error::InvalidSchema);
        }
        for rule in &self.kinds {
            if roles.insert(&rule.kind, rule.role).is_some() || rule.parents.len() > 256 {
                return Err(Error::InvalidSchema);
            }
        }
        for rule in &self.kinds {
            let needs_parent = matches!(rule.role, Role::Entry | Role::Segment);
            if (needs_parent && rule.root) || (!rule.root && rule.parents.is_empty()) {
                return Err(Error::InvalidSchema);
            }
            for parent in &rule.parents {
                match roles.get(parent) {
                    None | Some(Role::Document) => return Err(Error::InvalidSchema),
                    Some(_) => {}
                }
            }
        }
        Ok(())
    }

    /// The rule for a kind, if declared.
    #[must_use]
    pub fn rule(&self, kind: &Kind) -> Option<&KindRule> {
        self.kinds.iter().find(|rule| &rule.kind == kind)
    }

    /// Whether an object of `kind` and `role` may sit under a parent of
    /// `parent_kind` (`None` for a top-level object).
    #[must_use]
    pub fn admits(&self, kind: &Kind, role: Role, parent_kind: Option<&Kind>) -> bool {
        self.rule(kind).is_some_and(|rule| {
            rule.role == role
                && match parent_kind {
                    None => rule.root,
                    Some(parent) => rule.parents.contains(parent),
                }
        })
    }
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
// Batches are short-lived wire values; boxing objects would only add indirection.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "op", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Change {
    UpsertObject(Object),
    /// Deletes the object and every descendant.
    DeleteObject {
        id: Id,
    },
    /// Replaces the source's schema; all its objects are re-checked.
    DeclareSchema(Schema),
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
    /// An object is upserted under, or replies to, an object deleted in the same batch.
    ConflictingDeletion,
    InvalidKind,
    /// A field that the object's role does not allow, or a missing required one.
    InvalidRoleFields,
    /// An entry without a parent, or a document used as a parent.
    InvalidParent,
    SelfReply,
    FieldTooLarge,
    InvalidUrl,
    InvalidAttachment,
    MetadataTooLarge,
    CyclicHierarchy,
    /// A reply to an object in this batch that is not an entry.
    InvalidReply,
    InvalidSchema,
    /// An object that the source's declared schema does not admit.
    SchemaViolation,
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
        let mut seen = BTreeSet::new();
        let mut objects = BTreeMap::new();
        let mut deleted = BTreeSet::new();
        let mut schema = None;
        for change in &self.changes {
            change.validate()?;
            let id = match change {
                Change::DeclareSchema(declared) => {
                    if schema.replace(declared).is_some() {
                        return Err(Error::DuplicateMutation);
                    }
                    continue;
                }
                Change::UpsertObject(object) => {
                    objects.insert(&object.id, object);
                    &object.id
                }
                Change::DeleteObject { id } => {
                    deleted.insert(id);
                    id
                }
            };
            if !seen.insert(id) {
                return Err(Error::DuplicateMutation);
            }
        }
        validate_graph(&objects, &deleted)?;
        if let Some(schema) = schema {
            // Parents outside the batch are checked against stored silver.
            for object in objects.values() {
                let admitted = match object.parent.as_ref().map(|parent| objects.get(parent)) {
                    None => schema.admits(&object.kind, object.role, None),
                    Some(Some(parent)) => {
                        schema.admits(&object.kind, object.role, Some(&parent.kind))
                    }
                    Some(None) => schema
                        .rule(&object.kind)
                        .is_some_and(|rule| rule.role == object.role),
                };
                if !admitted {
                    return Err(Error::SchemaViolation);
                }
            }
        }
        serde_json::to_writer(SizeLimit::new(MAX_BATCH_BYTES), self)
            .map_err(|_| Error::BatchTooLarge)
    }
}

impl Change {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        match self {
            Self::UpsertObject(object) => object.validate(),
            Self::DeleteObject { .. } => Ok(()),
            Self::DeclareSchema(schema) => schema.validate(),
        }
    }
}

impl Object {
    /// Field rules that follow from the role alone.
    ///
    /// # Errors
    /// Rejects fields the role does not allow and invalid field values.
    pub fn validate(&self) -> Result<(), Error> {
        let text = matches!(self.role, Role::Entry | Role::Document);
        if text != self.content.is_some()
            || (!text && !self.attachments.is_empty())
            || (self.role != Role::Entry && (self.author.is_some() || self.reply_to.is_some()))
        {
            return Err(Error::InvalidRoleFields);
        }
        if self.role == Role::Entry && self.parent.is_none() {
            return Err(Error::InvalidParent);
        }
        if self.reply_to.as_ref() == Some(&self.id) {
            return Err(Error::SelfReply);
        }
        if self.parent.as_ref() == Some(&self.id) {
            return Err(Error::CyclicHierarchy);
        }
        if self
            .content
            .as_ref()
            .is_some_and(|content| content.len() > MAX_CONTENT_BYTES)
        {
            return Err(Error::FieldTooLarge);
        }
        text_field(self.title.as_deref())?;
        url(self.url.as_deref())?;
        if let Some(author) = &self.author {
            text_field(author.name.as_deref())?;
        }
        attachments(&self.attachments)?;
        metadata(self.metadata.as_ref())
    }
}

/// Check relationships visible in this batch. Ingestion must separately check
/// the final stored graph, including objects supplied by earlier batches.
fn validate_graph(objects: &BTreeMap<&Id, &Object>, deleted: &BTreeSet<&Id>) -> Result<(), Error> {
    let mut parents = BTreeMap::new();
    let mut replies = BTreeMap::new();
    for object in objects.values() {
        for target in [object.parent.as_ref(), object.reply_to.as_ref()]
            .into_iter()
            .flatten()
        {
            if deleted.contains(target) {
                return Err(Error::ConflictingDeletion);
            }
        }
        if let Some(parent) = &object.parent
            && objects
                .get(parent)
                .is_some_and(|parent| parent.role == Role::Document)
        {
            return Err(Error::InvalidParent);
        }
        if let Some(target) = &object.reply_to
            && objects
                .get(target)
                .is_some_and(|target| target.role != Role::Entry)
        {
            return Err(Error::InvalidReply);
        }
        parents.insert(&object.id, object.parent.as_ref());
        replies.insert(&object.id, object.reply_to.as_ref());
    }
    acyclic(&parents)?;
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
