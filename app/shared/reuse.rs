//! Dependency-checked reuse of shared gold, without another canonical body copy.
use super::{
    protocol::{self, ConversationRecord, MessageRecord},
    server::Identity,
    snapshot::{Data, Head, Loaded},
    validation::conflict,
};
use crate::{AppError, gold::config::Build};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Target {
    pub(super) schema_version: u32,
    pub(super) build: Build,
    pub(super) generation: String,
    pub(super) dimensions: usize,
    pub(super) publication_version: String,
    pub(super) instance: Option<String>,
    pub(super) publication_revision: i64,
    pub(super) threads: BTreeMap<String, String>,
}

impl Target {
    pub(super) fn validate(&self) -> Result<(), AppError> {
        if self.schema_version != SCHEMA_VERSION
            || self.publication_revision < 0
            || self.dimensions == 0
            || self.dimensions > 2048
            || self.threads.values().any(|hash| !valid_fingerprint(hash))
        {
            return Err(AppError::shared("invalid shared publication plan"));
        }
        Ok(())
    }

    pub(super) fn new(
        loaded: &Loaded,
        identity: &Identity,
        build: Build,
    ) -> Result<Self, AppError> {
        let threads = if build.dense_enabled() {
            eligible(loaded, identity, &build)?
        } else {
            BTreeMap::new()
        };
        let mut target = Self::metadata(loaded.head.as_ref(), &loaded.expected, identity, build);
        target.threads = threads;
        Ok(target)
    }

    pub(super) fn metadata(
        head: Option<&Head>,
        version: &str,
        identity: &Identity,
        build: Build,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            build,
            generation: identity.generation.clone(),
            dimensions: identity.dimensions,
            publication_version: version.to_owned(),
            instance: head.map(|h| h.instance.clone()),
            publication_revision: head.map_or(0, |h| h.revision),
            threads: BTreeMap::new(),
        }
    }
}

fn valid_fingerprint(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())
}

/// One thread's ordered canonical dependency stream. No vectors, model identity,
/// checkpoint stamps or retrieval policy enter this digest.
pub(super) struct Fingerprint {
    digest: blake3::Hasher,
    conversation_id: String,
    ordinal: Option<i64>,
}

struct Digest<'a>(&'a mut blake3::Hasher);
impl Write for Digest<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Fingerprint {
    pub(super) fn new(conversation: &ConversationRecord) -> Result<Self, AppError> {
        let mut digest = blake3::Hasher::new();
        digest.update(b"mash-shared-thread-dependencies-v1\0");
        serde_json::to_writer(Digest(&mut digest), conversation)
            .map_err(|_| AppError::internal("fingerprint shared thread"))?;
        digest.update(b"\n");
        Ok(Self {
            digest,
            conversation_id: conversation.id.clone(),
            ordinal: None,
        })
    }

    pub(super) fn push(&mut self, message: &MessageRecord) -> Result<(), AppError> {
        if message.conversation_id != self.conversation_id
            || message.ordinal < 0
            || self
                .ordinal
                .is_some_and(|previous| previous >= message.ordinal)
        {
            return Err(AppError::shared("invalid ordered thread dependencies"));
        }
        serde_json::to_writer(Digest(&mut self.digest), &message.silver_ref())
            .map_err(|_| AppError::internal("fingerprint shared message"))?;
        self.digest.update(b"\n");
        self.ordinal = Some(message.ordinal);
        Ok(())
    }

    pub(super) fn finish(self) -> String {
        self.digest.finalize().to_hex().to_string()
    }
}

pub(super) fn fingerprints(data: &Data) -> Result<BTreeMap<String, String>, AppError> {
    let mut rows = Vec::with_capacity(data.messages.len());
    for (id, message) in &data.messages {
        if id != &message.id || !data.conversations.contains_key(&message.conversation_id) {
            return Err(AppError::shared(
                "invalid shared thread dependency identity",
            ));
        }
        rows.push(message);
    }
    rows.sort_unstable_by(|a, b| {
        (&a.conversation_id, a.ordinal, &a.id).cmp(&(&b.conversation_id, b.ordinal, &b.id))
    });
    let mut rows = rows.into_iter().peekable();
    let mut hashes = BTreeMap::new();
    for (id, conversation) in &data.conversations {
        if id != &conversation.id {
            return Err(AppError::shared("invalid shared conversation identity"));
        }
        let mut fingerprint = Fingerprint::new(conversation)?;
        while let Some(message) = rows.next_if(|m| &m.conversation_id == id) {
            fingerprint.push(message)?;
        }
        hashes.insert(id.clone(), fingerprint.finish());
    }
    Ok(hashes)
}

fn eligible(
    loaded: &Loaded,
    identity: &Identity,
    build: &Build,
) -> Result<BTreeMap<String, String>, AppError> {
    let compatible = loaded.head.as_ref().is_some_and(|head| {
        head.gold_manifest
            .as_ref()
            .is_some_and(|m| m.matches(build, &identity.generation, head.silver_revision.as_ref()))
    });
    if !compatible {
        return Ok(BTreeMap::new());
    }
    let mut hashes = fingerprints(&loaded.data)?;
    for message in loaded
        .data
        .messages
        .values()
        .filter(|m| !m.projection.is_empty())
    {
        if message.generation.as_deref() != Some(&identity.generation)
            || message
                .vector
                .as_ref()
                .is_none_or(|v| !protocol::valid_vector(v, identity.dimensions))
        {
            hashes.remove(&message.conversation_id);
        }
    }
    Ok(hashes)
}

#[derive(Default)]
pub(super) struct Retained {
    hashes: BTreeMap<String, String>,
    vectors: BTreeMap<String, Vec<f64>>,
    requests: BTreeSet<String>,
}

impl Retained {
    pub(super) fn take(
        loaded: &mut Loaded,
        identity: &Identity,
        build: &Build,
    ) -> Result<Self, AppError> {
        let hashes = eligible(loaded, identity, build)?;
        let mut vectors = BTreeMap::new();
        for message in loaded.data.messages.values_mut() {
            if hashes.contains_key(&message.conversation_id)
                && let Some(vector) = message.vector.take()
            {
                vectors.insert(message.id.clone(), vector);
            }
        }
        Ok(Self {
            hashes,
            vectors,
            requests: BTreeSet::new(),
        })
    }

    pub(super) fn request(&mut self, id: String, fingerprint: &str) -> Result<(), AppError> {
        if !valid_fingerprint(fingerprint)
            || self.hashes.get(&id).is_none_or(|hash| hash != fingerprint)
            || !self.requests.insert(id)
        {
            return Err(conflict("shared gold reuse is unverified; retry indexing"));
        }
        Ok(())
    }

    pub(super) fn restore(mut self, data: &mut Data, identity: &Identity) -> Result<(), AppError> {
        if self.requests.is_empty() {
            return Ok(());
        }
        let incoming = fingerprints(data)?;
        for id in &self.requests {
            if incoming.get(id) != self.hashes.get(id) {
                return Err(conflict("shared gold dependencies changed; retry indexing"));
            }
        }
        for message in data
            .messages
            .values_mut()
            .filter(|m| self.requests.contains(&m.conversation_id) && !m.projection.is_empty())
        {
            if message.vector.is_some() || message.generation.is_some() {
                return Err(AppError::usage(
                    "reused thread also supplies gold embeddings",
                ));
            }
            let vector = self
                .vectors
                .remove(&message.id)
                .ok_or_else(|| conflict("missing retained shared embedding"))?;
            message.vector = Some(vector);
            message.generation = Some(identity.generation.clone());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
