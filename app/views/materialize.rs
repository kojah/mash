//! Silver projection and embedding shared by direct and server workflows.
//! Callers own silver snapshots, mutation admission and final certification.
use super::documents::{ChatRecord, EmbeddingInput, SearchMessage};
use super::qdrant::{Documents, Store};
use super::scope::Scope;
use super::shared::{self as embedding, Build};
use crate::AppError;
use crate::semantic::LoadedIndexer;
use std::path::Path;

pub(crate) struct Writer<'a> {
    store: &'a Store,
    scope: &'a Scope,
    model: Option<&'a Path>,
    pool: Option<LoadedIndexer>,
    execution: crate::config::Embeddings,
    reuse: bool,
    build: Build,
    documents: Documents,
}

pub(crate) struct ChatBuild {
    pub(crate) messages: u64,
    pub(crate) embeddings: u64,
    pub(crate) complete: bool,
}

#[derive(Clone, Default)]
struct ChatStats {
    messages: u64,
    stored: u64,
    expected: u64,
}

struct PendingChunk<'a> {
    chat: &'a ChatRecord,
    chat_index: usize,
    rows: Vec<SearchMessage>,
    texts: Vec<Option<String>>,
    cached: Vec<Option<Vec<f64>>>,
}

struct Prepared {
    rows: Vec<SearchMessage>,
    texts: Vec<Option<String>>,
    cached: Vec<Option<Vec<f64>>>,
}

impl Prepared {
    async fn read(
        store: &Store,
        scope: &Scope,
        chat: &ChatRecord,
        batch: &[SearchMessage],
        mut units: super::units::Stream,
        reuse: bool,
    ) -> Result<(super::units::Stream, Self), AppError> {
        let rows = batch.to_vec();
        let title = chat.title.clone();
        let plan = store.build().clone();
        let (units, rows, texts) = crate::blocking(move || {
            let mut texts = Vec::with_capacity(rows.len());
            for message in &rows {
                texts.push(units.prepare(
                    &plan,
                    super::units::Input {
                        chat: &message.chat_id,
                        id: &message.id,
                        ordinal: message.ordinal,
                        context: &message.context,
                        role: &message.role,
                        projection: &message.projection,
                    },
                    title.as_deref(),
                )?);
            }
            Ok((units, rows, texts))
        })
        .await?;
        let inputs = inputs(chat, &rows, &texts);
        // A whole-node replay deletes every document first, so Qdrant has
        // nothing to reuse. Skip those per-chat reads entirely.
        let cached = if reuse {
            store.reusable_vectors(scope, &inputs).await?
        } else {
            vec![None; inputs.len()]
        };
        Ok((
            units,
            Self {
                rows,
                texts,
                cached,
            },
        ))
    }
}

fn inputs<'a>(
    chat: &ChatRecord,
    rows: &'a [SearchMessage],
    texts: &'a [Option<String>],
) -> Vec<EmbeddingInput<'a>> {
    rows.iter()
        .zip(texts)
        .filter_map(|(message, text)| {
            text.as_deref().map(|text| EmbeddingInput {
                provider: chat.provider,
                anchor: &message.id,
                text,
            })
        })
        .collect()
}

impl<'a> Writer<'a> {
    pub(crate) fn new(store: &'a Store, scope: &'a Scope, model: Option<&'a Path>) -> Self {
        Self {
            store,
            scope,
            model,
            pool: None,
            execution: crate::config::Embeddings::Local {},
            reuse: true,
            build: Build::default(),
            documents: Documents::new(scope),
        }
    }

    pub(crate) fn with_execution(mut self, execution: crate::config::Embeddings) -> Self {
        self.execution = execution;
        self
    }

    /// Stage writes without awaiting each batch; the caller supplies the barrier.
    pub(crate) fn with_bulk(mut self) -> Self {
        self.documents = self.documents.with_wait(false);
        self
    }

    /// A replay starts from an empty node; do not query Qdrant for vectors to reuse.
    pub(crate) fn without_reuse(mut self) -> Self {
        self.reuse = false;
        self
    }

    /// Input is canonical silver in chat order. Hidden rows still define
    /// inference batch boundaries, which can affect quantized model output.
    pub(crate) async fn write_chat(
        &mut self,
        chat: &ChatRecord,
        messages: &[SearchMessage],
    ) -> Result<ChatBuild, AppError> {
        let mut batches = messages.chunks(embedding::CACHE_ROWS);
        let Some(first) = batches.next() else {
            return Ok(ChatBuild {
                messages: 0,
                embeddings: 0,
                complete: true,
            });
        };
        let (mut units, mut prepared) = Prepared::read(
            self.store,
            self.scope,
            chat,
            first,
            super::units::Stream::default(),
            self.reuse,
        )
        .await?;
        let mut stored = 0;
        let mut expected = 0;
        for batch in batches {
            // Only reads/preparation overlap the single writer. Join both even
            // on error: no queued mutation escapes the caller's admission.
            let (next, written) = tokio::join!(
                Prepared::read(self.store, self.scope, chat, batch, units, self.reuse,),
                self.write_prepared(chat, prepared),
            );
            let (count, wanted) = written?;
            stored += count;
            expected += wanted;
            (units, prepared) = next?;
        }
        let (count, wanted) = self.write_prepared(chat, prepared).await?;
        stored += count;
        expected += wanted;
        Ok(ChatBuild {
            messages: messages.len() as u64,
            embeddings: stored,
            complete: stored == expected,
        })
    }

    /// Materialize a bounded window of chats together, so remote inference
    /// fills whole batches across chat boundaries instead of one request per
    /// chat. Documents are still written per chat and per message.
    pub(crate) async fn write_chats(
        &mut self,
        chats: &[(ChatRecord, Vec<SearchMessage>)],
    ) -> Result<Vec<(String, ChatBuild)>, AppError> {
        let batch_rows = embedding::batch_rows(&self.execution);
        // Remote requests are sent in pairs, so fill two batches before
        // flushing. This also avoids discarding the tail of each chat group.
        let pending_rows = if matches!(self.execution, crate::config::Embeddings::Remote { .. }) {
            batch_rows * 2
        } else {
            batch_rows
        };
        let mut pending = Vec::new();
        let mut pending_inputs = 0usize;
        let mut stats = vec![ChatStats::default(); chats.len()];
        for (chat_index, (chat, messages)) in chats.iter().enumerate() {
            stats[chat_index].messages = messages.len() as u64;
            let mut units = super::units::Stream::default();
            for batch in messages.chunks(embedding::CACHE_ROWS) {
                let (next_units, prepared) =
                    Prepared::read(self.store, self.scope, chat, batch, units, self.reuse).await?;
                units = next_units;
                let Prepared {
                    rows,
                    texts,
                    cached,
                } = prepared;
                let mut rows = rows.into_iter();
                let mut texts = texts.into_iter();
                let mut cached = cached.into_iter();
                loop {
                    let group_rows: Vec<_> = rows.by_ref().take(batch_rows).collect();
                    if group_rows.is_empty() {
                        break;
                    }
                    let group_texts: Vec<_> = texts.by_ref().take(group_rows.len()).collect();
                    let input_count = group_texts.iter().filter(|text| text.is_some()).count();
                    stats[chat_index].expected += input_count as u64;
                    let group_cached: Vec<_> = cached.by_ref().take(input_count).collect();
                    if !pending.is_empty() && pending_inputs + input_count > pending_rows {
                        self.flush_pending(&mut pending, &mut stats).await?;
                        pending_inputs = 0;
                    }
                    pending.push(PendingChunk {
                        chat,
                        chat_index,
                        rows: group_rows,
                        texts: group_texts,
                        cached: group_cached,
                    });
                    pending_inputs += input_count;
                    if pending_inputs >= pending_rows {
                        self.flush_pending(&mut pending, &mut stats).await?;
                        pending_inputs = 0;
                    }
                }
            }
        }
        if !pending.is_empty() {
            self.flush_pending(&mut pending, &mut stats).await?;
        }
        Ok(chats
            .iter()
            .zip(stats)
            .map(|((chat, _), stats)| {
                (
                    chat.id.clone(),
                    ChatBuild {
                        messages: stats.messages,
                        embeddings: stats.stored,
                        complete: stats.stored == stats.expected,
                    },
                )
            })
            .collect())
    }

    async fn flush_pending(
        &mut self,
        pending: &mut Vec<PendingChunk<'_>>,
        stats: &mut [ChatStats],
    ) -> Result<(), AppError> {
        if pending.is_empty() {
            return Ok(());
        }
        let mut flat_inputs = Vec::new();
        let mut flat_cached = Vec::new();
        for chunk in pending.iter() {
            flat_inputs.extend(inputs(chunk.chat, &chunk.rows, &chunk.texts));
            flat_cached.extend(chunk.cached.iter().cloned());
        }
        let vectors = embedding::embed_units(
            &flat_inputs,
            (&self.scope.generation, self.scope.dimensions),
            self.model,
            &mut self.pool,
            &mut self.build,
            flat_cached,
            &self.execution,
        )
        .await?;
        drop(flat_inputs);
        let mut vectors = vectors.into_iter();
        for chunk in pending.drain(..) {
            for (mut message, text) in chunk.rows.into_iter().zip(chunk.texts) {
                let fingerprint = if text.is_some() {
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
                let stored = self
                    .documents
                    .push_native(self.store, self.scope, chunk.chat, message, fingerprint)
                    .await?;
                stats[chunk.chat_index].stored += stored;
            }
        }
        Ok(())
    }

    async fn write_prepared(
        &mut self,
        chat: &ChatRecord,
        prepared: Prepared,
    ) -> Result<(u64, u64), AppError> {
        let Prepared {
            rows,
            texts,
            cached,
        } = prepared;
        let mut cached = cached.into_iter();
        let mut vectors = Vec::new();
        let batch_rows = embedding::batch_rows(&self.execution);
        for (rows, texts) in rows.chunks(batch_rows).zip(texts.chunks(batch_rows)) {
            let inputs = inputs(chat, rows, texts);
            let evidence = cached.by_ref().take(inputs.len()).collect();
            vectors.extend(
                embedding::embed_units(
                    &inputs,
                    (&self.scope.generation, self.scope.dimensions),
                    self.model,
                    &mut self.pool,
                    &mut self.build,
                    evidence,
                    &self.execution,
                )
                .await?,
            );
        }
        let mut vectors = vectors.into_iter();
        let mut stored = 0;
        let mut expected = 0;
        for (mut message, unit) in rows.into_iter().zip(texts) {
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
                .push_native(self.store, self.scope, chat, message, fingerprint)
                .await?;
        }
        Ok((stored, expected))
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
