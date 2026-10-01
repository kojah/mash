//! Stage immutable data, then CAS the active node pointer. No partially visible uploads.
use super::CatalogRecord;
use super::{
    AppError, Chunk, Document, Head, Identity, Manifest, NodeHead, PointKind, Store, api,
    collection, failure, point_id, sparse, tokens,
};
use crate::shared::{
    backend::{conflict, valid_id},
    protocol::{Forget, Forgotten, Indexed},
    snapshot::{self, Data, Plan},
};

impl Store {
    pub(in crate::shared) async fn index(
        &self,
        identity: &Identity,
        file: tempfile::NamedTempFile,
    ) -> Result<Indexed, AppError> {
        if file
            .as_file()
            .metadata()
            .map_err(|_| AppError::internal("inspect index upload"))?
            .len()
            > u64::try_from(self.max_node_bytes).unwrap_or(u64::MAX)
        {
            return Err(AppError::usage(
                "Qdrant upload exceeds the configured snapshot limit",
            ));
        }
        let loaded = self.load(identity).await?;
        let dimensions = identity.dimensions;
        let plan = crate::blocking(move || snapshot::plan(&file, loaded, dimensions)).await?;
        let instance = plan
            .loaded
            .head
            .as_ref()
            .ok_or_else(failure)?
            .instance
            .clone();
        self.commit(identity, plan, instance, true).await
    }
    pub(in crate::shared) async fn forget(
        &self,
        identity: &Identity,
        input: Forget,
    ) -> Result<Forgotten, AppError> {
        valid_id(&input.instance)?;
        if input.ids.len() > 10000 {
            return Err(AppError::usage("too many forget identities"));
        }
        let loaded = self.load(identity).await?;
        let instance = input.instance.clone();
        let (plan, forgotten) = crate::blocking(move || {
            let accepted = loaded.head.as_ref().map_or(0, |h| h.revision);
            if loaded
                .head
                .as_ref()
                .is_some_and(|h| h.instance != input.instance)
                || input.revision.is_some_and(|r| r <= accepted)
            {
                return Err(conflict("node identity or revision conflict"));
            }
            let revision = input.revision.unwrap_or(
                accepted
                    .checked_add(1)
                    .ok_or_else(|| conflict("node revision exhausted"))?,
            );
            let mut plan = Plan {
                loaded,
                revision,
                changed: false,
                removed: 0,
                purged: 0,
            };
            let mut forgotten = false;
            for id in input.ids {
                forgotten |= snapshot::forget(&mut plan.loaded.data, &id)?;
            }
            plan.changed = forgotten;
            Ok((plan, forgotten))
        })
        .await?;
        self.commit(identity, plan, instance, false).await?;
        Ok(Forgotten { forgotten })
    }
    pub(super) async fn stage(
        &self,
        identity: &Identity,
        data: &Data,
        version: &str,
    ) -> Result<Manifest, AppError> {
        let manifest = self.stage_snapshot(identity, data, version).await?;
        self.stage_catalog(identity, data, version).await?;
        self.stage_documents(identity, data, version).await?;
        Ok(manifest)
    }
    async fn stage_snapshot(
        &self,
        identity: &Identity,
        data: &Data,
        version: &str,
    ) -> Result<Manifest, AppError> {
        let json = serde_json::to_string(data)
            .map_err(|_| AppError::internal("serialize Qdrant snapshot"))?;
        if json.len() > self.max_node_bytes {
            return Err(AppError::usage(
                "Qdrant node exceeds the configured snapshot limit",
            ));
        }
        let digest = blake3::hash(json.as_bytes()).to_hex().to_string();
        // Keep chunks valid UTF-8 and below Qdrant's default request-size bound.
        let mut begin = 0;
        let mut chunks = 0;
        while begin < json.len() {
            let mut end = begin.saturating_add(4 * 1024 * 1024).min(json.len());
            while !json.is_char_boundary(end) {
                end -= 1;
            }
            let point = api::Point {
                id: point_id(&["chunk", version, &chunks.to_string()]),
                vector: api::Vectors::default(),
                payload: Chunk {
                    kind: PointKind::Chunk,
                    version: version.into(),
                    text: json[begin..end].into(),
                },
            };
            self.api
                .upsert(
                    &collection(&identity.group_id, true),
                    &api::Upsert {
                        points: vec![point],
                        update_filter: None,
                        update_mode: None,
                    },
                )
                .await?;
            begin = end;
            chunks += 1;
        }
        Ok(Manifest {
            bytes: json.len(),
            chunks,
            digest,
        })
    }
    async fn stage_catalog(
        &self,
        identity: &Identity,
        data: &Data,
        version: &str,
    ) -> Result<(), AppError> {
        let mut catalog = Vec::new();
        for conversation in data.conversations.values() {
            catalog.push(api::Point {
                id: point_id(&["catalog", version, &conversation.id]),
                vector: api::Vectors::default(),
                payload: CatalogRecord {
                    kind: PointKind::Catalog,
                    version: version.into(),
                    node_id: identity.node_id.clone(),
                    conversation: conversation.clone(),
                },
            });
            if catalog.len() == 32 {
                self.api
                    .upsert(
                        &collection(&identity.group_id, true),
                        &api::Upsert {
                            points: std::mem::take(&mut catalog),
                            update_filter: None,
                            update_mode: None,
                        },
                    )
                    .await?;
            }
        }
        if !catalog.is_empty() {
            self.api
                .upsert(
                    &collection(&identity.group_id, true),
                    &api::Upsert {
                        points: catalog,
                        update_filter: None,
                        update_mode: None,
                    },
                )
                .await?;
        }
        Ok(())
    }
    async fn stage_documents(
        &self,
        identity: &Identity,
        data: &Data,
        version: &str,
    ) -> Result<(), AppError> {
        let mut points = Vec::new();
        for message in data.messages.values() {
            let conversation = data
                .conversations
                .get(&message.conversation_id)
                .ok_or_else(failure)?;
            points.push(api::Point {
                id: point_id(&["message", version, &message.id]),
                vector: api::Vectors {
                    dense: message.vector.clone(),
                    lexical: (!tokens(&message.projection).is_empty())
                        .then(|| sparse(&message.projection, false)),
                },
                payload: Document {
                    version: version.into(),
                    node_id: identity.node_id.clone(),
                    provider: conversation.provider,
                    role: message.role.clone(),
                    conversation_id: conversation.id.clone(),
                    project: conversation.project.clone(),
                    repository: conversation.repository.clone(),
                    created_at: message.created_at,
                    tokens: tokens(&message.projection),
                    conversation: conversation.clone(),
                    message: message.clone(),
                },
            });
            if points.len() == 32 {
                self.api
                    .upsert(
                        &collection(&identity.group_id, false),
                        &api::Upsert {
                            points: std::mem::take(&mut points),
                            update_filter: None,
                            update_mode: None,
                        },
                    )
                    .await?;
            }
        }
        if !points.is_empty() {
            self.api
                .upsert(
                    &collection(&identity.group_id, false),
                    &api::Upsert {
                        points,
                        update_filter: None,
                        update_mode: None,
                    },
                )
                .await?;
        }
        Ok(())
    }
    pub(super) async fn commit(
        &self,
        identity: &Identity,
        plan: Plan,
        instance: String,
        checked: bool,
    ) -> Result<Indexed, AppError> {
        if plan.revision < 1 {
            return Err(AppError::usage("invalid Qdrant revision"));
        }
        let counts = plan.loaded.data.counts(identity);
        let mut random = [0u8; 32];
        getrandom::fill(&mut random)
            .map_err(|_| AppError::internal("create Qdrant staging identity"))?;
        let version = blake3::hash(&random).to_hex().to_string();
        let previous = plan.loaded.head.as_ref();
        let now = chrono::DateTime::from(std::time::SystemTime::now())
            .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
        let head = Head {
            instance,
            generation: identity.generation.clone(),
            revision: plan.revision,
            version: version.clone(),
            counts: counts.clone(),
            last_synced_at: if checked {
                Some(now.clone())
            } else {
                previous.and_then(|h| h.last_synced_at.clone())
            },
            last_updated_at: if plan.changed {
                Some(now)
            } else {
                previous.and_then(|h| h.last_updated_at.clone())
            },
            metadata_ready: true,
            catalog_ready: true,
        };
        let manifest = match self.stage(identity, &plan.loaded.data, &version).await {
            Ok(manifest) => manifest,
            Err(error) => {
                self.cleanup(identity, &version).await;
                return Err(error);
            }
        };
        let node = NodeHead {
            kind: PointKind::Head,
            node_id: identity.node_id.clone(),
            version: version.clone(),
            head: Some(head),
            manifest: Some(manifest),
        };
        let point = api::Point {
            id: point_id(&["head", &identity.node_id]),
            vector: api::Vectors::default(),
            payload: node,
        };
        let expected = &plan.loaded.expected;
        let input = api::Upsert {
            points: vec![point],
            update_filter: (!expected.is_empty()).then(|| api::Filter {
                must: vec![api::matches("version", vec![expected.clone()])],
                ..api::Filter::default()
            }),
            update_mode: Some(if expected.is_empty() {
                api::UpdateMode::InsertOnly
            } else {
                api::UpdateMode::UpdateOnly
            }),
        };
        // A timeout may mean the CAS committed. Verify its unique version before
        // returning or reclaiming anything; if verification fails leave staging intact.
        let result = self
            .api
            .upsert(&collection(&identity.group_id, true), &input)
            .await;
        let current = self.node(identity).await?;
        if current.version != version {
            self.cleanup(identity, &version).await;
            result?;
            return Err(conflict("concurrent Qdrant update; retry the operation"));
        }
        if !expected.is_empty() {
            self.cleanup(identity, expected).await;
        }
        Ok(Indexed {
            counts,
            revision: plan.revision,
            removed_messages: plan.removed,
            purged_conversations: plan.purged,
        })
    }
    async fn cleanup(&self, identity: &Identity, version: &str) {
        for metadata in [false, true] {
            if self
                .api
                .delete_version(&collection(&identity.group_id, metadata), version)
                .await
                .is_err()
            {
                tracing::warn!("Qdrant obsolete staging cleanup failed; hidden version retained");
            }
        }
    }
}
