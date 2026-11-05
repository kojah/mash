//! Native silver projection and embedding shared by direct and server workflows.
//! Callers own silver snapshots, mutation admission and final certification.
use super::documents::{ConversationRecord, MessageRecord};
use super::qdrant::{Documents, Store};
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
    }
}

pub(crate) struct NativeWriter<'a> {
    store: &'a Store,
    scope: &'a Scope,
    models: &'a Path,
    embeddings: Option<&'a remote::Configuration>,
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

impl<'a> NativeWriter<'a> {
    pub(crate) fn new(
        store: &'a Store,
        scope: &'a Scope,
        models: &'a Path,
        embeddings: Option<&'a remote::Configuration>,
    ) -> Self {
        Self {
            store,
            scope,
            models,
            embeddings,
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
            let texts = prepared
                .iter()
                .flatten()
                .map(String::as_str)
                .collect::<Vec<_>>();
            let mut vectors = embedding::embed_texts(
                self.models,
                self.embeddings,
                &mut self.pool,
                &texts,
                &mut self.build,
            )
            .await?
            .into_iter();
            if vectors.len() != texts.len() {
                return Err(AppError::model("missing materialized embedding"));
            }
            self.build.embeddings.model_inferences += texts.len() as u64;
            for (mut message, unit) in rows.into_iter().zip(prepared) {
                if unit.is_some() {
                    expected += 1;
                    message.vector = vectors.next();
                    message.generation = Some(self.scope.generation.clone());
                }
                stored += self
                    .documents
                    .push(self.store, self.scope, conversation, message)
                    .await?;
            }
        }
        Ok(ChatBuild {
            messages: messages.len() as u64,
            embeddings: stored,
            complete: stored == expected,
        })
    }

    /// All queued documents must finish before the caller certifies coverage.
    pub(crate) async fn finish(self) -> Result<Build, AppError> {
        self.documents.finish(self.store).await?;
        Ok(self.build)
    }
}
