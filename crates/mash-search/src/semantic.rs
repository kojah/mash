//! Built-in views: model execution, embeddings, vector caches and semantic retrieval.
#[cfg(test)]
#[path = "semantic/evaluation/cleanup.rs"]
mod cleanup_experiment;
#[cfg(any(test, all(target_os = "macos", target_arch = "aarch64")))]
mod coreml;
mod embeddings;
mod models;
pub mod remote;
pub mod remote_reranker;
mod retrieval;
#[cfg(any(test, feature = "test-support"))]
pub mod test_embeddings;
#[cfg(test)]
mod tests;

use fastembed::{EmbeddingModel, RerankerModel};

#[cfg(test)]
pub use embeddings::EmbeddingPool;

pub use embeddings::{EmbeddingProgress, EmbeddingSummary};
pub use models::indexer;
pub use models::indexer::LoadedIndexer;
pub use models::{InstallSummary, LoadedBackend, Models};
pub use retrieval::SHARED_EMBEDDING_GENERATION;
pub use retrieval::{finish_hybrid, fuse, shared_query_vector};

const EMBEDDING_MODEL: EmbeddingModel = EmbeddingModel::AllMiniLML6V2Q;
const RERANKER_MODEL: RerankerModel = RerankerModel::JINARerankerV1TurboEn;
const MARKER_NAME: &str = "installed.json";
const EMBEDDING_REPOSITORY: &str = "models--Xenova--all-MiniLM-L6-v2";
pub const RERANKER_REPOSITORY: &str = "models--jinaai--jina-reranker-v1-turbo-en";
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
const EMBEDDING_BATCH_SIZE: usize = 8;
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
const EMBEDDING_WORKERS: usize = 8;
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
const EMBEDDING_THREADS_PER_WORKER: usize = 2;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const COREML_BATCH_SIZE: usize = 32;
#[cfg(any(test, all(target_os = "macos", target_arch = "aarch64")))]
const COREML_SEQUENCE_LENGTH: usize = 512;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const COREML_MODEL_FILE: &str = "onnx/model.onnx";
const COREML_CACHE_DIRECTORY: &str = "coreml-cache";
