//! Local canonical persistence. Qdrant owns immutable bytes and the conditional head.
//! Setup is explicit; opening/reading a namespace cannot create an empty index.
use super::{AppError, api, failure, point_id, publish::staging_version};
use crate::silver::archive::{self, Summary};
use serde::{Deserialize, Serialize};
use std::io::BufReader;
use tokio::io::AsyncWriteExt;

#[derive(Clone)]
pub(crate) struct Store {
    api: api::Client,
    collection: String,
    scope: String,
    max_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Head {
    kind: Kind,
    schema_version: u32,
    scope: String,
    pub(crate) version: String,
    chunks: usize,
    pub(crate) last_updated_at: String,
    pub(crate) summary: Summary,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Kind {
    CanonicalHead,
    CanonicalChunk,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Chunk {
    kind: Kind,
    version: String,
    ordinal: usize,
    text: String,
}

pub(crate) struct Verified {
    file: tempfile::NamedTempFile,
    summary: Summary,
}

impl Verified {
    pub(crate) async fn new(
        file: tempfile::NamedTempFile,
        max_bytes: u64,
    ) -> Result<Self, AppError> {
        crate::blocking(move || {
            if file.as_file().metadata().map_err(AppError::io)?.len() > max_bytes {
                return Err(AppError::usage(
                    "canonical history exceeds the local Qdrant snapshot limit",
                ));
            }
            let summary = archive::inspect(BufReader::new(file.reopen().map_err(AppError::io)?))?;
            Ok(Self { file, summary })
        })
        .await
    }
}

impl Store {
    pub(crate) fn new(
        url: &str,
        key: Option<&str>,
        namespace: &str,
        max_bytes: u64,
    ) -> Result<Self, AppError> {
        if namespace.is_empty()
            || namespace.len() > 4096
            || namespace.chars().any(char::is_control)
            || max_bytes == 0
        {
            return Err(AppError::configuration(
                "invalid local Qdrant namespace or snapshot limit",
            ));
        }
        let scope = blake3::hash(namespace.as_bytes()).to_hex().to_string();
        Ok(Self {
            api: api::Client::new(url, key, 4 * 1024 * 1024)?,
            collection: format!("mash_local_{scope}_silver"),
            scope,
            max_bytes,
        })
    }

    #[allow(
        clippy::default_trait_access,
        reason = "the initializer owns the concrete physical memory policy type"
    )]
    pub(crate) async fn initialize(&self) -> Result<(), AppError> {
        self.api.verify_version().await?;
        self.api
            .initialize(&self.collection, None, Default::default())
            .await?;
        Ok(())
    }

    pub(crate) async fn head(&self) -> Result<Option<Head>, AppError> {
        if !self.api.metadata_exists(&self.collection).await? {
            return Ok(None);
        }
        self.read_head().await
    }

    async fn read_head(&self) -> Result<Option<Head>, AppError> {
        let id = point_id(&["canonical-head"]);
        let mut points = self
            .api
            .retrieve::<Head>(&self.collection, std::slice::from_ref(&id))
            .await?;
        if points.len() > 1 {
            return Err(failure());
        }
        let Some(point) = points.pop() else {
            return Ok(None);
        };
        if point.id != id {
            return Err(failure());
        }
        self.validate_head(&point.payload)?;
        Ok(Some(point.payload))
    }

    fn validate_head(&self, head: &Head) -> Result<(), AppError> {
        if head.kind != Kind::CanonicalHead
            || head.schema_version != archive::VERSION
            || head.scope != self.scope
            || !valid_version(&head.version)
            || !valid_version(&head.summary.digest)
            || head.summary.bytes == 0
            || head.summary.bytes > self.max_bytes
            || head.chunks == 0
            || head.chunks as u64 > head.summary.bytes
            || head.summary.counts.embeddings != 0
        {
            return Err(AppError::schema(
                "incompatible or corrupt local Qdrant canonical head",
            ));
        }
        Ok(())
    }

    /// Caller supplies the version it read; `None` means an empty destination.
    /// Never retry an ambiguous conditional write or reclaim its staged bytes.
    pub(crate) async fn publish(
        &self,
        incoming: Verified,
        expected: Option<&str>,
    ) -> Result<Head, AppError> {
        if incoming.summary.bytes > self.max_bytes {
            return Err(AppError::usage(
                "canonical history exceeds the local Qdrant snapshot limit",
            ));
        }
        let previous = self.read_head().await?;
        if previous.as_ref().map(|head| head.version.as_str()) != expected {
            return Err(crate::shared::validation::conflict(
                "local canonical history changed; retry the operation",
            ));
        }
        if let Some(previous) = previous.filter(|head| head.summary == incoming.summary) {
            let observed = self.read_head().await?.ok_or_else(failure)?;
            if observed.version != previous.version {
                return Err(crate::shared::validation::conflict(
                    "local canonical history changed; retry the operation",
                ));
            }
            return Ok(observed);
        }
        let version = staging_version()?;
        let chunks = self.stage(&incoming, &version).await?;
        let head = Head {
            kind: Kind::CanonicalHead,
            schema_version: archive::VERSION,
            scope: self.scope.clone(),
            version: version.clone(),
            chunks,
            last_updated_at: chrono::DateTime::from(std::time::SystemTime::now())
                .to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
            summary: incoming.summary,
        };
        self.validate_head(&head)?;
        let result = self
            .api
            .upsert(
                &self.collection,
                &api::Upsert {
                    points: vec![api::Point {
                        id: point_id(&["canonical-head"]),
                        vector: api::Vectors::default(),
                        payload: head.clone(),
                    }],
                    update_filter: expected.map(|version| api::Filter {
                        must: vec![api::matches("version", vec![version.to_owned()])],
                        ..api::Filter::default()
                    }),
                    update_mode: Some(if expected.is_some() {
                        api::UpdateMode::UpdateOnly
                    } else {
                        api::UpdateMode::InsertOnly
                    }),
                },
            )
            .await;
        let observed = self.read_head().await?.ok_or_else(failure)?;
        if observed.version != version {
            result?;
            self.api
                .delete_versions(&self.collection, &[version])
                .await?;
            return Err(crate::shared::validation::conflict(
                "concurrent local canonical update; retry the operation",
            ));
        }
        // Old snapshots remain readable to holders of their immutable head.
        // Reclamation is explicit, separate from acknowledging canonical writes.
        Ok(observed)
    }

    async fn stage(&self, incoming: &Verified, version: &str) -> Result<usize, AppError> {
        let mut input = tokio::fs::File::from_std(incoming.file.reopen().map_err(AppError::io)?);
        let mut batch = api::StagingBatch::default();
        let mut bytes = 0_u64;
        let mut chunks = 0;
        let mut hash = blake3::Hasher::new();
        loop {
            let text = crate::shared::snapshot::encoding::read_text(&mut input, 256 * 1024).await?;
            if text.is_empty() {
                break;
            }
            bytes = bytes.checked_add(text.len() as u64).ok_or_else(failure)?;
            if bytes > incoming.summary.bytes {
                return Err(failure());
            }
            hash.update(text.as_bytes());
            batch
                .push(
                    &self.api,
                    &self.collection,
                    api::Point {
                        id: chunk_id(version, chunks),
                        vector: api::Vectors::default(),
                        payload: Chunk {
                            kind: Kind::CanonicalChunk,
                            version: version.to_owned(),
                            ordinal: chunks,
                            text,
                        },
                    },
                )
                .await?;
            chunks += 1;
        }
        batch.flush(&self.api, &self.collection).await?;
        if bytes != incoming.summary.bytes
            || hash.finalize().to_hex().as_str() != incoming.summary.digest
        {
            return Err(AppError::usage("canonical history changed during staging"));
        }
        Ok(chunks)
    }

    /// Read exactly the captured revision; newer publications cannot mix into it.
    pub(crate) async fn read(&self, head: &Head) -> Result<tempfile::NamedTempFile, AppError> {
        self.validate_head(head)?;
        let output =
            crate::blocking(|| tempfile::NamedTempFile::new().map_err(AppError::io)).await?;
        let mut writer = tokio::fs::File::from_std(output.reopen().map_err(AppError::io)?);
        let mut hash = blake3::Hasher::new();
        let mut bytes = 0_u64;
        for ordinal in 0..head.chunks {
            let id = chunk_id(&head.version, ordinal);
            let mut points = self
                .api
                .retrieve::<Chunk>(&self.collection, std::slice::from_ref(&id))
                .await?;
            if points.len() != 1 {
                return Err(failure());
            }
            let point = points.pop().ok_or_else(failure)?;
            let chunk = point.payload;
            if point.id != id
                || chunk.kind != Kind::CanonicalChunk
                || chunk.version != head.version
                || chunk.ordinal != ordinal
            {
                return Err(failure());
            }
            bytes = bytes
                .checked_add(chunk.text.len() as u64)
                .ok_or_else(failure)?;
            if bytes > head.summary.bytes {
                return Err(failure());
            }
            hash.update(chunk.text.as_bytes());
            writer
                .write_all(chunk.text.as_bytes())
                .await
                .map_err(AppError::io)?;
        }
        writer.flush().await.map_err(AppError::io)?;
        drop(writer);
        if bytes != head.summary.bytes || hash.finalize().to_hex().as_str() != head.summary.digest {
            return Err(AppError::shared("canonical snapshot checksum mismatch"));
        }
        Ok(output)
    }
}

fn chunk_id(version: &str, ordinal: usize) -> String {
    point_id(&["canonical-chunk", version, &ordinal.to_string()])
}
fn valid_version(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests;
