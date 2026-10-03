//! Stage immutable data, then CAS the active node pointer. No partially visible uploads.
use super::CatalogRecord;
use super::{
    AppError, Chunk, Document, Head, Identity, Manifest, NodeHead, PointKind, Store, api,
    collection, failure, point_id, sparse, tokens,
};
use crate::shared::snapshot::Encoded;
use crate::shared::{
    protocol::{Forget, Forgotten, Indexed},
    snapshot::{self, Data, Plan},
    validation::{conflict, valid_id},
};
use tracing::Instrument;

#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq, serde::Deserialize))]
pub(super) enum PublicationOutcome {
    Observed,
    Unresolved,
    Conflict,
    StagingFailed,
    VerificationFailed,
}

fn outcome(value: PublicationOutcome) {
    tracing::Span::current().record("outcome", tracing::field::debug(value));
}

impl Store {
    #[tracing::instrument(name = "backend_index", skip_all, fields(backend = "qdrant"))]
    pub(in crate::shared) async fn index(
        &self,
        identity: &Identity,
        file: tempfile::NamedTempFile,
    ) -> Result<Indexed, AppError> {
        let incoming = file
            .as_file()
            .metadata()
            .map_err(|_| AppError::internal("inspect index upload"))?
            .len();
        if incoming > u64::try_from(self.max_node_bytes).unwrap_or(u64::MAX) {
            return Err(AppError::usage(
                "Qdrant upload exceeds the configured snapshot limit",
            ));
        }
        let pending = self.pending_snapshot(identity).await?;
        let reservation = self
            .snapshot_budget
            .reserve(pending.encoded_bytes(), incoming)?;
        let dimensions = identity.dimensions;
        let (plan, _reservation) = crate::blocking(move || {
            snapshot::plan(&file, pending.decode()?, dimensions).map(|plan| (plan, reservation))
        })
        .await?;
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
        let pending = self.pending_snapshot(identity).await?;
        let reservation = self.snapshot_budget.reserve(pending.encoded_bytes(), 0)?;
        let instance = input.instance.clone();
        let (plan, forgotten, _reservation) = crate::blocking(move || {
            let loaded = pending.decode()?;
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
            if let Some(head) = &mut plan.loaded.head {
                head.source_silver = None;
                head.silver_scans.clear();
            }
            Ok((plan, forgotten, reservation))
        })
        .await?;
        self.commit(identity, plan, instance, false).await?;
        Ok(Forgotten { forgotten })
    }
    pub(super) async fn stage(
        &self,
        identity: &Identity,
        data: &Data,
        encoded: &Encoded,
        version: &str,
    ) -> Result<Manifest, AppError> {
        let manifest = self.stage_snapshot(identity, encoded, version).await?;
        self.stage_catalog(identity, data, version).await?;
        self.stage_documents(identity, data, version).await?;
        Ok(manifest)
    }
    #[tracing::instrument(name = "qdrant_stage_snapshot", skip_all)]
    async fn stage_snapshot(
        &self,
        identity: &Identity,
        encoded: &Encoded,
        version: &str,
    ) -> Result<Manifest, AppError> {
        // Keep chunks valid UTF-8 and below Qdrant's default request-size bound.
        let mut reader = encoded.reader()?;
        let mut chunks = 0;
        loop {
            let text = snapshot::encoding::read_text(&mut reader, 4 * 1024 * 1024).await?;
            if text.is_empty() {
                break;
            }
            let point = api::Point {
                id: point_id(&["chunk", version, &chunks.to_string()]),
                vector: api::Vectors::default(),
                payload: Chunk {
                    kind: PointKind::Chunk,
                    version: version.into(),
                    text,
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
            chunks += 1;
        }
        Ok(Manifest {
            bytes: encoded.bytes,
            chunks,
            digest: encoded.digest.clone(),
        })
    }
    #[tracing::instrument(name = "qdrant_stage_catalog", skip_all)]
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
    #[tracing::instrument(name = "qdrant_stage_documents", skip_all)]
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
    #[tracing::instrument(name = "qdrant_publish", skip_all, fields(revision = plan.revision, outcome = tracing::field::Empty))]
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
        let max_bytes = self.max_node_bytes;
        let (plan, encoded) = crate::blocking(move || {
            let encoded = Encoded::new(&plan.loaded.data.layers(), max_bytes)?;
            Ok((plan, encoded))
        })
        .await?;
        let counts = plan.loaded.data.counts(identity);
        let mut random = [0u8; 32];
        getrandom::fill(&mut random)
            .map_err(|_| AppError::internal("create Qdrant staging identity"))?;
        let version = blake3::hash(&random).to_hex().to_string();
        let previous = plan.loaded.head.as_ref();
        let now = chrono::DateTime::from(std::time::SystemTime::now())
            .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
        let head = Head {
            silver_scans: previous.map_or_else(Vec::new, |h| h.silver_scans.clone()),
            source_silver: previous.and_then(|h| h.source_silver.clone()),
            snapshot_schema: crate::shared::snapshot::layers::Schema::Layered,
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
        let manifest = match self
            .stage(identity, &plan.loaded.data, &encoded, &version)
            .await
        {
            Ok(manifest) => manifest,
            Err(error) => {
                outcome(PublicationOutcome::StagingFailed);
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
        let expected = &plan.loaded.expected;
        // A failed request may still commit after this reread. Seeing another head
        // only permits reclamation when the conditional write completed successfully.
        // If verification fails, leave staging intact as well.
        let result = self.publish_head(identity, node, expected).await;
        let current = match self
            .node(identity)
            .instrument(tracing::info_span!("qdrant_verify_head"))
            .await
        {
            Ok(current) => current,
            Err(error) => {
                outcome(PublicationOutcome::VerificationFailed);
                return Err(error);
            }
        };
        if current.version != version {
            if let Err(error) = result {
                outcome(PublicationOutcome::Unresolved);
                tracing::warn!(
                    revision = plan.revision,
                    "Qdrant publication unresolved; staging retained for possible delayed completion"
                );
                return Err(error);
            }
            outcome(PublicationOutcome::Conflict);
            self.cleanup(identity, &version).await;
            return Err(conflict("concurrent Qdrant update; retry the operation"));
        }
        outcome(PublicationOutcome::Observed);
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

    #[tracing::instrument(name = "qdrant_publish_head", skip_all)]
    async fn publish_head(
        &self,
        identity: &Identity,
        node: NodeHead,
        expected: &str,
    ) -> Result<(), AppError> {
        let input = api::Upsert {
            points: vec![api::Point {
                id: point_id(&["head", &identity.node_id]),
                vector: api::Vectors::default(),
                payload: node,
            }],
            update_filter: (!expected.is_empty()).then(|| api::Filter {
                must: vec![api::matches("version", vec![expected.to_owned()])],
                ..api::Filter::default()
            }),
            update_mode: Some(if expected.is_empty() {
                api::UpdateMode::InsertOnly
            } else {
                api::UpdateMode::UpdateOnly
            }),
        };
        self.api
            .upsert(&collection(&identity.group_id, true), &input)
            .await
    }

    #[tracing::instrument(name = "qdrant_cleanup", skip_all, fields(retained = tracing::field::Empty))]
    async fn cleanup(&self, identity: &Identity, version: &str) {
        let mut retained = false;
        for metadata in [false, true] {
            if self
                .api
                .delete_version(&collection(&identity.group_id, metadata), version)
                .await
                .is_err()
            {
                retained = true;
                tracing::warn!("Qdrant obsolete staging cleanup failed; hidden version retained");
            }
        }
        tracing::Span::current().record("retained", retained);
    }
}
