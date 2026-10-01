//! Bounded checksummed payload reads shared by canonical data and thread catalogs.
use super::{AppError, CANONICAL_CHUNK_BYTES, CanonicalManifest, Store, decode, redis_error};
use crate::shared::snapshot::payload::{Received, Receiver};

impl Store {
    pub(super) async fn read_payload(&self, key: &str) -> Result<Received, AppError> {
        let raw: Option<String> = redis::cmd("GET")
            .arg(key)
            .query_async(&mut self.connection.clone())
            .await
            .map_err(redis_error)?;
        let raw = raw.ok_or_else(|| {
            AppError::shared("Redis snapshot payload is missing; use persistence and noeviction")
        })?;
        let manifest: CanonicalManifest = decode(&raw)?;
        if manifest.bytes > self.max_node_bytes
            || manifest.chunks == 0
            || manifest.chunks != manifest.bytes.div_ceil(CANONICAL_CHUNK_BYTES)
        {
            return Err(AppError::shared(
                "Redis snapshot manifest exceeds the configured limit or is invalid",
            ));
        }
        let mut receiver = Receiver::new(manifest.bytes).await?;
        for chunk in 0..manifest.chunks {
            let part: Option<Vec<u8>> = redis::cmd("GET")
                .arg(format!("{key}:chunk:{chunk}"))
                .query_async(&mut self.connection.clone())
                .await
                .map_err(redis_error)?;
            let part = part.ok_or_else(|| AppError::shared("Redis snapshot chunk is missing"))?;
            if part.len() > CANONICAL_CHUNK_BYTES {
                return Err(AppError::shared("invalid Redis snapshot chunk length"));
            }
            receiver.append(&part).await?;
        }
        receiver.finish(&manifest.digest).await
    }
}
