//! Built-in views: model execution, embeddings, vector caches and semantic retrieval.
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

#[cfg(test)]
pub(crate) use embeddings::EmbeddingPool;

pub(crate) use embeddings::{EmbeddingProgress, EmbeddingSummary};
pub(crate) use models::indexer;
pub(crate) use models::indexer::LoadedIndexer;
pub(crate) use models::{Backend, InstallSummary, LoadedBackend, Models};
pub(crate) use retrieval::{
    SHARED_EMBEDDING_GENERATION, configured_generation, finish_hybrid, fuse, model_space,
    shared_query_vector,
};

const EMBEDDING_MODEL: EmbeddingModel = EmbeddingModel::AllMiniLML6V2Q;
const RERANKER_MODEL: RerankerModel = RerankerModel::JINARerankerV1TurboEn;
const MARKER_NAME: &str = "installed.json";
const EMBEDDING_REPOSITORY: &str = "models--Xenova--all-MiniLM-L6-v2";
const RERANKER_REPOSITORY: &str = "models--jinaai--jina-reranker-v1-turbo-en";
const EMBEDDING_BATCH_SIZE: usize = 8;
const EMBEDDING_WORKERS: usize = 8;
const EMBEDDING_THREADS_PER_WORKER: usize = 2;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const COREML_BATCH_SIZE: usize = 32;
#[cfg(any(test, all(target_os = "macos", target_arch = "aarch64")))]
const COREML_SEQUENCE_LENGTH: usize = 512;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const COREML_MODEL_FILE: &str = "onnx/model.onnx";
const COREML_CACHE_DIRECTORY: &str = "coreml-cache";
