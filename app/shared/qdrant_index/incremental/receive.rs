//! Ordered snapshot assembly with a fixed number of bounded native reads.
use super::{
    AppError, Chunk, Identity, Manifest, PointKind, Store, api, collection, failure, point_id,
    snapshot,
};
use futures_util::{StreamExt, stream};
use std::collections::{BTreeMap, VecDeque};

const CHUNK_BYTES: usize = 4 * 1024 * 1024;
const BATCH_BYTES: usize = 8 * 1024 * 1024;
const IN_FLIGHT: usize = 4;

struct Batch {
    limits: BTreeMap<String, usize>,
    bytes: usize,
}

fn batches(parts: &[(&str, &Manifest)]) -> Result<Vec<Batch>, AppError> {
    let mut pending = VecDeque::new();
    for (version, manifest) in parts {
        // UTF-8 boundaries can shorten a generated chunk by at most three bytes.
        if manifest.bytes == 0
            || manifest.chunks == 0
            || manifest.chunks > manifest.bytes / (CHUNK_BYTES - 3) + 1
        {
            return Err(failure());
        }
        for chunk in 0..manifest.chunks {
            pending.push_back((
                point_id(&["chunk", version, &chunk.to_string()]),
                manifest.bytes.min(CHUNK_BYTES),
            ));
        }
    }
    let mut batches = Vec::new();
    while !pending.is_empty() {
        let mut batch = Batch {
            limits: BTreeMap::new(),
            bytes: 0,
        };
        while let Some((_, bytes)) = pending.front() {
            if batch.limits.len() >= 100 || batch.bytes + bytes > BATCH_BYTES {
                break;
            }
            let (id, bytes) = pending.pop_front().ok_or_else(failure)?;
            batch.bytes += bytes;
            if batch.limits.insert(id, bytes).is_some() {
                return Err(failure());
            }
        }
        batches.push(batch);
    }
    Ok(batches)
}

impl Batch {
    async fn read(
        mut self,
        client: &api::Client,
        name: &str,
    ) -> Result<BTreeMap<String, Chunk>, AppError> {
        // A JSON control byte expands to at most six bytes. Fixed-size chunk
        // identity fields plus the envelope fit within 64 KiB for 100 records.
        // Apply this before decoding; a broken backend cannot return a node-sized
        // body for every one of the parallel requests.
        let limit = self
            .bytes
            .checked_mul(6)
            .and_then(|bytes| bytes.checked_add(64 * 1024))
            .ok_or_else(failure)?;
        let ids: Vec<_> = self.limits.keys().cloned().collect();
        let points = client.retrieve_bounded::<Chunk>(name, &ids, limit).await?;
        if points.len() != ids.len() {
            return Err(failure());
        }
        let mut cache = BTreeMap::new();
        for point in points {
            let limit = self.limits.remove(&point.id).ok_or_else(failure)?;
            if point.payload.text.len() > limit {
                return Err(failure());
            }
            cache.insert(point.id, point.payload);
        }
        Ok(cache)
    }
}

impl Store {
    #[tracing::instrument(name = "qdrant_receive_thread_parts", skip_all, fields(parts = parts.len()))]
    pub(super) async fn receive_parts(
        &self,
        identity: &Identity,
        parts: &[(&str, &Manifest)],
    ) -> Result<Vec<snapshot::payload::Received>, AppError> {
        let name = collection(&identity.group_id, true);
        let reads = stream::iter(
            batches(parts)?
                .into_iter()
                .map(|batch| batch.read(&self.api, &name)),
        )
        .buffered(IN_FLIGHT);
        tokio::pin!(reads);
        let mut cache = BTreeMap::new();
        let mut completed = Vec::new();
        // This budget is unchanged. Prefetch adds at most four request-sized
        // responses; no tasks are detached, and dropping assembly cancels them.
        let mut memory_remaining = 64 * 1024 * 1024;
        for (version, manifest) in parts {
            let mut receiver =
                if manifest.bytes <= 1024 * 1024 && manifest.bytes <= memory_remaining {
                    memory_remaining -= manifest.bytes;
                    snapshot::payload::Receiver::in_memory(manifest.bytes)?
                } else {
                    snapshot::payload::Receiver::new(manifest.bytes).await?
                };
            for chunk in 0..manifest.chunks {
                if cache.is_empty() {
                    cache = reads.next().await.ok_or_else(failure)??;
                }
                let point = cache
                    .remove(&point_id(&["chunk", version, &chunk.to_string()]))
                    .ok_or_else(failure)?;
                if point.kind != PointKind::Chunk || point.version != *version {
                    return Err(failure());
                }
                receiver.append(point.text.as_bytes()).await?;
            }
            completed.push(receiver.finish(&manifest.digest).await?);
        }
        Ok(completed)
    }
}

#[cfg(test)]
mod tests;
