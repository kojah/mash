//! Reuse immutable thread snapshots; Qdrant owns physical storage and reclamation.
use super::{
    AppError, Chunk, Identity, Manifest, NodeHead, PointKind, Store, ThreadSnapshot, api,
    collection, failure, point_id,
};
use crate::shared::snapshot::{self, Data, Encoded, Plan};
use std::collections::BTreeMap;
mod receive;

pub(super) struct Prepared {
    pub(super) threads: BTreeMap<String, ThreadSnapshot>,
    changed: BTreeMap<String, Encoded>,
    metadata: Encoded,
}

#[tracing::instrument(name = "qdrant_prepare_threads", skip_all, fields(threads = plan.loaded.data.conversations.len(), messages = plan.loaded.data.messages.len()))]
pub(super) fn prepare(
    plan: Plan,
    previous: &NodeHead,
    version: &str,
    limit: usize,
    reuse: bool,
) -> Result<(Plan, Prepared, crate::shared::silver_revision::State), AppError> {
    if plan.revision < 1 || previous.version != plan.loaded.expected {
        return Err(crate::shared::validation::conflict(
            "publication changed while preparing threads",
        ));
    }
    let silver = plan.silver_revision()?;
    let data = &plan.loaded.data;
    let metadata = Data {
        sources: data.sources.clone(),
        tombstones: data.tombstones.clone(),
        ..Data::default()
    };
    let metadata = Encoded::new(&metadata.layers(), limit)?;
    let mut bytes = metadata.bytes;
    let mut grouped = BTreeMap::<&str, Vec<&crate::shared::protocol::MessageRecord>>::new();
    for message in data.messages.values() {
        grouped
            .entry(&message.conversation_id)
            .or_default()
            .push(message);
    }
    let mut threads = BTreeMap::new();
    let mut changed = BTreeMap::new();
    for (id, conversation) in &data.conversations {
        let thread = Data {
            conversations: BTreeMap::from([(id.clone(), conversation.clone())]),
            messages: grouped
                .remove(id.as_str())
                .unwrap_or_default()
                .into_iter()
                .map(|message| (message.id.clone(), message.clone()))
                .collect(),
            ..Data::default()
        };
        if let Some(retained) = previous
            .threads
            .as_ref()
            .and_then(|threads| threads.get(id))
            .filter(|_| reuse)
            && Encoded::matches(
                &thread.layers(),
                limit,
                retained.manifest.bytes,
                &retained.manifest.digest,
            )?
        {
            bytes = add_bytes(bytes, retained.manifest.bytes, limit)?;
            threads.insert(id.clone(), retained.clone());
            continue;
        }
        let encoded = Encoded::new(&thread.layers(), limit)?;
        bytes = add_bytes(bytes, encoded.bytes, limit)?;
        let thread_version = blake3::hash(format!("{version}\0{id}").as_bytes())
            .to_hex()
            .to_string();
        threads.insert(
            id.clone(),
            ThreadSnapshot {
                version: thread_version,
                manifest: Manifest {
                    bytes: encoded.bytes,
                    digest: encoded.digest.clone(),
                    chunks: 0,
                },
            },
        );
        changed.insert(id.clone(), encoded);
    }
    if !grouped.is_empty() {
        return Err(failure());
    }
    Ok((
        plan,
        Prepared {
            threads,
            changed,
            metadata,
        },
        silver,
    ))
}

fn add_bytes(bytes: usize, next: usize, limit: usize) -> Result<usize, AppError> {
    bytes
        .checked_add(next)
        .filter(|bytes| *bytes <= limit)
        .ok_or_else(|| AppError::usage("node exceeds the configured snapshot limit"))
}

impl Prepared {
    pub(super) fn staged_versions(&self, version: &str) -> Vec<String> {
        std::iter::once(version.to_owned())
            .chain(
                self.changed
                    .keys()
                    .map(|id| self.threads[id].version.clone()),
            )
            .collect()
    }
}

impl Store {
    pub(super) async fn pending_threads(
        &self,
        identity: &Identity,
        node: NodeHead,
    ) -> Result<snapshot::PendingSnapshot, AppError> {
        let manifest = node.manifest.as_ref().ok_or_else(failure)?;
        let threads = node.threads.as_ref().ok_or_else(failure)?;
        let mut bytes = manifest.bytes;
        for thread in threads.values() {
            bytes = bytes
                .checked_add(thread.manifest.bytes)
                .filter(|bytes| *bytes <= self.max_node_bytes)
                .ok_or_else(failure)?;
        }
        if bytes > self.max_node_bytes {
            return Err(failure());
        }
        let manifests: Vec<_> = std::iter::once((node.version.as_str(), manifest))
            .chain(
                threads
                    .values()
                    .map(|thread| (thread.version.as_str(), &thread.manifest)),
            )
            .collect();
        let mut received = self.receive_parts(identity, &manifests).await?.into_iter();
        let payload = received.next().ok_or_else(failure)?;
        let parts = threads.keys().cloned().zip(received).collect();
        self.unchanged(identity, &node.version).await?;
        Ok(snapshot::PendingSnapshot {
            payload: Some(payload),
            threads: Some(parts),
            head: node.head,
            expected: node.version,
        })
    }
    async fn write_part(
        &self,
        identity: &Identity,
        encoded: &Encoded,
        version: &str,
        batch: &mut api::StagingBatch,
    ) -> Result<Manifest, AppError> {
        let mut reader = encoded.reader()?;
        let mut chunks = 0;
        loop {
            let text = snapshot::encoding::read_text(&mut reader, 4 * 1024 * 1024).await?;
            if text.is_empty() {
                break;
            }
            batch
                .push(
                    &self.api,
                    &collection(&identity.group_id, true),
                    api::Point {
                        id: point_id(&["chunk", version, &chunks.to_string()]),
                        vector: api::Vectors::default(),
                        payload: Chunk {
                            kind: PointKind::Chunk,
                            version: version.into(),
                            text,
                        },
                    },
                )
                .await?;
            chunks += 1;
        }
        Ok(Manifest {
            bytes: encoded.bytes,
            digest: encoded.digest.clone(),
            chunks,
        })
    }
    #[tracing::instrument(name = "qdrant_stage_snapshot", skip_all)]
    pub(super) async fn stage_incremental(
        &self,
        identity: &Identity,
        data: &Data,
        prepared: &mut Prepared,
        version: &str,
    ) -> Result<Manifest, AppError> {
        let mut batch = api::StagingBatch::default();
        let metadata = self
            .write_part(identity, &prepared.metadata, version, &mut batch)
            .await?;
        let mut changed = BTreeMap::new();
        for (id, encoded) in &prepared.changed {
            let thread = prepared.threads.get_mut(id).ok_or_else(failure)?;
            thread.manifest = self
                .write_part(identity, encoded, &thread.version, &mut batch)
                .await?;
            changed.insert(id.clone(), thread.clone());
        }
        batch
            .flush(&self.api, &collection(&identity.group_id, true))
            .await?;
        self.stage_catalog(identity, data, version, Some(&changed))
            .await?;
        self.stage_documents(identity, data, version, Some(&changed))
            .await?;
        Ok(metadata)
    }
}
