//! Bounded JSON serialization without a second full canonical snapshot in RAM.
use crate::AppError;
use serde::Serialize;
use std::io::{BufWriter, Write};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

pub(in crate::shared) struct Encoded {
    file: tempfile::TempPath,
    pub(in crate::shared) bytes: usize,
    pub(in crate::shared) digest: String,
}

struct Writer<'a> {
    output: BufWriter<&'a mut std::fs::File>,
    hasher: blake3::Hasher,
    bytes: usize,
    limit: usize,
    exceeded: bool,
}
impl Write for Writer<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes) {
            self.exceeded = true;
            return Err(std::io::Error::other("snapshot limit"));
        }
        self.output.write_all(bytes)?;
        self.hasher.update(bytes);
        self.bytes += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.output.flush()
    }
}

impl Encoded {
    /// Call on a blocking worker; temporary files are private and deleted on drop.
    #[tracing::instrument(name = "serialize_shared_snapshot", skip_all, fields(bytes = tracing::field::Empty))]
    pub(in crate::shared) fn new<T: Serialize>(value: &T, limit: usize) -> Result<Self, AppError> {
        let mut file = tempfile::NamedTempFile::new()
            .map_err(|_| AppError::internal("create canonical snapshot spool"))?;
        let mut writer = Writer {
            output: BufWriter::with_capacity(64 * 1024, file.as_file_mut()),
            hasher: blake3::Hasher::new(),
            bytes: 0,
            limit,
            exceeded: false,
        };
        let result = serde_json::to_writer(&mut writer, value);
        if writer.exceeded {
            return Err(AppError::usage(
                "node exceeds the configured snapshot limit",
            ));
        }
        result.map_err(|_| AppError::internal("serialize canonical snapshot spool"))?;
        writer
            .flush()
            .map_err(|_| AppError::internal("flush canonical snapshot spool"))?;
        let bytes = writer.bytes;
        let digest = writer.hasher.finalize().to_hex().to_string();
        tracing::Span::current().record("bytes", bytes);
        drop(writer);
        Ok(Self {
            file: file.into_temp_path(),
            bytes,
            digest,
        })
    }

    pub(in crate::shared) fn reader(&self) -> Result<tokio::fs::File, AppError> {
        std::fs::File::open(&self.file)
            .map(tokio::fs::File::from_std)
            .map_err(|_| AppError::internal("open canonical snapshot spool"))
    }
}

pub(in crate::shared) async fn read_bytes(
    file: &mut tokio::fs::File,
    limit: usize,
) -> Result<Vec<u8>, AppError> {
    let mut bytes = Vec::with_capacity(limit);
    file.take(limit as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| AppError::internal("read canonical snapshot spool"))?;
    Ok(bytes)
}

/// Qdrant stores chunks as JSON strings, so every chunk must end on a UTF-8 boundary.
pub(in crate::shared) async fn read_text(
    file: &mut tokio::fs::File,
    limit: usize,
) -> Result<String, AppError> {
    if limit < 4 {
        return Err(AppError::internal("invalid canonical text chunk limit"));
    }
    let mut bytes = read_bytes(file, limit).await?;
    if let Err(error) = std::str::from_utf8(&bytes) {
        if error.error_len().is_some() {
            return Err(AppError::internal("invalid canonical snapshot UTF-8"));
        }
        let trailing = bytes.len() - error.valid_up_to();
        file.seek(std::io::SeekFrom::Current(
            -i64::try_from(trailing).map_err(|_| AppError::internal("invalid UTF-8 boundary"))?,
        ))
        .await
        .map_err(|_| AppError::internal("seek canonical snapshot spool"))?;
        bytes.truncate(error.valid_up_to());
    }
    String::from_utf8(bytes).map_err(|_| AppError::internal("invalid canonical snapshot UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn chunks_preserve_exact_json_digest_and_unicode_at_every_boundary() {
        let value = vec!["é🦀漢字\n\"\\".repeat(33), "plain".into()];
        let expected = serde_json::to_vec(&value).unwrap();
        let encoded = Encoded::new(&value, expected.len()).unwrap();
        assert_eq!(encoded.bytes, expected.len());
        assert_eq!(encoded.digest, blake3::hash(&expected).to_hex().as_str());
        for limit in 4..20 {
            let mut reader = encoded.reader().unwrap();
            let mut combined = String::new();
            loop {
                let chunk = read_text(&mut reader, limit).await.unwrap();
                if chunk.is_empty() {
                    break;
                }
                assert!(chunk.len() <= limit);
                combined.push_str(&chunk);
            }
            assert_eq!(combined.as_bytes(), expected);
            let mut reader = encoded.reader().unwrap();
            let mut combined = Vec::new();
            loop {
                let chunk = read_bytes(&mut reader, limit).await.unwrap();
                if chunk.is_empty() {
                    break;
                }
                assert!(chunk.len() <= limit);
                combined.extend(chunk);
            }
            assert_eq!(combined, expected);
        }
        let error = Encoded::new(&value, expected.len() - 1).err().unwrap();
        assert_eq!(error.error.kind, crate::ErrorKind::Usage);
        assert!(!error.error.message.contains('🦀'));
    }
}
