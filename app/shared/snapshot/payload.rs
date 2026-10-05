//! Receive checksummed canonical JSON through bounded chunks, then decode on a worker.
use crate::AppError;
use serde::de::DeserializeOwned;
use tokio::io::AsyncWriteExt;

pub(in crate::shared) struct Receiver {
    storage: Storage,
    hasher: blake3::Hasher,
    bytes: usize,
    limit: usize,
}

enum Storage {
    Memory(Vec<u8>),
    File {
        file: tempfile::NamedTempFile,
        output: tokio::fs::File,
    },
}

enum VerifiedStorage {
    Memory(Vec<u8>),
    File(tempfile::TempPath),
}

pub(in crate::shared) struct Received {
    storage: VerifiedStorage,
    bytes: usize,
}

impl Receiver {
    pub(in crate::shared) async fn new(limit: usize) -> Result<Self, AppError> {
        let file = crate::blocking(|| {
            tempfile::NamedTempFile::new()
                .map_err(|_| AppError::internal("create received snapshot spool"))
        })
        .await?;
        let output = file
            .reopen()
            .map(tokio::fs::File::from_std)
            .map_err(|_| AppError::internal("open received snapshot spool"))?;
        Ok(Self {
            storage: Storage::File { file, output },
            hasher: blake3::Hasher::new(),
            bytes: 0,
            limit,
        })
    }

    /// The caller admits this allocation against its aggregate receive budget.
    pub(in crate::shared) fn in_memory(limit: usize) -> Result<Self, AppError> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(limit)
            .map_err(|_| AppError::internal("allocate received snapshot buffer"))?;
        Ok(Self {
            storage: Storage::Memory(bytes),
            hasher: blake3::Hasher::new(),
            bytes: 0,
            limit,
        })
    }

    pub(in crate::shared) async fn append(&mut self, part: &[u8]) -> Result<(), AppError> {
        if part.len() > self.limit.saturating_sub(self.bytes) {
            return Err(AppError::shared("invalid snapshot chunk length"));
        }
        match &mut self.storage {
            Storage::Memory(bytes) => bytes.extend_from_slice(part),
            Storage::File { output, .. } => output
                .write_all(part)
                .await
                .map_err(|_| AppError::internal("write received snapshot spool"))?,
        }
        self.hasher.update(part);
        self.bytes += part.len();
        Ok(())
    }

    pub(in crate::shared) async fn finish(self, digest: &str) -> Result<Received, AppError> {
        if self.bytes != self.limit || self.hasher.finalize().to_hex().as_str() != digest {
            return Err(AppError::shared("snapshot checksum mismatch"));
        }
        let storage = match self.storage {
            Storage::Memory(bytes) => VerifiedStorage::Memory(bytes),
            Storage::File { file, mut output } => {
                output
                    .flush()
                    .await
                    .map_err(|_| AppError::internal("flush received snapshot spool"))?;
                drop(output);
                VerifiedStorage::File(file.into_temp_path())
            }
        };
        Ok(Received {
            storage,
            bytes: self.bytes,
        })
    }
}

impl Received {
    pub(in crate::shared) fn encoded_bytes(&self) -> usize {
        self.bytes
    }
    /// A complete, checksum-verified payload; call on a blocking worker.
    #[tracing::instrument(name = "decode_shared_snapshot", skip_all, fields(bytes = self.bytes))]
    pub(in crate::shared) fn decode<T: DeserializeOwned>(self) -> Result<T, AppError> {
        let result = match self.storage {
            VerifiedStorage::Memory(bytes) => serde_json::from_slice(&bytes),
            VerifiedStorage::File(path) => {
                let file = std::fs::File::open(&path)
                    .map_err(|_| AppError::internal("open received snapshot spool"))?;
                serde_json::from_reader(std::io::BufReader::with_capacity(64 * 1024, file))
            }
        };
        result.map_err(|_| AppError::shared("invalid canonical snapshot JSON"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn verified_chunks_decode_unicode_and_reject_truncation_corruption_and_overflow() {
        for memory in [false, true] {
            verify_chunks(memory).await;
        }
    }

    async fn receiver(limit: usize, memory: bool) -> Receiver {
        if memory {
            Receiver::in_memory(limit).unwrap()
        } else {
            Receiver::new(limit).await.unwrap()
        }
    }

    async fn verify_chunks(memory: bool) {
        let value = vec!["é🦀漢字\n\"\\".repeat(33), "plain".into()];
        let bytes = serde_json::to_vec(&value).unwrap();
        let digest = blake3::hash(&bytes).to_hex().to_string();
        let mut input = receiver(bytes.len(), memory).await;
        for part in bytes.chunks(3) {
            input.append(part).await.unwrap();
        }
        let verified = input.finish(&digest).await.unwrap();
        assert_eq!(verified.decode::<Vec<String>>().unwrap(), value);

        let mut input = receiver(bytes.len(), memory).await;
        input.append(&bytes[..bytes.len() - 1]).await.unwrap();
        assert!(input.finish(&digest).await.is_err());
        let mut input = receiver(bytes.len(), memory).await;
        let mut corrupted = bytes.clone();
        corrupted[0] ^= 1;
        input.append(&corrupted).await.unwrap();
        assert!(input.finish(&digest).await.is_err());
        let mut input = receiver(bytes.len() - 1, memory).await;
        assert!(input.append(&bytes).await.is_err());

        let invalid = b"[\"private-canary\xff\"]";
        let mut input = receiver(invalid.len(), memory).await;
        input.append(invalid).await.unwrap();
        let verified = input
            .finish(blake3::hash(invalid).to_hex().as_str())
            .await
            .unwrap();
        let error = verified.decode::<Vec<String>>().unwrap_err();
        assert_eq!(error.error.kind, crate::ErrorKind::SharedIndex);
        assert!(!error.error.message.contains("canary"));
    }
}
