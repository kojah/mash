//! Explicit reviewed exclusions for ambiguous user-role startup records.
//! No wording-based classification; unknown or changed sources fail closed.
use super::super::{Thread, provenance::Source};
use crate::{AppError, provider::Provider};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Read;

#[cfg(test)]
mod inventory;
#[cfg(test)]
mod tests;

const MAXIMUM_BYTES: usize = 262_144;
const MAXIMUM_SOURCES: usize = 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    sources: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    thread: String,
    provider: Provider,
    message_id: String,
    canonical_blake3: String,
    category: Category,
    review: String,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Category {
    GeneratedStartup,
}

struct Reviewed {
    provider: Provider,
    hash: [u8; 32],
    _category: Category,
}

pub(super) struct Selection {
    sources: BTreeMap<String, BTreeMap<String, Reviewed>>,
    fingerprint: [u8; 32],
}

#[derive(Serialize)]
#[serde(tag = "policy", rename_all = "snake_case")]
pub(super) enum Policy {
    Preserve,
    ReviewedStartup {
        manifest_blake3: [u8; 32],
        reviewed_sources: usize,
    },
}

impl Selection {
    pub(super) fn load() -> Result<Option<Self>, AppError> {
        let Some(path) = std::env::var_os("MASH_TEST_TOPIC_STARTUP_CONTEXT") else {
            return Ok(None);
        };
        let file = std::fs::File::open(path)
            .map_err(|_| AppError::usage("cannot read reviewed startup manifest"))?;
        let mut bytes = Vec::new();
        file.take(u64::try_from(MAXIMUM_BYTES + 1).expect("bounded read"))
            .read_to_end(&mut bytes)
            .map_err(|_| AppError::usage("cannot read reviewed startup manifest"))?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| AppError::usage("invalid reviewed startup manifest"))?;
        Self::parse(text).map(Some)
    }

    fn parse(text: &str) -> Result<Self, AppError> {
        if text.len() > MAXIMUM_BYTES {
            return Err(AppError::usage(
                "reviewed startup manifest exceeds byte limit",
            ));
        }
        let input: Manifest = toml::from_str(text)
            .map_err(|_| AppError::usage("invalid reviewed startup manifest"))?;
        if input.schema_version != 1 || !(1..=MAXIMUM_SOURCES).contains(&input.sources.len()) {
            return Err(AppError::usage("invalid reviewed startup manifest bounds"));
        }
        let mut sources: BTreeMap<String, BTreeMap<String, Reviewed>> = BTreeMap::new();
        for entry in input.sources {
            if [&entry.thread, &entry.message_id]
                .into_iter()
                .any(|id| id.trim().is_empty() || id.len() > 256 || id.contains(['\n', '\r']))
                || entry.review.trim().is_empty()
                || entry.review.len() > 1024
            {
                return Err(AppError::usage("invalid reviewed startup source"));
            }
            let hash = blake3::Hash::from_hex(&entry.canonical_blake3)
                .map_err(|_| AppError::usage("invalid reviewed startup source hash"))?;
            if sources
                .entry(entry.thread)
                .or_default()
                .insert(
                    entry.message_id,
                    Reviewed {
                        provider: entry.provider,
                        hash: *hash.as_bytes(),
                        _category: entry.category,
                    },
                )
                .is_some()
            {
                return Err(AppError::usage("duplicate reviewed startup source"));
            }
        }
        Ok(Self {
            sources,
            fingerprint: *blake3::hash(text.as_bytes()).as_bytes(),
        })
    }

    pub(super) fn validate(&self, threads: &BTreeMap<String, Thread>) -> Result<(), AppError> {
        for (id, sources) in &self.sources {
            let thread = threads
                .get(id)
                .ok_or_else(|| AppError::usage("reviewed startup thread is unavailable"))?;
            for (message, reviewed) in sources {
                if thread.provider != reviewed.provider
                    || thread
                        .user_sources
                        .iter()
                        .filter(|s| s.reviewed_canonical(message, &reviewed.hash))
                        .count()
                        != 1
                {
                    return Err(AppError::usage("reviewed startup source identity changed"));
                }
            }
        }
        Ok(())
    }

    pub(super) fn omits(&self, thread: &str, source: &Source) -> bool {
        let Some((message, _)) = source.anchor() else {
            return false;
        };
        self.sources
            .get(thread)
            .and_then(|s| s.get(message))
            .is_some_and(|reviewed| source.reviewed_canonical(message, &reviewed.hash))
    }

    pub(super) fn policy(selection: Option<&Self>) -> Policy {
        selection.map_or(Policy::Preserve, |selection| Policy::ReviewedStartup {
            manifest_blake3: selection.fingerprint,
            reviewed_sources: selection.sources.values().map(BTreeMap::len).sum(),
        })
    }
}
