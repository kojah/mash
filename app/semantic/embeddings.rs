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
#[cfg(test)]
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Instant;

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
pub(super) mod cpu;

static EMBEDDING_GENERATION: OnceLock<String> = OnceLock::new();

pub(crate) struct EmbeddingPool {
    pub(super) backend: EmbeddingPoolBackend,
    pub(super) assets_lease: Option<std::fs::File>,
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
    /// Offline label experiment: retain document batches, isolate label inputs.
    #[cfg(test)]
    pub(crate) fn singleton_label_vectors(
        &mut self,
        texts: &[&str],
    ) -> Result<Vec<Vec<f64>>, AppError> {
        if texts.len() > 64 {
            return Err(AppError::usage("singleton label batch cap exceeded"));
        }
        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        if let EmbeddingPoolBackend::Cpu(workers) = &mut self.backend {
            let groups = texts
                .iter()
                .map(|text| EmbeddingGroup {
                    content: (*text).to_owned(),
                    message_ids: Vec::new(),
                })
                .collect::<Vec<_>>();
            return Ok(cpu::embed_singletons(workers, &groups)?
                .iter()
                .map(|vector| {
                    quantize_vector(vector)
                        .values
                        .into_iter()
                        .map(|value| f64::from(value.cast_signed()))
                        .collect()
                })
                .collect());
        }
        Err(AppError::usage(
            "singleton label experiment requires the local CPU backend",
        ))
    }

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
            assets_lease: None,
        })
    }

    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    pub(super) fn load_local(root: &Path) -> Result<Self, AppError> {
        let workers = (0..EMBEDDING_WORKERS)
            .map(|_| load_local_embedding(root, Some(EMBEDDING_THREADS_PER_WORKER)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            backend: EmbeddingPoolBackend::Cpu(workers),
            assets_lease: None,
        })
    }

    #[tracing::instrument(name = "embedding_wave", skip_all, fields(inputs = groups.len()))]
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
            EmbeddingPoolBackend::Cpu(workers) => cpu::embed_wave(workers, groups),
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

#[tracing::instrument(name = "rebuild_embeddings", skip_all)]
pub(crate) fn rebuild_embeddings(
    storage: &mut Storage,
    pool: &mut EmbeddingPool,
    generation: &str,
    on_progress: impl FnMut(EmbeddingProgress),
) -> Result<EmbeddingSummary, AppError> {
    rebuild_with_build(
        storage,
        pool,
        generation,
        &crate::gold::config::Build::default(),
        on_progress,
    )
}

pub(crate) fn rebuild_with_build(
    storage: &mut Storage,
    pool: &mut EmbeddingPool,
    generation: &str,
    build: &crate::gold::config::Build,
    mut on_progress: impl FnMut(EmbeddingProgress),
) -> Result<EmbeddingSummary, AppError> {
    let mut plan = crate::progress::during(crate::progress::Phase::SemanticEmbeddings, || {
        if build == &crate::gold::config::Build::default() {
            crate::storage::embedding_plan::EmbeddingPlan::new(storage)
        } else {
            crate::storage::embedding_plan::EmbeddingPlan::with_build(storage, build)
        }
    })?;
    let total_vectors = plan.total_vectors;
    if total_vectors == 0 {
        return Ok(EmbeddingSummary::default());
    }
    let started = Instant::now();
    let mut last_progress = started;
    let mut estimate = crate::progress::Estimate::new();
    let mut summary = EmbeddingSummary::default();
    on_progress(embedding_progress(&summary, total_vectors, started));
    let mut rows_since_checkpoint = 0_u64;
    let wave_size = pool.wave_size();
    let batch_size = pool.batch_size();
    loop {
        let texts = plan.next_wave(wave_size)?;
        if texts.is_empty() {
            break;
        }
        let group_ids = texts.iter().map(|text| text.group_id).collect::<Vec<_>>();
        let wave = texts
            .into_iter()
            .map(|text| EmbeddingGroup {
                content: text.content,
                message_ids: Vec::new(),
            })
            .collect::<Vec<_>>();
        let wave_vectors = pool.embed_wave(&wave)?;
        if wave.len() != wave_vectors.len() {
            return Err(AppError::model(
                "embedding model returned an unexpected result count",
            ));
        }
        for (index, (group, vector)) in group_ids.into_iter().zip(wave_vectors).enumerate() {
            let vector = quantize_vector(&vector);
            summary.model_inferences = summary
                .model_inferences
                .checked_add(1)
                .ok_or_else(|| AppError::internal("too many model inferences"))?;
            let mut after: Option<String> = None;
            loop {
                let ids = plan.message_ids(group, after.as_deref())?;
                if ids.is_empty() {
                    break;
                }
                let rows = ids
                    .iter()
                    .map(|message_id| EmbeddingWrite {
                        message_id,
                        vector: &vector.values,
                        norm: vector.norm,
                    })
                    .collect::<Vec<_>>();
                storage.replace_embeddings(generation, &rows)?;
                let batch_rows = count_embeddings(rows.len())?;
                summary.stored_vectors = summary
                    .stored_vectors
                    .checked_add(batch_rows)
                    .ok_or_else(|| AppError::internal("too many embeddings"))?;
                rows_since_checkpoint = rows_since_checkpoint
                    .checked_add(batch_rows)
                    .ok_or_else(|| AppError::internal("too many embeddings"))?;
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
                after = ids.last().cloned();
            }
            // Preserve the original quantization-batch checkpoint boundary.
            if ((index + 1) % batch_size == 0 || index + 1 == wave.len())
                && rows_since_checkpoint >= EMBEDDING_CHECKPOINT_ROWS
            {
                storage.checkpoint_writer()?;
                rows_since_checkpoint = 0;
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

#[cfg(test)]
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
