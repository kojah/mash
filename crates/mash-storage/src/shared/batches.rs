//! The batches of a publication: what storage reads out for upload and applies on receipt.
use crate::provider::Provider;
pub use crate::views::documents::{ChatRecord, MessageRecord};
use serde::{Deserialize, Serialize};

/// Most message IDs one amendment names; a chat with more changes since the
/// delta's base is resent whole.
pub const MAX_AMENDED_MESSAGES: usize = 1024;

/// A delta's change to one chat the server already holds at the delta's base:
/// the chat's new record, every message ID changed or removed since the base,
/// and the chat's resulting message count. The changed messages that still
/// exist follow as ordinary message batches; the rest of the chat is kept.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Amendment {
    pub chat: ChatRecord,
    pub replaced: Vec<String>,
    pub messages: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Batch {
    Records {
        records: Vec<crate::records::transfer::Row>,
    },
    Tombstones {
        records: Vec<Tombstone>,
    },
    Chats {
        records: Vec<ChatRecord>,
    },
    Messages {
        records: Vec<MessageRecord>,
    },
}

#[derive(Clone, Copy)]
pub enum BatchKind {
    Tombstones,
    Chats,
    Messages,
}

impl Batch {
    /// Masks credentials in everything a batch publishes.
    pub fn redact(&mut self, policy: crate::redaction::Policy) {
        if !policy.enabled() {
            return;
        }
        match self {
            Self::Records { records } => records.iter_mut().for_each(|row| row.redact(policy)),
            Self::Chats { records } => records.iter_mut().for_each(|chat| chat.redact(policy)),
            Self::Messages { records } => {
                for record in records {
                    record.content = crate::redaction::text(&record.content);
                    record.projection = crate::redaction::text(&record.projection);
                    crate::redaction::attachments(&mut record.attachments);
                    record.tools.redact();
                }
            }
            Self::Tombstones { .. } => {}
        }
    }

    pub fn take(&mut self) -> Self {
        match self {
            Self::Records { records } => Self::Records {
                records: std::mem::take(records),
            },
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
    pub fn len(&self) -> usize {
        match self {
            Self::Records { records } => records.len(),
            Self::Tombstones { records } => records.len(),
            Self::Chats { records } => records.len(),
            Self::Messages { records } => records.len(),
        }
    }

    /// Whether the batch carries no records.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl ChatRecord {
    /// Masks credentials in the descriptive fields; enrichment derived from a
    /// masked field is dropped with it.
    pub fn redact(&mut self, policy: crate::redaction::Policy) {
        if !policy.enabled() {
            return;
        }
        let mut changed = false;
        for value in [&mut self.title, &mut self.project, &mut self.repository]
            .into_iter()
            .flatten()
        {
            let safe = crate::redaction::text(value);
            changed |= safe != *value;
            *value = safe;
        }
        if changed {
            self.enrichment = None;
        }
    }

    pub fn validate_enrichment(&self) -> Result<(), crate::AppError> {
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
pub struct Tombstone {
    pub provider: Provider,
    pub chat_id: String,
}
