//! Gold: model execution, embeddings, vector caches and semantic retrieval.
#[cfg(any(test, all(target_os = "macos", target_arch = "aarch64")))]
mod coreml;
mod embeddings;
#[cfg(test)]
pub(crate) mod evaluation;
mod models;
pub(crate) mod remote;
mod retrieval;
#[cfg(test)]
mod tests;

use fastembed::{EmbeddingModel, RerankerModel};
use std::time::Duration;

pub(crate) use embeddings::{
    EmbeddingPool, EmbeddingProgress, EmbeddingSummary, embedding_generation, rebuild_embeddings,
    rebuild_with_build,
};
pub(crate) use models::{Backend, InstallSummary, LoadedBackend, Models};
pub(crate) use retrieval::{
    SHARED_EMBEDDING_GENERATION, configured_generation, fuse, hybrid_search_filtered,
    local_candidates, model_space, shared_query_vector,
};

const EMBEDDING_MODEL: EmbeddingModel = EmbeddingModel::AllMiniLML6V2Q;
const RERANKER_MODEL: RerankerModel = RerankerModel::JINARerankerV1TurboEn;
const MARKER_NAME: &str = "installed.json";
const EMBEDDING_REPOSITORY: &str = "models--Xenova--all-MiniLM-L6-v2";
const RERANKER_REPOSITORY: &str = "models--jinaai--jina-reranker-v1-turbo-en";
const EMBEDDING_BATCH_SIZE: usize = 8;
const EMBEDDING_WORKERS: usize = 8;
const EMBEDDING_THREADS_PER_WORKER: usize = 2;
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
const EMBEDDING_WAVE_SIZE: usize = EMBEDDING_BATCH_SIZE * EMBEDDING_WORKERS;
const COREML_BATCH_SIZE: usize = 32;
const COREML_SEQUENCE_LENGTH: usize = 512;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const COREML_MODEL_FILE: &str = "onnx/model.onnx";
const COREML_CACHE_DIRECTORY: &str = "coreml-cache";
const EMBEDDING_CHECKPOINT_ROWS: u64 = 4_096;
const EMBEDDING_PROGRESS_INTERVAL: Duration = Duration::from_secs(1);
