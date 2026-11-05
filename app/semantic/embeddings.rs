//! Bounded model execution and vector conversion for the common gold engine.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use super::coreml::CoreMlEmbeddingPool;
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
use super::models::load_local_embedding;
use super::remote;
use super::retrieval::quantize_vector;
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
use super::{EMBEDDING_BATCH_SIZE, EMBEDDING_THREADS_PER_WORKER, EMBEDDING_WORKERS};
use crate::AppError;
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
use fastembed::TextEmbedding;
use std::path::Path;

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
pub(super) mod cpu;

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
    #[cfg_attr(
        all(target_os = "macos", target_arch = "aarch64"),
        allow(
            clippy::unused_self,
            reason = "the CPU-only experiment rejects Core ML backends"
        )
    )]
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
    pub(super) fn load_local(root: &Path, _watching: bool) -> Result<Self, AppError> {
        Ok(Self {
            backend: EmbeddingPoolBackend::CoreMl(Box::new(CoreMlEmbeddingPool::load(root)?)),
            assets_lease: None,
        })
    }

    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    pub(super) fn load_local(root: &Path, watching: bool) -> Result<Self, AppError> {
        // Watchers favor a small persistent footprint over bulk throughput.
        // Keep the same waves and quantization batches regardless of concurrency.
        let worker_count = if watching { 1 } else { EMBEDDING_WORKERS };
        let workers = (0..worker_count)
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
}
