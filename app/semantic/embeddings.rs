//! Bounded embedding batches, deduplication, progress, and persisted vectors.
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
use super::EMBEDDING_WAVE_SIZE;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use super::coreml::CoreMlEmbeddingPool;
use super::models::coreml_embedding_enabled;
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
use super::models::load_local_embedding;
use super::retrieval::quantize_vector;
use super::{
    COREML_BATCH_SIZE, COREML_SEQUENCE_LENGTH, EMBEDDING_BATCH_SIZE, EMBEDDING_CHECKPOINT_ROWS,
    EMBEDDING_MODEL, EMBEDDING_PROGRESS_INTERVAL, EMBEDDING_THREADS_PER_WORKER, EMBEDDING_WORKERS,
    remote,
};
use crate::AppError;
use crate::storage::{EmbeddingWrite, Storage};
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
use fastembed::TextEmbedding;
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Instant;

static EMBEDDING_GENERATION: OnceLock<String> = OnceLock::new();

pub(crate) struct EmbeddingPool {
    pub(super) backend: EmbeddingPoolBackend,
}

pub(super) enum EmbeddingPoolBackend {
    Remote(remote::Client),
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    Cpu(Vec<TextEmbedding>),
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    CoreMl(Box<CoreMlEmbeddingPool>),
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct EmbeddingGroup {
    pub(super) content: String,
    pub(super) message_ids: Vec<String>,
}

#[derive(Debug, Default, Eq, PartialEq)]
pub(crate) struct EmbeddingSummary {
    pub(crate) stored_vectors: u64,
    pub(crate) model_inferences: u64,
    pub(crate) reused_vectors: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EmbeddingProgress {
    pub(crate) stored_vectors: u64,
    pub(crate) total_vectors: u64,
    pub(crate) model_inferences: u64,
    pub(crate) reused_vectors: u64,
    pub(crate) elapsed_milliseconds: u64,
    pub(crate) stored_vectors_per_second: f64,
    pub(crate) eta_seconds: Option<u64>,
}

impl EmbeddingPool {
    pub(crate) fn shared_vectors(&mut self, texts: &[&str]) -> Result<Vec<Vec<f64>>, AppError> {
        let groups = texts
            .iter()
            .map(|text| EmbeddingGroup {
                content: (*text).to_owned(),
                message_ids: Vec::new(),
            })
            .collect::<Vec<_>>();
        let vectors = self.embed_wave(&groups)?;
        if vectors.len() != texts.len() {
            return Err(AppError::model(
                "embedding model returned an unexpected result count",
            ));
        }
        Ok(vectors
            .iter()
            .map(|vector| {
                quantize_vector(vector)
                    .values
                    .into_iter()
                    .map(|value| f64::from(value.cast_signed()))
                    .collect()
            })
            .collect())
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    pub(super) fn load_local(root: &Path) -> Result<Self, AppError> {
        Ok(Self {
            backend: EmbeddingPoolBackend::CoreMl(Box::new(CoreMlEmbeddingPool::load(root)?)),
        })
    }

    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    pub(super) fn load_local(root: &Path) -> Result<Self, AppError> {
        let workers = (0..EMBEDDING_WORKERS)
            .map(|_| load_local_embedding(root, Some(EMBEDDING_THREADS_PER_WORKER)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            backend: EmbeddingPoolBackend::Cpu(workers),
        })
    }

    pub(super) fn embed_wave(
        &mut self,
        groups: &[EmbeddingGroup],
    ) -> Result<Vec<Vec<f32>>, AppError> {
        match &mut self.backend {
            EmbeddingPoolBackend::Remote(client) => client.embed_on_worker(
                &groups.iter().map(|g| g.content.clone()).collect::<Vec<_>>(),
                false,
            ),
            #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
            EmbeddingPoolBackend::Cpu(workers) => embed_cpu_wave(workers, groups),
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            EmbeddingPoolBackend::CoreMl(embedding) => embedding.embed(groups),
        }
    }

    const fn batch_size(&self) -> usize {
        match self.backend {
            EmbeddingPoolBackend::Remote(_) => 32,
            #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
            EmbeddingPoolBackend::Cpu(_) => EMBEDDING_BATCH_SIZE,
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            EmbeddingPoolBackend::CoreMl(_) => COREML_BATCH_SIZE,
        }
    }

    const fn wave_size(&self) -> usize {
        match self.backend {
            EmbeddingPoolBackend::Remote(_) => 32,
            #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
            EmbeddingPoolBackend::Cpu(_) => EMBEDDING_WAVE_SIZE,
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            EmbeddingPoolBackend::CoreMl(_) => COREML_BATCH_SIZE,
        }
    }
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn embed_cpu_wave(
    workers: &mut [TextEmbedding],
    groups: &[EmbeddingGroup],
) -> Result<Vec<Vec<f32>>, AppError> {
    let worker_batches = groups.chunks(EMBEDDING_BATCH_SIZE);
    let results = std::thread::scope(|scope| {
        workers
            .iter_mut()
            .zip(worker_batches)
            .map(|(worker, batch)| {
                scope.spawn(move || {
                    let texts = batch
                        .iter()
                        .map(|group| group.content.as_str())
                        .collect::<Vec<_>>();
                    worker.embed(texts, None).map_err(|error| {
                        AppError::model(format!("embedding inference failed: {error}"))
                    })
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|_| AppError::internal("embedding worker panicked"))?
            })
            .collect::<Result<Vec<_>, _>>()
    })?;
    Ok(results.into_iter().flatten().collect())
}

pub(crate) fn embedding_generation() -> &'static str {
    EMBEDDING_GENERATION.get_or_init(|| {
        let specification = if coreml_embedding_enabled() {
            format!(
                "fastembed=6.0.1;model={EMBEDDING_MODEL};backend=coreml-fp32;\
                 batch={COREML_BATCH_SIZE};sequence={COREML_SEQUENCE_LENGTH};\
                 vector=i8-per-vector-symmetric;cosine=quantized-flat-exact;schema=4"
            )
        } else {
            format!(
                "fastembed=6.0.1;model={EMBEDDING_MODEL};batch={EMBEDDING_BATCH_SIZE};\
                 workers={EMBEDDING_WORKERS};threads={EMBEDDING_THREADS_PER_WORKER};\
                 vector=i8-per-vector-symmetric;cosine=quantized-flat-exact;schema=3"
            )
        };
        blake3::hash(specification.as_bytes()).to_hex().to_string()
    })
}

pub(crate) fn rebuild_embeddings(
    storage: &mut Storage,
    pool: &mut EmbeddingPool,
    generation: &str,
    mut on_progress: impl FnMut(EmbeddingProgress),
) -> Result<EmbeddingSummary, AppError> {
    let groups = plan_embedding_groups(storage.messages_needing_embeddings()?);
    if groups.is_empty() {
        return Ok(EmbeddingSummary::default());
    }
    let total_vectors = groups.iter().try_fold(0_u64, |total, group| {
        total
            .checked_add(count_embeddings(group.message_ids.len())?)
            .ok_or_else(|| AppError::internal("too many embeddings"))
    })?;
    let started = Instant::now();
    let mut last_progress = started;
    let mut estimate = crate::progress::Estimate::new();
    let mut summary = EmbeddingSummary::default();
    on_progress(embedding_progress(&summary, total_vectors, started));
    let mut rows_since_checkpoint = 0_u64;
    let wave_size = pool.wave_size();
    let batch_size = pool.batch_size();
    for wave in groups.chunks(wave_size) {
        let wave_vectors = pool.embed_wave(wave)?;
        if wave.len() != wave_vectors.len() {
            return Err(AppError::model(
                "embedding model returned an unexpected result count",
            ));
        }
        for (batch, batch_vectors) in wave.chunks(batch_size).zip(wave_vectors.chunks(batch_size)) {
            let quantized = batch_vectors
                .iter()
                .map(|vector| quantize_vector(vector))
                .collect::<Vec<_>>();
            let mut rows = Vec::new();
            for (group, vector) in batch.iter().zip(&quantized) {
                for message_id in &group.message_ids {
                    rows.push(EmbeddingWrite {
                        message_id,
                        vector: &vector.values,
                        norm: vector.norm,
                    });
                }
            }
            storage.replace_embeddings(generation, &rows)?;
            let batch_rows = count_embeddings(rows.len())?;
            summary.stored_vectors = summary
                .stored_vectors
                .checked_add(batch_rows)
                .ok_or_else(|| AppError::internal("too many embeddings"))?;
            summary.model_inferences = summary
                .model_inferences
                .checked_add(count_embeddings(batch.len())?)
                .ok_or_else(|| AppError::internal("too many model inferences"))?;
            rows_since_checkpoint = rows_since_checkpoint
                .checked_add(batch_rows)
                .ok_or_else(|| AppError::internal("too many embeddings"))?;
            if rows_since_checkpoint >= EMBEDDING_CHECKPOINT_ROWS {
                storage.checkpoint_writer()?;
                rows_since_checkpoint = 0;
            }
            let now = Instant::now();
            if now.duration_since(last_progress) >= EMBEDDING_PROGRESS_INTERVAL
                || summary.stored_vectors == total_vectors
            {
                let mut progress = embedding_progress(&summary, total_vectors, started);
                progress.eta_seconds = estimate.observe(
                    summary.stored_vectors,
                    total_vectors,
                    progress.elapsed_milliseconds,
                );
                on_progress(progress);
                last_progress = now;
            }
        }
    }
    summary.reused_vectors = summary
        .stored_vectors
        .checked_sub(summary.model_inferences)
        .ok_or_else(|| AppError::internal("model inference count exceeds stored embeddings"))?;
    Ok(summary)
}

fn embedding_progress(
    summary: &EmbeddingSummary,
    total_vectors: u64,
    started: Instant,
) -> EmbeddingProgress {
    let elapsed = started.elapsed();
    #[allow(clippy::cast_precision_loss)]
    let stored_vectors_per_second = if elapsed.is_zero() {
        0.0
    } else {
        summary.stored_vectors as f64 / elapsed.as_secs_f64()
    };
    EmbeddingProgress {
        stored_vectors: summary.stored_vectors,
        total_vectors,
        model_inferences: summary.model_inferences,
        reused_vectors: summary
            .stored_vectors
            .saturating_sub(summary.model_inferences),
        elapsed_milliseconds: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
        stored_vectors_per_second,
        eta_seconds: None,
    }
}

pub(super) fn plan_embedding_groups(
    messages: Vec<crate::storage::SearchableMessage>,
) -> Vec<EmbeddingGroup> {
    let mut grouped = HashMap::<String, Vec<String>>::new();
    for message in messages {
        grouped.entry(message.content).or_default().push(message.id);
    }
    let mut groups = grouped
        .into_iter()
        .map(|(content, mut message_ids)| {
            message_ids.sort_unstable();
            EmbeddingGroup {
                content,
                message_ids,
            }
        })
        .collect::<Vec<_>>();
    groups.sort_unstable_by(|left, right| {
        left.content
            .len()
            .cmp(&right.content.len())
            .then_with(|| left.content.cmp(&right.content))
    });
    groups
}

fn count_embeddings(count: usize) -> Result<u64, AppError> {
    u64::try_from(count).map_err(|_| AppError::internal("too many embeddings"))
}
