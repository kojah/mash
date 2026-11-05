//! Silver projection and embedding shared by direct and server workflows.
//! Callers own silver snapshots, mutation admission and final certification.
use super::documents::{ConversationRecord, EmbeddingInput, MessageRecord};
use super::qdrant::{ConnectorPoint, Documents, Store};
use super::scope::Scope;
use super::shared::{self as embedding, Build};
use crate::AppError;
use crate::semantic::{LoadedIndexer, remote};
use std::path::Path;

pub(crate) fn connector_point(
    unit: &crate::storage::connector_views::Unit,
    evidence: &crate::storage::connector_views::Evidence,
    vector: Option<Vec<f64>>,
) -> super::qdrant::ConnectorPoint {
    let (group_created_at, group_updated_at) = evidence.group_times();
    super::qdrant::ConnectorPoint {
        source_id: unit.source_id.clone(),
        object: unit.object.clone(),
        group_anchor: unit.group_anchor.clone(),
        created_at: unit.created_at,
        unit: super::qdrant::SearchUnit {
            anchor: unit.anchor.clone(),
            text: unit.text.clone(),
            content: evidence.content().to_owned(),
            title: evidence.title().map(str::to_owned),
            enrichment: None,
            author_kind: evidence.author(),
            sequence: unit.sequence,
            chat_created_at: group_created_at,
            chat_updated_at: group_updated_at,
            ancestors: evidence.ancestors(&unit.source_id),
            generation: None,
        },
        vector,
        embedding_fingerprint: None,
    }
}

pub(crate) struct Writer<'a> {
    store: &'a Store,
    scope: &'a Scope,
    model: Option<(&'a Path, Option<&'a remote::Configuration>)>,
    pool: Option<LoadedIndexer>,
    build: Build,
    documents: Documents,
}

/// Reuse must preserve all stored evidence, not only the embedding text.
pub(crate) fn connector_digest(
    unit: &crate::storage::connector_views::Unit,
    evidence: &crate::storage::connector_views::Evidence,
) -> Result<String, AppError> {
    let point = connector_point(unit, evidence, None);
    let bytes = serde_json::to_vec(&point)
        .map_err(|_| AppError::internal("encode connector projection fingerprint"))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

pub(crate) struct ChatBuild {
    pub(crate) messages: u64,
    pub(crate) embeddings: u64,
    pub(crate) complete: bool,
}

impl<'a> Writer<'a> {
    pub(crate) fn new(
        store: &'a Store,
        scope: &'a Scope,
        model: Option<(&'a Path, Option<&'a remote::Configuration>)>,
    ) -> Self {
        Self {
            store,
            scope,
            model,
            pool: None,
            build: Build::default(),
            documents: Documents::new(scope),
        }
    }

    /// Input is canonical silver in conversation order. Hidden rows still define
    /// inference batch boundaries, which can affect quantized model output.
    pub(crate) async fn write_chat(
        &mut self,
        conversation: &ConversationRecord,
        messages: &[MessageRecord],
    ) -> Result<ChatBuild, AppError> {
        let mut units = super::units::Stream::default();
        let mut stored = 0;
        let mut expected = 0;
        for batch in messages.chunks(embedding::BATCH_ROWS) {
            let rows = batch.to_vec();
            let title = conversation.title.clone();
            let plan = self.store.build().clone();
            let (next, rows, prepared) = crate::blocking(move || {
                let mut prepared = Vec::new();
                for message in &rows {
                    prepared.push(units.prepare(
                        &plan,
                        super::units::Input {
                            chat: &message.conversation_id,
                            ordinal: message.ordinal,
                            context: message.context,
                            role: &message.role,
                            projection: &message.projection,
                        },
                        title.as_deref(),
                    )?);
                }
                Ok((units, rows, prepared))
            })
            .await?;
            units = next;
            let inputs = rows
                .iter()
                .zip(&prepared)
                .filter_map(|(message, text)| {
                    text.as_deref().map(|text| EmbeddingInput {
                        source: crate::source::Source::Provider(conversation.provider),
                        anchor: &message.id,
                        text,
                    })
                })
                .collect::<Vec<_>>();
            let mut vectors = self.embed(&inputs).await?.into_iter();
            for (mut message, unit) in rows.into_iter().zip(prepared) {
                let fingerprint = if unit.is_some() {
                    expected += 1;
                    let (vector, fingerprint) = vectors
                        .next()
                        .ok_or_else(|| AppError::model("missing materialized embedding"))?;
                    message.vector = Some(vector);
                    message.generation = Some(self.scope.generation.clone());
                    Some(fingerprint)
                } else {
                    message.vector = None;
                    message.generation = None;
                    None
                };
                stored += self
                    .documents
                    .push_native(self.store, self.scope, conversation, message, fingerprint)
                    .await?;
            }
        }
        Ok(ChatBuild {
            messages: messages.len() as u64,
            embeddings: stored,
            complete: stored == expected,
        })
    }

    /// Connector normalization supplies the same evidence and bounded writer as
    /// native projection. Callers retain ownership of reconciliation and receipts.
    pub(crate) async fn write_connectors(
        &mut self,
        points: &[ConnectorPoint],
    ) -> Result<(), AppError> {
        for batch in points.chunks(embedding::BATCH_ROWS) {
            let vectors = if self.store.build().dense_enabled() {
                let inputs = batch
                    .iter()
                    .map(|point| EmbeddingInput {
                        source: crate::source::Source::Connector(point.source_id.clone()),
                        anchor: &point.unit.anchor,
                        text: &point.unit.text,
                    })
                    .collect::<Vec<_>>();
                self.embed(&inputs)
                    .await?
                    .into_iter()
                    .map(Some)
                    .collect::<Vec<_>>()
            } else {
                vec![None; batch.len()]
            };
            for (point, vector) in batch.iter().zip(vectors) {
                let mut point = point.clone();
                if let Some((vector, fingerprint)) = vector {
                    point.vector = Some(vector);
                    point.embedding_fingerprint = Some(fingerprint);
                } else {
                    point.vector = None;
                    point.embedding_fingerprint = None;
                }
                self.documents
                    .push_connector(self.store, self.scope, point)
                    .await?;
            }
        }
        Ok(())
    }

    async fn embed(
        &mut self,
        inputs: &[EmbeddingInput<'_>],
    ) -> Result<Vec<(Vec<f64>, String)>, AppError> {
        let cached = self.store.reusable_vectors(self.scope, inputs).await?;
        embedding::embed_units(
            inputs,
            (&self.scope.generation, self.scope.dimensions),
            self.model,
            &mut self.pool,
            &mut self.build,
            cached,
        )
        .await
    }

    /// Preserve coordinator flush boundaries while retaining the loaded model.
    pub(crate) async fn flush(&mut self) -> Result<(), AppError> {
        self.documents.flush(self.store).await
    }

    /// All queued documents must finish before the caller certifies coverage.
    pub(crate) async fn finish(self) -> Result<Build, AppError> {
        self.documents.finish(self.store).await?;
        Ok(self.build)
    }
}
