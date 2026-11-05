//! Silver projection and embedding shared by direct and server workflows.
//! Callers own silver snapshots, mutation admission and final certification.
use super::documents::{ConversationRecord, EmbeddingInput, MessageRecord};
use super::qdrant::{Documents, Store};
use super::scope::Scope;
use super::shared::{self as embedding, Build};
use crate::AppError;
use crate::semantic::{LoadedIndexer, remote};
use std::path::Path;

pub(crate) struct Writer<'a> {
    store: &'a Store,
    scope: &'a Scope,
    model: Option<(&'a Path, Option<&'a remote::Configuration>)>,
    pool: Option<LoadedIndexer>,
    build: Build,
    documents: Documents,
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
                            id: &message.id,
                            ordinal: message.ordinal,
                            context: &message.context,
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
