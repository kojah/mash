//! Storage-independent integrity and completeness checks; retain identities, not bodies.
use super::{Reader, Record, ScanState};
use crate::{
    AppError,
    provider::Provider,
    records::Counts,
    silver::certificate::{Certificate, Completeness, INTERPRETATION_VERSION},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    io::BufRead,
};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Summary {
    pub(crate) bytes: u64,
    pub(crate) digest: String,
    pub(crate) silver: Certificate,
    pub(crate) counts: Counts,
    pub(crate) attachment_files: u64,
    pub(crate) attachment_bytes: u64,
}

#[derive(Default)]
struct Integrity {
    threads: HashSet<String>,
    providers: HashSet<Provider>,
    owners: HashSet<(Provider, String)>,
    messages: HashSet<String>,
    ordinals: HashSet<(String, i64)>,
    enriched: HashSet<String>,
    sources: HashSet<String>,
    source_messages: HashSet<(String, String)>,
    checkpoints: HashSet<(Provider, String)>,
    rejected: HashSet<(Provider, String)>,
    tombstones: HashSet<(Provider, String)>,
    forgets: HashSet<(String, String)>,
    scans: BTreeMap<Provider, ScanState>,
    files: BTreeMap<String, u64>,
    searchable: u64,
    searchable_users: u64,
    unverified: bool,
    policy_seen: bool,
    state: Option<(u64, u32)>,
}

fn invalid() -> AppError {
    AppError::usage("inconsistent canonical archive records")
}

pub(crate) fn inspect(input: impl BufRead) -> Result<Summary, AppError> {
    let mut reader = Reader::new(input);
    let mut integrity = Integrity::default();
    for record in reader.by_ref() {
        integrity.observe(&record?)?;
    }
    let (bytes, digest) = reader.finish()?;
    let (revision, interpretation_version) = integrity.state.ok_or_else(invalid)?;
    let unverified = integrity.unverified
        || interpretation_version != INTERPRETATION_VERSION
        || integrity
            .providers
            .iter()
            .any(|p| !integrity.scans.contains_key(p));
    let completeness = if integrity
        .scans
        .values()
        .any(|s| matches!(s, ScanState::Pending))
    {
        Completeness::Pending
    } else if unverified {
        Completeness::Unverified
    } else if !integrity.rejected.is_empty()
        || integrity
            .scans
            .values()
            .any(|s| matches!(s, ScanState::Incomplete))
    {
        Completeness::Incomplete
    } else {
        Completeness::Complete
    };
    Ok(Summary {
        bytes,
        digest,
        silver: Certificate {
            revision,
            interpretation_version,
            completeness,
            rejected_sources: integrity.rejected.len() as u64,
        },
        counts: Counts {
            conversations: integrity.threads.len() as u64,
            messages: integrity.messages.len() as u64,
            searchable_messages: integrity.searchable,
            searchable_user_messages: Some(integrity.searchable_users),
            embeddings: 0,
        },
        attachment_files: integrity.files.len() as u64,
        attachment_bytes: integrity
            .files
            .values()
            .try_fold(0_u64, |sum, bytes| sum.checked_add(*bytes))
            .ok_or_else(invalid)?,
    })
}

impl Integrity {
    #[allow(
        clippy::too_many_lines,
        reason = "one exhaustive integrity boundary for the portable record vocabulary"
    )]
    fn observe(&mut self, record: &Record) -> Result<(), AppError> {
        match record {
            Record::State {
                revision,
                interpretation_version,
            } => {
                self.state = Some((*revision, *interpretation_version));
            }
            Record::Conversation {
                id,
                provider,
                source_key,
                ..
            } => {
                if !self.threads.insert(id.clone())
                    || !self.owners.insert((*provider, source_key.clone()))
                {
                    return Err(invalid());
                }
                self.providers.insert(*provider);
            }
            Record::Message {
                id,
                conversation_id,
                ordinal,
                content,
                dialogue_text,
                role,
                attachments,
                ..
            } => {
                if !self.threads.contains(conversation_id)
                    || !self.messages.insert(id.clone())
                    || !self.ordinals.insert((conversation_id.clone(), *ordinal))
                {
                    return Err(invalid());
                }
                if !dialogue_text.as_deref().unwrap_or(content).is_empty() {
                    self.searchable += 1;
                    self.searchable_users += u64::from(*role == crate::role::Role::User);
                }
                self.attachments(attachments)?;
            }
            Record::Enrichment {
                conversation_id, ..
            } => {
                if !self.threads.contains(conversation_id)
                    || !self.enriched.insert(conversation_id.clone())
                {
                    return Err(invalid());
                }
            }
            Record::ClaudeSource { source_key, .. } => {
                if !self.sources.insert(source_key.clone()) {
                    return Err(invalid());
                }
            }
            Record::ClaudeMessage {
                source_key,
                message_id,
                attachments,
                ..
            } => {
                if !self.sources.contains(source_key)
                    || !self
                        .source_messages
                        .insert((source_key.clone(), message_id.clone()))
                {
                    return Err(invalid());
                }
                self.attachments(attachments)?;
            }
            Record::Checkpoint {
                provider,
                source_key,
                interpretation_version,
                ..
            } => {
                if !self.checkpoints.insert((*provider, source_key.clone())) {
                    return Err(invalid());
                }
                self.unverified |= *interpretation_version != INTERPRETATION_VERSION;
            }
            Record::Scan {
                provider,
                interpretation_version,
                state,
            } => {
                if self.scans.insert(*provider, *state).is_some() {
                    return Err(invalid());
                }
                self.unverified |= *interpretation_version != INTERPRETATION_VERSION;
            }
            Record::Rejection {
                provider,
                source_key,
                ..
            } => {
                if !self.rejected.insert((*provider, source_key.clone())) {
                    return Err(invalid());
                }
            }
            Record::Tombstone {
                provider,
                conversation_id,
                ..
            } => {
                if !self.tombstones.insert((*provider, conversation_id.clone())) {
                    return Err(invalid());
                }
            }
            Record::AttachmentPolicy { .. } => {
                if self.policy_seen {
                    return Err(invalid());
                }
                self.policy_seen = true;
            }
            Record::SharedForget {
                scope,
                conversation_id,
            } => {
                if !self
                    .forgets
                    .insert((scope.clone(), conversation_id.clone()))
                {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }

    fn attachments(&mut self, values: &[crate::attachments::Metadata]) -> Result<(), AppError> {
        for value in values {
            if value.availability == crate::attachments::Availability::Retained {
                let digest = value.digest.as_ref().ok_or_else(invalid)?;
                let bytes = value.bytes.ok_or_else(invalid)?;
                if self
                    .files
                    .insert(digest.clone(), bytes)
                    .is_some_and(|previous| previous != bytes)
                {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }
}
