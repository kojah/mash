use crate::provider::Provider;
use serde::{Deserialize, Serialize};

use crate::storage::Counts;
pub(crate) use crate::views::documents::{ChatRecord, MessageRecord};
pub(crate) use crate::views::retrieval::{Candidates, Document, Search, valid_vector};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Start {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub views_build: Option<crate::views::config::Build>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_silver: Option<crate::silver::certificate::Certificate>,
    pub instance: String,
    pub revision: Option<i64>,
    pub base_revision: Option<i64>,
    pub snapshot: bool,
    pub forgets: Vec<String>,
}

impl Start {
    pub(super) fn validate_source_silver(&self) -> Result<(), crate::AppError> {
        if let Some(certificate) = &self.source_silver
            && ((!self.snapshot && self.base_revision.is_none())
                || certificate.interpretation_version
                    != crate::silver::certificate::INTERPRETATION_VERSION
                || (certificate.is_complete() && certificate.rejected_sources != 0))
        {
            return Err(crate::AppError::usage("invalid source silver certificate"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod source_certificate_tests {
    use super::*;
    use crate::silver::certificate::{Certificate, Completeness, INTERPRETATION_VERSION};

    #[test]
    fn source_certificates_are_typed_scoped_and_not_views_readiness() {
        let mut start: Start = serde_json::from_str(
            r#"{"instance":"node","revision":1,"base_revision":null,"snapshot":true,"forgets":[]}"#,
        )
        .unwrap();
        assert!(start.source_silver.is_none());
        start.source_silver = Some(Certificate {
            revision: 0,
            interpretation_version: INTERPRETATION_VERSION,
            completeness: Completeness::Pending,
            rejected_sources: 1,
        });
        start.validate_source_silver().unwrap();
        start.source_silver.as_mut().unwrap().completeness = Completeness::Complete;
        assert!(start.validate_source_silver().is_err());
        start.source_silver.as_mut().unwrap().rejected_sources = 0;
        start.validate_source_silver().unwrap();
        start.snapshot = false;
        assert!(start.validate_source_silver().is_err());
        start.base_revision = Some(1);
        start.validate_source_silver().unwrap();
        start.snapshot = true;
        start.source_silver.as_mut().unwrap().interpretation_version += 1;
        assert!(start.validate_source_silver().is_err());
        let malformed = r#"{"revision":0,"interpretation_version":1,"completeness":"complete","rejected_sources":0,"views_model":"invented"}"#;
        assert!(serde_json::from_str::<Certificate>(malformed).is_err());
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Event {
    Start(Start),
    Batch(Batch),
    Remove { id: String },
    Finish,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Batch {
    Tombstones { records: Vec<Tombstone> },
    Chats { records: Vec<ChatRecord> },
    Messages { records: Vec<MessageRecord> },
}

#[derive(Clone, Copy)]
pub(crate) enum BatchKind {
    Tombstones,
    Chats,
    Messages,
}

impl Batch {
    pub(crate) fn take(&mut self) -> Self {
        match self {
            Self::Tombstones { records } => Self::Tombstones {
                records: std::mem::take(records),
            },
            Self::Chats { records } => Self::Chats {
                records: std::mem::take(records),
            },
            Self::Messages { records } => Self::Messages {
                records: std::mem::take(records),
            },
        }
    }
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Tombstones { records } => records.len(),
            Self::Chats { records } => records.len(),
            Self::Messages { records } => records.len(),
        }
    }
}

impl ChatRecord {
    pub(crate) fn validate_enrichment(&self) -> Result<(), crate::AppError> {
        if let Some(evidence) = &self.enrichment {
            if !evidence.is_current()
                || !evidence.matches_metadata_values(
                    self.title.as_deref(),
                    self.project.as_deref(),
                    self.repository.as_deref(),
                    self.created_at,
                    self.updated_at,
                )
            {
                return Err(crate::AppError::usage(
                    "unsupported or invalid silver enrichment evidence",
                ));
            }
            for origin in [&evidence.title, &evidence.project, &evidence.repository] {
                if let Some(crate::silver::enrichment::Origin::FirstVisibleUserMessage {
                    message_id,
                    ..
                }) = origin
                {
                    super::validation::valid_id(message_id)?;
                }
            }
            if let Some(times) = &evidence.timestamps {
                for origin in [&times.created_at, &times.updated_at] {
                    if let Some(crate::silver::enrichment::TimestampOrigin::MessageBoundary {
                        message_id,
                        ..
                    }) = origin
                    {
                        super::validation::valid_id(message_id)?;
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Tombstone {
    pub provider: Provider,
    pub chat_id: String,
}

#[cfg(test)]
#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct NodeState {
    /// Changes on every committed publication, independently of the source counter.
    /// Empty before the first publication.
    #[serde(default)]
    pub publication_version: String,
    #[serde(default)]
    pub views_build: crate::views::config::Build,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub silver_revision: Option<super::silver_revision::State>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_silver: Option<crate::silver::certificate::Certificate>,
    pub instance: Option<String>,
    pub revision: i64,
    pub tombstones: Vec<Tombstone>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Indexed {
    pub counts: Counts,
    pub revision: i64,
    pub removed_messages: u64,
    pub purged_chats: u64,
    /// Whether the node's stored views cover the head after certification.
    /// A certification that finished behind a newer commit reports false.
    #[serde(default)]
    pub views_current: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct View {
    pub node_id: String,
    pub id: String,
    pub context: u32,
    #[serde(
        default,
        skip_serializing_if = "crate::storage::MessageContent::is_canonical"
    )]
    pub content: crate::storage::MessageContent,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChatPage {
    pub node_id: String,
    pub chat_id: String,
    pub offset: u32,
    pub limit: u32,
}

/// A bounded search-only batch; the explicit view API retains larger radii.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Contexts {
    pub anchors: Vec<View>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub(super) enum ContextResult {
    Complete {
        index: usize,
        messages: Vec<crate::storage::Message>,
    },
    Failed {
        index: usize,
        error: crate::AppError,
    },
}

impl ContextResult {
    pub(super) const fn index(&self) -> usize {
        match self {
            Self::Complete { index, .. } | Self::Failed { index, .. } => *index,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batches_validate_record_schema_at_the_protocol_boundary() {
        let line = r#"{"type":"batch","kind":"tombstones","records":[{"provider":"codex","chat_id":"gone"}]}"#;
        let event: Event = serde_json::from_str(line).unwrap();
        let encoded = serde_json::to_string(&event).unwrap();
        let Event::Batch(Batch::Tombstones { records }) = serde_json::from_str(&encoded).unwrap()
        else {
            panic!("typed tombstone batch")
        };
        assert_eq!(records[0].chat_id, "gone");
        for invalid in [
            r#"{"type":"batch","kind":"messages","records":[{"provider":"codex","chat_id":"gone"}]}"#,
            r#"{"type":"batch","kind":"unknown","records":[]}"#,
            r#"{"type":"batch","kind":"tombstones","records":[{"provider":"unknown","chat_id":"gone"}]}"#,
            r#"{"type":"batch","kind":"tombstones","records":[{"provider":"codex","chat_id":3}]}"#,
            r#"{"type":"batch","kind":"tombstones","records":[],"unexpected":true}"#,
        ] {
            assert!(serde_json::from_str::<Event>(invalid).is_err(), "{invalid}");
        }
    }
}
