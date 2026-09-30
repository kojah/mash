//! Explicit model installation, asset validation, and cached search backends.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use super::coreml::download_coreml_embedding;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use super::embeddings::EmbeddingGroup;
use super::embeddings::{EmbeddingPool, EmbeddingPoolBackend};
use super::remote;
use super::{
    EMBEDDING_MODEL, EMBEDDING_REPOSITORY, MARKER_NAME, RERANKER_MODEL, RERANKER_REPOSITORY,
    embedding_generation, model_space,
};
use crate::AppError;
use fastembed::{
    InitOptionsUserDefined, RerankInitOptions, RerankInitOptionsUserDefined, TextEmbedding,
    TextInitOptions, TextRerank, UserDefinedEmbeddingModel, UserDefinedRerankingModel,
};
use serde::Serialize;
use std::fs;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;
mod assets;
use assets::{InstallMarker, inventory, write_marker};
pub(super) use assets::{read_model_file, read_tokenizer_files, snapshot};

pub(crate) struct Models {
    root: PathBuf,
    remote: Option<remote::Configuration>,
}

pub(crate) struct Backend {
    embedding: Option<TextEmbedding>,
    remote: Option<remote::Client>,
    reranker: TextRerank,
}

#[derive(PartialEq, Eq)]
struct BackendCacheKey {
    root: PathBuf,
    marker_modified: Option<SystemTime>,
    files: Vec<(String, u64, Option<SystemTime>)>,
}

struct CachedBackend {
    key: BackendCacheKey,
    backend: Backend,
}

// MCP already serializes model work. Retain at most one idle local backend;
// in-flight searches own their backend without holding a lock across I/O.
static LOCAL_BACKEND: Mutex<Option<CachedBackend>> = Mutex::new(None);

pub(crate) struct LoadedBackend {
    backend: Option<Backend>,
    cache_key: Option<BackendCacheKey>,
}

impl Deref for LoadedBackend {
    type Target = Backend;

    fn deref(&self) -> &Backend {
        self.backend
            .as_ref()
            .expect("loaded backend is present until drop")
    }
}

impl DerefMut for LoadedBackend {
    fn deref_mut(&mut self) -> &mut Backend {
        self.backend
            .as_mut()
            .expect("loaded backend is present until drop")
    }
}

impl Drop for LoadedBackend {
    fn drop(&mut self) {
        if let (Some(key), Some(backend)) = (self.cache_key.take(), self.backend.take()) {
            let previous = LOCAL_BACKEND
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .replace(CachedBackend { key, backend });
            // Model destruction can be expensive; do it after releasing the lock.
            drop(previous);
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct InstallSummary {
    embedding_model: &'static str,
    reranker_model: &'static str,
    embedding_dimensions: usize,
    files: usize,
}

impl Models {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root, remote: None }
    }

    pub(crate) fn with_remote(mut self, config: Option<remote::Configuration>) -> Self {
        self.remote = config;
        self
    }

    pub(crate) fn require_indexer(&self) -> Result<(), AppError> {
        if let Some(config) = &self.remote {
            remote::Client::new(config.clone()).map(|_| ())
        } else {
            self.require_installed()
        }
    }

    pub(crate) fn generation(&self) -> String {
        self.remote.as_ref().map_or_else(
            || embedding_generation().to_owned(),
            |c| model_space(&c.model, c.dimensions),
        )
    }

    pub(crate) fn is_installed(&self) -> bool {
        self.read_valid_marker().is_ok()
    }

    pub(crate) fn require_installed(&self) -> Result<(), AppError> {
        if !self.root.join(MARKER_NAME).is_file() {
            return Err(AppError::model(
                "semantic models are not installed; run `mash models install`",
            ));
        }
        self.read_valid_marker().map(|_| ())
    }

    pub(crate) fn install(&self) -> Result<InstallSummary, AppError> {
        if let Some(config) = &self.remote {
            config.validate()?;
            fs::create_dir_all(&self.root).map_err(AppError::io)?;
            let mut reranker = TextRerank::try_new(
                RerankInitOptions::new(RERANKER_MODEL)
                    .with_cache_dir(self.root.clone())
                    .with_show_download_progress(false),
            )
            .map_err(|_| AppError::model("failed to install reranking model"))?;
            reranker
                .rerank("installation check", ["installation check"], false, None)
                .map_err(|_| AppError::model("reranking model installation check failed"))?;
            let files = inventory(&self.root)?;
            let summary = InstallSummary {
                embedding_model: "remote",
                reranker_model: "jinaai/jina-reranker-v1-turbo-en",
                embedding_dimensions: config.dimensions,
                files: files.len(),
            };
            write_marker(
                &self.root,
                &InstallMarker {
                    schema: 1,
                    embedding_model: EMBEDDING_MODEL.to_string(),
                    reranker_model: RERANKER_MODEL.to_string(),
                    embedding_dimensions: config.dimensions,
                    coreml_embedding: coreml_embedding_enabled(),
                    embedding_available: false,
                    files,
                },
            )?;
            return Ok(summary);
        }
        fs::create_dir_all(&self.root).map_err(AppError::io)?;
        let mut backend = Backend::download_and_load(&self.root)?;
        let embeddings = backend.embed(&["semantic model installation check"])?;
        let embedding_dimensions = embeddings
            .first()
            .map(Vec::len)
            .ok_or_else(|| AppError::model("embedding model returned no smoke result"))?;
        let reranked = backend.rerank(
            "semantic model installation check",
            &["unrelated", "semantic model installation check"],
        )?;
        if reranked.len() != 2 {
            return Err(AppError::model(
                "reranker returned an unexpected smoke result count",
            ));
        }
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            let mut indexer = EmbeddingPool::load_local(&self.root)?;
            let smoke = [EmbeddingGroup {
                content: "semantic Core ML installation check".to_owned(),
                message_ids: vec!["smoke".to_owned()],
            }];
            if indexer.embed_wave(&smoke)?.len() != 1 {
                return Err(AppError::model(
                    "Core ML embedding model returned an unexpected smoke result count",
                ));
            }
        }

        let files = inventory(&self.root)?;
        if files.is_empty() {
            return Err(AppError::model("model installation produced no assets"));
        }
        let marker = InstallMarker {
            schema: 1,
            embedding_model: EMBEDDING_MODEL.to_string(),
            reranker_model: RERANKER_MODEL.to_string(),
            embedding_dimensions,
            coreml_embedding: coreml_embedding_enabled(),
            embedding_available: true,
            files,
        };
        write_marker(&self.root, &marker)?;
        Ok(InstallSummary {
            embedding_model: embedding_model_description(),
            reranker_model: "jinaai/jina-reranker-v1-turbo-en",
            embedding_dimensions,
            files: marker.files.len(),
        })
    }

    pub(crate) fn load(&self) -> Result<Option<LoadedBackend>, AppError> {
        if !self.root.join(MARKER_NAME).is_file() {
            return Ok(None);
        }
        let marker = self.read_valid_marker()?;
        if let Some(config) = &self.remote {
            Ok(Some(LoadedBackend {
                backend: Some(Backend {
                    embedding: None,
                    remote: Some(remote::Client::new(config.clone())?),
                    reranker: load_reranker(&self.root)?,
                }),
                cache_key: None,
            }))
        } else {
            let key = BackendCacheKey {
                root: self.root.clone(),
                marker_modified: fs::metadata(self.root.join(MARKER_NAME))
                    .map_err(AppError::io)?
                    .modified()
                    .ok(),
                files: marker
                    .files
                    .into_iter()
                    .map(|file| {
                        let modified = fs::metadata(self.root.join(&file.path))
                            .map_err(AppError::io)?
                            .modified()
                            .ok();
                        Ok((file.path, file.size, modified))
                    })
                    .collect::<Result<_, AppError>>()?,
            };
            let cached = LOCAL_BACKEND
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            let backend = match cached {
                Some(cached) if cached.key == key => cached.backend,
                _ => Backend::load_local(&self.root)?,
            };
            Ok(Some(LoadedBackend {
                backend: Some(backend),
                cache_key: Some(key),
            }))
        }
    }

    pub(crate) fn load_indexer(&self) -> Result<Option<EmbeddingPool>, AppError> {
        if let Some(config) = &self.remote {
            return Ok(Some(EmbeddingPool {
                backend: EmbeddingPoolBackend::Remote(remote::Client::new(config.clone())?),
            }));
        }
        if !self.root.join(MARKER_NAME).is_file() {
            return Ok(None);
        }
        self.read_valid_marker()?;
        EmbeddingPool::load_local(&self.root).map(Some)
    }

    fn read_valid_marker(&self) -> Result<InstallMarker, AppError> {
        assets::read_valid_marker(&self.root, self.remote.is_some())
    }
}

fn load_reranker(root: &Path) -> Result<TextRerank, AppError> {
    let reranker_snapshot = snapshot(root, RERANKER_REPOSITORY)?;
    let reranker_info = TextRerank::get_model_info(&RERANKER_MODEL);
    let reranker_model = UserDefinedRerankingModel::new(
        reranker_snapshot.join(reranker_info.model_file),
        read_tokenizer_files(&reranker_snapshot)?,
    );
    let reranker = TextRerank::try_new_from_user_defined(
        reranker_model,
        RerankInitOptionsUserDefined::default(),
    )
    .map_err(|error| AppError::model(format!("failed to load reranking model: {error}")))?;
    Ok(reranker)
}

impl Backend {
    fn download_and_load(root: &Path) -> Result<Self, AppError> {
        let embedding = TextEmbedding::try_new(
            TextInitOptions::new(EMBEDDING_MODEL)
                .with_cache_dir(root.to_path_buf())
                .with_show_download_progress(false),
        )
        .map_err(|error| AppError::model(format!("failed to load embedding model: {error}")))?;
        let reranker = TextRerank::try_new(
            RerankInitOptions::new(RERANKER_MODEL)
                .with_cache_dir(root.to_path_buf())
                .with_show_download_progress(false),
        )
        .map_err(|error| AppError::model(format!("failed to load reranking model: {error}")))?;
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        download_coreml_embedding(root)?;
        Ok(Self {
            embedding: Some(embedding),
            remote: None,
            reranker,
        })
    }

    pub(super) fn load_local(root: &Path) -> Result<Self, AppError> {
        let embedding = load_local_embedding(root, None)?;
        let reranker = load_reranker(root)?;
        Ok(Self {
            embedding: Some(embedding),
            remote: None,
            reranker,
        })
    }

    pub(crate) fn embed<S: AsRef<str> + Send + Sync>(
        &mut self,
        texts: &[S],
    ) -> Result<Vec<Vec<f32>>, AppError> {
        if let Some(client) = &self.remote {
            return client.embed_on_worker(
                &texts
                    .iter()
                    .map(|t| t.as_ref().to_owned())
                    .collect::<Vec<_>>(),
                true,
            );
        }
        self.embedding
            .as_mut()
            .ok_or_else(|| AppError::internal("missing local embedding model"))?
            .embed(texts, None)
            .map_err(|error| AppError::model(format!("embedding inference failed: {error}")))
    }

    pub(crate) fn rerank<S: AsRef<str> + Send + Sync>(
        &mut self,
        query: S,
        documents: &[S],
    ) -> Result<Vec<fastembed::RerankResult>, AppError> {
        let mut order = (0..documents.len()).collect::<Vec<_>>();
        let bucket = |index: usize| documents[index].as_ref().len().next_power_of_two();
        order.sort_by_key(|&index| bucket(index));
        let mut scores = Vec::with_capacity(documents.len());
        // Small batches of similarly sized documents amortize inference calls
        // without making a long history message pad every short candidate.
        for group in order.chunk_by(|&a, &b| bucket(a) == bucket(b)) {
            for indices in group.chunks(4) {
                let refs = indices
                    .iter()
                    .map(|&index| documents[index].as_ref())
                    .collect::<Vec<_>>();
                let results = self
                    .reranker
                    .rerank(query.as_ref(), &refs, false, Some(4))
                    .map_err(|error| {
                        AppError::model(format!("reranking inference failed: {error}"))
                    })?;
                for mut result in results {
                    result.index = *indices.get(result.index).ok_or_else(|| {
                        AppError::model("reranker returned an invalid document index")
                    })?;
                    scores.push(result);
                }
            }
        }
        scores.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.index.cmp(&b.index))
        });
        Ok(scores)
    }

    /// Focus long candidates on query evidence before the tokenizer truncates
    /// them. Keep the model's raw logits: they are not calibrated probabilities.
    pub(crate) fn rerank_search(
        &mut self,
        query: &str,
        documents: &[&str],
    ) -> Result<Vec<fastembed::RerankResult>, AppError> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }
        let windows = documents
            .iter()
            .map(|document| crate::search_results::rerank_document(document, query))
            .collect::<Vec<_>>();
        let refs = windows.iter().map(AsRef::as_ref).collect::<Vec<_>>();
        self.rerank(query, &refs)
    }
}

pub(super) fn load_local_embedding(
    root: &Path,
    intra_threads: Option<usize>,
) -> Result<TextEmbedding, AppError> {
    let embedding_snapshot = snapshot(root, EMBEDDING_REPOSITORY)?;
    let embedding_info = TextEmbedding::get_model_info(&EMBEDDING_MODEL)
        .map_err(|error| AppError::model(format!("unknown embedding model: {error}")))?;
    let mut embedding_model = UserDefinedEmbeddingModel::new(
        read_model_file(&embedding_snapshot.join(&embedding_info.model_file))?,
        read_tokenizer_files(&embedding_snapshot)?,
    )
    .with_quantization(TextEmbedding::get_quantization_mode(&EMBEDDING_MODEL));
    embedding_model.pooling = TextEmbedding::get_default_pooling_method(&EMBEDDING_MODEL);
    embedding_model
        .output_key
        .clone_from(&embedding_info.output_key);
    let options = intra_threads.map_or_else(InitOptionsUserDefined::default, |threads| {
        InitOptionsUserDefined::default().with_intra_threads(threads)
    });
    TextEmbedding::try_new_from_user_defined(embedding_model, options)
        .map_err(|error| AppError::model(format!("failed to load embedding model: {error}")))
}

pub(super) const fn coreml_embedding_enabled() -> bool {
    cfg!(all(target_os = "macos", target_arch = "aarch64"))
}

const fn embedding_model_description() -> &'static str {
    if coreml_embedding_enabled() {
        "sentence-transformers/all-MiniLM-L6-v2 (Core ML FP32)"
    } else {
        "sentence-transformers/all-MiniLM-L6-v2 (quantized ONNX)"
    }
}

#[cfg(test)]
mod tests;
