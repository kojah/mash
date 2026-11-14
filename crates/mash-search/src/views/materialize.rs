//! Silver projection and embedding shared by direct and server workflows.
//! Callers own silver snapshots, mutation admission and final certification.
use super::documents::{ChatRecord, EmbeddingInput, SearchMessage};
use super::qdrant::{Documents, Store};
use super::scope::Scope;
use super::shared::{self as embedding, Build};
use crate::AppError;
use crate::semantic::LoadedIndexer;
use std::path::Path;

pub struct Writer<'a> {
    store: &'a Store,
    scope: &'a Scope,
    model: Option<&'a Path>,
    pool: Option<LoadedIndexer>,
    execution: crate::config::Embeddings,
    reuse: bool,
    words_survive_inference: bool,
    build: Build,
    documents: Documents,
}

pub struct ChatBuild {
    pub messages: u64,
    pub embeddings: u64,
    pub complete: bool,
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

pub struct Prepared {
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
    pub fn new(store: &'a Store, scope: &'a Scope, model: Option<&'a Path>) -> Self {
        Self {
            store,
            scope,
            model,
            pool: None,
            execution: crate::config::Embeddings::Local {},
            reuse: true,
            words_survive_inference: false,
            build: Build::default(),
            documents: Documents::new(scope),
        }
    }

    pub fn with_execution(mut self, execution: crate::config::Embeddings) -> Self {
        self.execution = execution;
        self
    }

    /// Stage writes without awaiting each batch; the caller supplies the barrier.
    pub fn with_bulk(mut self) -> Self {
        self.documents = self.documents.with_wait(false);
        self
    }

    /// When inference fails, store the remaining documents without vectors
    /// and record why, instead of failing the pass: their text stays
    /// searchable by word and their chats stay incomplete for dense. Nothing
    /// is embedded again in this pass, so a failed request is never retried.
    pub fn keeping_words_without_inference(mut self) -> Self {
        self.words_survive_inference = true;
        self
    }

    /// Whether inference has already failed in this pass, so nothing more
    /// will be embedded by it.
    pub fn inference_failed(&self) -> bool {
        self.build.inference_error.is_some()
    }

    /// A replay starts from an empty node; do not query Qdrant for vectors to reuse.
    pub fn without_reuse(mut self) -> Self {
        self.reuse = false;
        self
    }

    /// Input is canonical silver in chat order. Hidden rows still define
    /// inference batch boundaries, which can affect quantized model output.
    /// Production materializes chat windows; focused tests use this helper.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn write_chat(
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
    pub async fn write_chats(
        &mut self,
        chats: &[(ChatRecord, Vec<SearchMessage>)],
    ) -> Result<Vec<(String, ChatBuild)>, AppError> {
        let batch_rows = embedding::batch_rows(&self.execution);
        // Fill one batch per in-flight remote request before flushing. This
        // also avoids discarding the tail of each chat group.
        let pending_rows = if matches!(self.execution, crate::config::Embeddings::Remote { .. }) {
            batch_rows * crate::semantic::remote::REQUEST_CONCURRENCY
        } else {
            batch_rows
        };
        let mut pending = Vec::new();
        let mut pending_inputs = 0usize;
        let mut stats = vec![ChatStats::default(); chats.len()];
        for (chat_index, (chat, messages)) in chats.iter().enumerate() {
            stats[chat_index].messages = messages.len() as u64;
            let (prepared, kept) = self.prepare_chat(chat, messages).await?;
            stats[chat_index].expected += kept;
            stats[chat_index].stored += kept;
            self.build.embeddings.reused_vectors += kept;
            for prepared in prepared {
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

    /// Prepare a whole chat, drop the rows whose documents are already
    /// stored as they would be written, then look up reusable vectors for the
    /// rest together. Most rows of a long agent chat carry no dense input, so
    /// a lookup per row batch would cost one Qdrant request for a handful of
    /// vectors. Returns the dense documents left in place.
    async fn prepare_chat(
        &self,
        chat: &ChatRecord,
        messages: &[SearchMessage],
    ) -> Result<(Vec<Prepared>, u64), AppError> {
        let mut units = super::units::Stream::default();
        let mut prepared = Vec::new();
        for batch in messages.chunks(embedding::CACHE_ROWS) {
            let (next, batch) =
                Prepared::read(self.store, self.scope, chat, batch, units, false).await?;
            units = next;
            prepared.push(batch);
        }
        if !self.reuse {
            return Ok((prepared, 0));
        }
        let expected = prepared
            .iter()
            .flat_map(|batch| batch.rows.iter().zip(&batch.texts))
            .filter(|(message, _)| !message.projection.is_empty())
            .map(|(message, text)| super::qdrant::Expected {
                message,
                dense: text.as_deref(),
            })
            .collect::<Vec<_>>();
        let mut unchanged = self
            .store
            .unchanged_documents(self.scope, chat, &expected)
            .await?
            .into_iter();
        drop(expected);
        let mut kept = 0;
        for batch in &mut prepared {
            let rows = std::mem::take(&mut batch.rows);
            let texts = std::mem::take(&mut batch.texts);
            for (message, text) in rows.into_iter().zip(texts) {
                // A row with nothing to search has no document to compare.
                if !message.projection.is_empty()
                    && unchanged
                        .next()
                        .ok_or_else(|| AppError::internal("incomplete document comparison"))?
                {
                    kept += u64::from(text.is_some());
                    continue;
                }
                batch.rows.push(message);
                batch.texts.push(text);
            }
        }
        let lookup = prepared
            .iter()
            .flat_map(|batch| inputs(chat, &batch.rows, &batch.texts))
            .collect::<Vec<_>>();
        let mut cached = self
            .store
            .reusable_vectors(self.scope, &lookup)
            .await?
            .into_iter();
        drop(lookup);
        for batch in &mut prepared {
            let dense = batch.texts.iter().filter(|text| text.is_some()).count();
            batch.cached = cached.by_ref().take(dense).collect();
            if batch.cached.len() != dense {
                return Err(AppError::internal("incomplete reusable vector lookup"));
            }
        }
        Ok((prepared, kept))
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
        let vectors = if self.build.inference_error.is_some() {
            None
        } else {
            match embedding::embed_units(
                &flat_inputs,
                (&self.scope.generation, self.scope.dimensions),
                self.model,
                &mut self.pool,
                &mut self.build,
                flat_cached,
                &self.execution,
            )
            .await
            {
                Ok(vectors) => Some(vectors),
                Err(error)
                    if self.words_survive_inference
                        && matches!(
                            error.error.kind,
                            crate::ErrorKind::Model | crate::ErrorKind::Configuration
                        ) =>
                {
                    self.build.inference_error = Some(error.error.message);
                    None
                }
                Err(error) => return Err(error),
            }
        };
        drop(flat_inputs);
        let mut vectors = vectors.map(Vec::into_iter);
        for chunk in pending.drain(..) {
            for (mut message, text) in chunk.rows.into_iter().zip(chunk.texts) {
                let fingerprint = if let (Some(_), Some(vectors)) = (&text, &mut vectors) {
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

    #[cfg(any(test, feature = "test-support"))]
    pub async fn write_prepared(
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
    pub async fn flush(&mut self) -> Result<(), AppError> {
        self.documents.flush(self.store).await
    }

    /// All queued documents must finish before the caller certifies coverage.
    pub async fn finish(self) -> Result<Build, AppError> {
        self.documents.finish(self.store).await?;
        Ok(self.build)
    }
}
