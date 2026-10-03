//! Receive checksummed canonical JSON through bounded chunks, then decode on a worker.
use crate::AppError;
use serde::de::DeserializeOwned;
use tokio::io::AsyncWriteExt;

pub(in crate::shared) struct Receiver {
    file: tempfile::NamedTempFile,
    output: tokio::fs::File,
    hasher: blake3::Hasher,
    bytes: usize,
    limit: usize,
}

pub(in crate::shared) struct Received {
    file: tempfile::NamedTempFile,
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
            file,
            output,
            hasher: blake3::Hasher::new(),
            bytes: 0,
            limit,
        })
    }

    pub(in crate::shared) async fn append(&mut self, part: &[u8]) -> Result<(), AppError> {
        if part.len() > self.limit.saturating_sub(self.bytes) {
            return Err(AppError::shared("invalid snapshot chunk length"));
        }
        self.output
            .write_all(part)
            .await
            .map_err(|_| AppError::internal("write received snapshot spool"))?;
        self.hasher.update(part);
        self.bytes += part.len();
        Ok(())
    }

    pub(in crate::shared) async fn finish(mut self, digest: &str) -> Result<Received, AppError> {
        if self.bytes != self.limit || self.hasher.finalize().to_hex().as_str() != digest {
            return Err(AppError::shared("snapshot checksum mismatch"));
        }
        self.output
            .flush()
            .await
            .map_err(|_| AppError::internal("flush received snapshot spool"))?;
        drop(self.output);
        Ok(Received {
            file: self.file,
            bytes: self.bytes,
        })
    }
}

impl Received {
    pub(in crate::shared) fn encoded_bytes(&self) -> usize {
        self.bytes
    }
    /// A complete, checksum-verified file; call on a blocking worker.
    #[tracing::instrument(name = "decode_shared_snapshot", skip_all, fields(bytes = self.bytes))]
    pub(in crate::shared) fn decode<T: DeserializeOwned>(self) -> Result<T, AppError> {
        let file = self
            .file
            .reopen()
            .map_err(|_| AppError::internal("open received snapshot spool"))?;
        serde_json::from_reader(std::io::BufReader::with_capacity(64 * 1024, file))
            .map_err(|_| AppError::shared("invalid canonical snapshot JSON"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn verified_chunks_decode_unicode_and_reject_truncation_corruption_and_overflow() {
        let value = vec!["é🦀漢字\n\"\\".repeat(33), "plain".into()];
        let bytes = serde_json::to_vec(&value).unwrap();
        let digest = blake3::hash(&bytes).to_hex().to_string();
        let mut receiver = Receiver::new(bytes.len()).await.unwrap();
        for part in bytes.chunks(3) {
            receiver.append(part).await.unwrap();
        }
        let verified = receiver.finish(&digest).await.unwrap();
        assert_eq!(verified.decode::<Vec<String>>().unwrap(), value);

        let mut receiver = Receiver::new(bytes.len()).await.unwrap();
        receiver.append(&bytes[..bytes.len() - 1]).await.unwrap();
        assert!(receiver.finish(&digest).await.is_err());
        let mut receiver = Receiver::new(bytes.len()).await.unwrap();
        let mut corrupted = bytes.clone();
        corrupted[0] ^= 1;
        receiver.append(&corrupted).await.unwrap();
        assert!(receiver.finish(&digest).await.is_err());
        let mut receiver = Receiver::new(bytes.len() - 1).await.unwrap();
        assert!(receiver.append(&bytes).await.is_err());

        let invalid = b"[\"private-canary\xff\"]";
        let mut receiver = Receiver::new(invalid.len()).await.unwrap();
        receiver.append(invalid).await.unwrap();
        let verified = receiver
            .finish(blake3::hash(invalid).to_hex().as_str())
            .await
            .unwrap();
        let error = verified.decode::<Vec<String>>().unwrap_err();
        assert_eq!(error.error.kind, crate::ErrorKind::SharedIndex);
        assert!(!error.error.message.contains("canary"));
    }
}
