//! Stage immutable data, then CAS the active node pointer. No partially visible uploads.
use super::CatalogRecord;
use super::{
    AppError, Document, Head, Identity, NodeHead, PointKind, Store, api, collection, failure,
    point_id, sparse, tokens,
};
#[cfg(test)]
use super::{Chunk, Manifest};
use crate::shared::gold_manifest;
#[cfg(test)]
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

fn publication_timestamp() -> String {
    chrono::DateTime::from(std::time::SystemTime::now())
        .to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

fn publication_manifests(
    plan: &Plan,
    identity: &Identity,
    silver: &crate::shared::silver_revision::State,
    checked: bool,
) -> (
    Option<gold_manifest::Manifest>,
    crate::shared::lexical_manifest::Manifest,
) {
    (
        gold_manifest::publication(
            plan.source_build.as_ref(),
            checked,
            plan.loaded.head.as_ref(),
            identity,
            silver,
        ),
        crate::shared::lexical_manifest::Manifest::new(silver.clone()),
    )
}

pub(super) fn staging_version() -> Result<String, AppError> {
    let mut random = [0u8; 32];
    getrandom::fill(&mut random)
        .map_err(|_| AppError::internal("create Qdrant staging identity"))?;
    Ok(blake3::hash(&random).to_hex().to_string())
}

impl Store {
    pub(in crate::shared) async fn rebuild_gold(
        &self,
        identity: &Identity,
        file: tempfile::NamedTempFile,
    ) -> Result<Indexed, AppError> {
        let incoming = file.as_file().metadata().map_err(AppError::io)?.len();
        if incoming > u64::try_from(self.max_node_bytes).unwrap_or(u64::MAX) {
            return Err(AppError::usage(
                "gold upload exceeds the configured snapshot limit",
            ));
        }
        let pending = self.pending_snapshot(identity).await?;
        let reservation = self
            .snapshot_budget
            .reserve(pending.encoded_bytes(), incoming)?;
        let generation = identity.generation.clone();
        let dimensions = identity.dimensions;
        let build = self.build.clone();
        let (plan, _reservation) = crate::blocking(move || {
            super::super::gold_rebuild::plan(
                &file,
                pending.decode()?,
                &generation,
                dimensions,
                &build,
            )
            .map(|plan| (plan, reservation))
        })
        .await?;
        let instance = plan
            .loaded
            .head
            .as_ref()
            .ok_or_else(failure)?
            .instance
            .clone();
        self.commit(identity, plan, instance, false).await
    }

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
        let identity_for_plan = identity.clone();
        let build = self.build.clone();
        let (plan, _reservation) = crate::blocking(move || {
            snapshot::plan(&file, pending.decode()?, &identity_for_plan, &build)
                .map(|plan| (plan, reservation))
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
                source_build: None,
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
                head.silver_coverage.clear();
            }
            Ok((plan, forgotten, reservation))
        })
        .await?;
        self.commit(identity, plan, instance, false).await?;
        Ok(Forgotten { forgotten })
    }
    #[cfg(test)]
    pub(super) async fn stage(
        &self,
        identity: &Identity,
        data: &Data,
        encoded: &Encoded,
        version: &str,
    ) -> Result<Manifest, AppError> {
        let manifest = self.stage_snapshot(identity, encoded, version).await?;
        self.stage_catalog(identity, data, version, None).await?;
        self.stage_documents(identity, data, version, None).await?;
        Ok(manifest)
    }
    #[cfg(test)]
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
    pub(super) async fn stage_catalog(
        &self,
        identity: &Identity,
        data: &Data,
        version: &str,
        versions: Option<&std::collections::BTreeMap<String, super::ThreadSnapshot>>,
    ) -> Result<(), AppError> {
        let mut catalog = api::StagingBatch::default();
        let collection = collection(&identity.group_id, true);
        for conversation in data.conversations.values() {
            let version = if let Some(versions) = versions {
                let Some(thread) = versions.get(&conversation.id) else {
                    continue;
                };
                thread.version.as_str()
            } else {
                version
            };
            catalog
                .push(
                    &self.api,
                    &collection,
                    api::Point {
                        id: point_id(&["catalog", version, &conversation.id]),
                        vector: api::Vectors::default(),
                        payload: CatalogRecord {
                            kind: PointKind::Catalog,
                            version: version.into(),
                            node_id: identity.node_id.clone(),
                            conversation: conversation.clone(),
                        },
                    },
                )
                .await?;
        }
        catalog.flush(&self.api, &collection).await
    }
    #[tracing::instrument(name = "qdrant_stage_documents", skip_all)]
    pub(super) async fn stage_documents(
        &self,
        identity: &Identity,
        data: &Data,
        version: &str,
        versions: Option<&std::collections::BTreeMap<String, super::ThreadSnapshot>>,
    ) -> Result<(), AppError> {
        let mut points = api::StagingBatch::default();
        let collection = collection(&identity.group_id, false);
        for message in data.messages.values() {
            let version = if let Some(versions) = versions {
                let Some(thread) = versions.get(&message.conversation_id) else {
                    continue;
                };
                thread.version.as_str()
            } else {
                version
            };
            let conversation = data
                .conversations
                .get(&message.conversation_id)
                .ok_or_else(failure)?;
            points
                .push(
                    &self.api,
                    &collection,
                    api::Point {
                        id: point_id(&["message", version, &message.id]),
                        vector: api::Vectors {
                            dense: message
                                .vector
                                .clone()
                                .map(|v| (self.dense_name(identity).into(), v)),
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
                    },
                )
                .await?;
        }
        points.flush(&self.api, &collection).await
    }
    #[tracing::instrument(name = "qdrant_publish", skip_all, fields(revision = plan.revision, outcome = tracing::field::Empty))]
    #[allow(
        clippy::too_many_lines,
        reason = "keep staging, conditional publication, ambiguous-write recovery and safe reclamation in one lifecycle"
    )]
    pub(super) async fn commit(
        &self,
        identity: &Identity,
        plan: Plan,
        instance: String,
        checked: bool,
    ) -> Result<Indexed, AppError> {
        let gold_version = self.gold_version(identity).await?;
        let max_bytes = self.max_node_bytes;
        let prior_node = self.node(identity).await?;
        let reuse = prior_node.head.as_ref().is_some_and(|head| {
            head.gold_manifest.as_ref().is_some_and(|manifest| {
                manifest.matches(
                    &self.build,
                    &identity.generation,
                    head.silver_revision.as_ref(),
                )
            })
        });
        let version = staging_version()?;
        let incoming_version = version.clone();
        let prior = prior_node.clone();
        let (plan, mut prepared, silver_revision) = crate::blocking(move || {
            super::incremental::prepare(plan, &prior, &incoming_version, max_bytes, reuse)
        })
        .await?;
        let counts = plan.loaded.data.counts(identity);
        let staged = prepared.staged_versions(&version);
        let previous = plan.loaded.head.as_ref();
        let (gold_manifest, lexical_manifest) =
            publication_manifests(&plan, identity, &silver_revision, checked);
        let manifest = match Box::pin(self.stage_incremental(
            identity,
            &plan.loaded.data,
            &mut prepared,
            &version,
        ))
        .await
        {
            Ok(manifest) => manifest,
            Err(error) => {
                outcome(PublicationOutcome::StagingFailed);
                self.cleanup_versions(identity, &staged).await;
                return Err(error);
            }
        };
        // Record publication preparation after staging, which can take minutes.
        // The subsequent conditional write remains the visibility boundary;
        // this application timestamp is not an exact database commit time.
        let now = publication_timestamp();
        let head = Head {
            lexical_manifest: Some(lexical_manifest),
            gold_manifest,
            silver_coverage: previous.map_or_else(Vec::new, |h| h.silver_coverage.clone()),
            silver_revision: Some(silver_revision),
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
        let node = NodeHead {
            kind: PointKind::Head,
            node_id: identity.node_id.clone(),
            version: version.clone(),
            head: Some(head),
            manifest: Some(manifest),
            threads: Some(prepared.threads),
        };
        let active_versions = node.versions();
        let expected = &plan.loaded.expected;
        if let Err(error) = self.unchanged_gold(identity, &gold_version).await {
            self.cleanup_versions(identity, &staged).await;
            return Err(error);
        }
        // A failed request may still commit after this reread. Seeing another head
        // only permits reclamation when the conditional write completed successfully.
        // If verification fails, leave staging intact as well.
        let result = self
            .publish_head(identity, node, expected, &gold_version)
            .await;
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
            self.cleanup_versions(identity, &staged).await;
            return Err(conflict("concurrent Qdrant update; retry the operation"));
        }
        outcome(PublicationOutcome::Observed);
        // If activation overlapped publication, retain the committed node head
        // but do not acknowledge it. New-recipe reads reject its stale manifest.
        self.unchanged_gold(identity, &gold_version).await?;
        if !expected.is_empty() {
            let mut obsolete = prior_node
                .versions()
                .into_iter()
                .filter(|version| !active_versions.contains(version))
                .collect::<Vec<_>>();
            obsolete.push(expected.clone());
            obsolete.sort();
            obsolete.dedup();
            self.cleanup_versions(identity, &obsolete).await;
        }
        // Deletion can start another optimization after staging has finished.
        // Pay that drain in indexing rather than the first interactive search.
        self.api
            .settle_search(&collection(&identity.group_id, false))
            .await?;
        if self.build.dense_enabled() && counts.embeddings > 0 {
            let configuration = self.active_configuration(identity).await?;
            let mut versions = configuration
                .nodes
                .as_ref()
                .ok_or_else(failure)?
                .values()
                .flat_map(NodeHead::versions)
                .collect::<Vec<_>>();
            versions.sort_unstable();
            versions.dedup();
            self.api
                .warm_search(
                    &collection(&identity.group_id, false),
                    self.dense_name(identity),
                    identity.dimensions,
                    &versions,
                    configuration.vector_search,
                )
                .await?;
        }
        self.unchanged_gold(identity, &gold_version).await?;
        Ok(Indexed {
            counts,
            revision: plan.revision,
            removed_messages: plan.removed,
            purged_conversations: plan.purged,
        })
    }

    #[tracing::instrument(name = "qdrant_cleanup", skip_all, fields(retained = tracing::field::Empty))]
    async fn cleanup_versions(&self, identity: &Identity, versions: &[String]) {
        let mut retained = false;
        for versions in versions.chunks(128) {
            for metadata in [false, true] {
                if self
                    .api
                    .delete_versions(&collection(&identity.group_id, metadata), versions)
                    .await
                    .is_err()
                {
                    retained = true;
                    tracing::warn!(
                        "Qdrant obsolete staging cleanup failed; hidden version retained"
                    );
                }
            }
        }
        tracing::Span::current().record("retained", retained);
    }
}
