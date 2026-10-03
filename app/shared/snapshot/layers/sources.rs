//! Source facts survive model changes; live readiness belongs to gold.
use super::Checkpoint;
use crate::provider::Provider;
use serde::{Deserialize, Serialize, ser::SerializeMap};
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
#[serde(from = "Checkpoint")]
pub(in crate::shared) struct Facts {
    provider: Provider,
    path_hash: String,
    size_bytes: i64,
    modified_ns: i64,
    fingerprint: String,
    interpretation_version: u32,
    conversation_id: Option<String>,
}

impl From<Checkpoint> for Facts {
    fn from(source: Checkpoint) -> Self {
        Self {
            provider: source.provider,
            path_hash: source.path_hash,
            size_bytes: source.size_bytes,
            modified_ns: source.modified_ns,
            fingerprint: source.fingerprint,
            interpretation_version: source.interpretation_version,
            conversation_id: source.conversation_id,
        }
    }
}

impl Facts {
    pub(super) fn checkpoint(self) -> Checkpoint {
        Checkpoint {
            provider: self.provider,
            path_hash: self.path_hash,
            size_bytes: self.size_bytes,
            modified_ns: self.modified_ns,
            fingerprint: self.fingerprint,
            interpretation_version: self.interpretation_version,
            conversation_id: self.conversation_id,
            ready: false,
        }
    }
}

#[derive(Serialize)]
struct FactsRef<'a> {
    provider: Provider,
    path_hash: &'a str,
    size_bytes: i64,
    modified_ns: i64,
    fingerprint: &'a str,
    interpretation_version: u32,
    conversation_id: Option<&'a str>,
}

pub(super) struct SourcesRef<'a>(pub(super) &'a BTreeMap<String, Checkpoint>);
impl Serialize for SourcesRef<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (id, source) in self.0 {
            map.serialize_entry(
                id,
                &FactsRef {
                    provider: source.provider,
                    path_hash: &source.path_hash,
                    size_bytes: source.size_bytes,
                    modified_ns: source.modified_ns,
                    fingerprint: &source.fingerprint,
                    interpretation_version: source.interpretation_version,
                    conversation_id: source.conversation_id.as_deref(),
                },
            )?;
        }
        map.end()
    }
}
