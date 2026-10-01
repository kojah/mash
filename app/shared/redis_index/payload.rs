//! Bounded checksummed payload reads shared by canonical data and thread catalogs.
use super::{AppError, CANONICAL_CHUNK_BYTES, CanonicalManifest, Store, decode, redis_error};

impl Store {
    pub(super) async fn read_payload(&self, key: &str) -> Result<(Vec<u8>, String), AppError> {
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
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(manifest.bytes)
            .map_err(|_| AppError::shared("Redis snapshot cannot be buffered"))?;
        for chunk in 0..manifest.chunks {
            let part: Option<Vec<u8>> = redis::cmd("GET")
                .arg(format!("{key}:chunk:{chunk}"))
                .query_async(&mut self.connection.clone())
                .await
                .map_err(redis_error)?;
            let part = part.ok_or_else(|| AppError::shared("Redis snapshot chunk is missing"))?;
            if part.len() > CANONICAL_CHUNK_BYTES
                || bytes.len().saturating_add(part.len()) > manifest.bytes
            {
                return Err(AppError::shared("invalid Redis snapshot chunk length"));
            }
            bytes.extend(part);
        }
        if bytes.len() != manifest.bytes {
            return Err(AppError::shared("Redis snapshot checksum mismatch"));
        }
        crate::blocking(move || {
            if blake3::hash(&bytes).to_hex().as_str() != manifest.digest {
                return Err(AppError::shared("Redis snapshot checksum mismatch"));
            }
            Ok((bytes, manifest.digest))
        })
        .await
    }
}
