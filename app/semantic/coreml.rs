//! Apple Silicon Core ML sessions with fixed batch and sequence shapes.
#[cfg(any(test, all(target_os = "macos", target_arch = "aarch64")))]
use super::COREML_SEQUENCE_LENGTH;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use super::{
    COREML_BATCH_SIZE, COREML_CACHE_DIRECTORY, COREML_MODEL_FILE, EMBEDDING_MODEL,
    EMBEDDING_REPOSITORY,
    embeddings::EmbeddingGroup,
    models::{read_model_file, read_tokenizer_files, snapshot},
};
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use crate::AppError;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use fastembed::{
    InitOptionsUserDefined, QuantizationMode, TextEmbedding, UserDefinedEmbeddingModel,
};
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use hf_hub::api::sync::ApiBuilder;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use ort::ep;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use std::collections::HashMap;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use std::fs;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use std::path::{Path, PathBuf};
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(super) struct CoreMlEmbeddingPool {
    root: PathBuf,
    tokenizer: tokenizers::Tokenizer,
    sessions: HashMap<usize, TextEmbedding>,
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
impl CoreMlEmbeddingPool {
    pub(super) fn load(root: &Path) -> Result<Self, AppError> {
        let embedding = load_coreml_embedding(root, 64)?;
        let mut tokenizer = embedding.tokenizer.clone();
        tokenizer.with_padding(None);
        Ok(Self {
            root: root.to_path_buf(),
            tokenizer,
            sessions: HashMap::from([(64, embedding)]),
        })
    }

    pub(super) fn embed(&mut self, groups: &[EmbeddingGroup]) -> Result<Vec<Vec<f32>>, AppError> {
        if groups.is_empty() {
            return Ok(Vec::new());
        }
        if groups.len() > COREML_BATCH_SIZE {
            return Err(AppError::internal("Core ML embedding wave is too large"));
        }
        let mut texts = groups
            .iter()
            .map(|g| g.content.as_str())
            .collect::<Vec<_>>();
        let first_length = self
            .tokenizer
            .encode(texts[0], true)
            .map_err(|error| AppError::model(format!("Core ML tokenization failed: {error}")))?
            .len();
        // Once one input needs the largest shape, no other token counts can
        // change the choice. Avoid a second tokenization of the whole long batch.
        let length = if first_length > 256 {
            first_length
        } else {
            self.tokenizer
                .encode_batch(texts[1..].to_vec(), true)
                .map_err(|error| AppError::model(format!("Core ML tokenization failed: {error}")))?
                .iter()
                .map(tokenizers::Encoding::len)
                .max()
                .unwrap_or(0)
                .max(first_length)
        };
        let sequence = coreml_sequence_bucket(length);
        let embedding = match self.sessions.entry(sequence) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(load_coreml_embedding(&self.root, sequence)?)
            }
        };
        texts.resize(COREML_BATCH_SIZE, "");
        let mut vectors = embedding.embed(texts, None).map_err(|error| {
            AppError::model(format!("Core ML embedding inference failed: {error}"))
        })?;
        if vectors.len() != COREML_BATCH_SIZE {
            return Err(AppError::model(
                "Core ML embedding model returned an unexpected result count",
            ));
        }
        vectors.truncate(groups.len());
        Ok(vectors)
    }
}

#[cfg(any(test, all(target_os = "macos", target_arch = "aarch64")))]
pub(super) fn coreml_sequence_bucket(tokens: usize) -> usize {
    [64, 128, 256, COREML_SEQUENCE_LENGTH]
        .into_iter()
        .find(|&length| length >= tokens)
        .unwrap_or(COREML_SEQUENCE_LENGTH)
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(super) fn load_coreml_embedding(
    root: &Path,
    sequence_length: usize,
) -> Result<TextEmbedding, AppError> {
    let embedding_snapshot = snapshot(root, EMBEDDING_REPOSITORY)?;
    let embedding_info = TextEmbedding::get_model_info(&EMBEDDING_MODEL)
        .map_err(|error| AppError::model(format!("unknown embedding model: {error}")))?;
    let mut embedding_model = UserDefinedEmbeddingModel::new(
        read_model_file(&embedding_snapshot.join(COREML_MODEL_FILE))?,
        read_tokenizer_files(&embedding_snapshot)?,
    )
    .with_quantization(QuantizationMode::None);
    embedding_model.pooling = TextEmbedding::get_default_pooling_method(&EMBEDDING_MODEL);
    embedding_model
        .output_key
        .clone_from(&embedding_info.output_key);

    let cache_dir = root.join(COREML_CACHE_DIRECTORY).join(format!(
        "batch-{COREML_BATCH_SIZE}-sequence-{sequence_length}"
    ));
    fs::create_dir_all(&cache_dir).map_err(AppError::io)?;
    let provider = ep::CoreML::default()
        .with_model_format(ep::coreml::ModelFormat::MLProgram)
        .with_compute_units(ep::coreml::ComputeUnits::All)
        .with_specialization_strategy(ep::coreml::SpecializationStrategy::FastPrediction)
        .with_model_cache_dir(cache_dir.to_string_lossy())
        .build()
        .error_on_failure();
    let options = InitOptionsUserDefined::default()
        .with_execution_providers(vec![provider])
        .with_disable_cpu_fallback(true)
        .with_dimension_override("batch_size", 32)
        .with_dimension_override(
            "sequence_length",
            i64::try_from(sequence_length)
                .map_err(|_| AppError::internal("Core ML sequence length exceeds i64"))?,
        );
    let mut embedding = TextEmbedding::try_new_from_user_defined(embedding_model, options)
        .map_err(|error| {
            AppError::model(format!("failed to load Core ML embedding model: {error}"))
        })?;
    let mut padding = embedding
        .tokenizer
        .get_padding()
        .cloned()
        .ok_or_else(|| AppError::model("Core ML tokenizer is missing padding configuration"))?;
    padding.strategy = tokenizers::PaddingStrategy::Fixed(sequence_length);
    embedding.tokenizer.with_padding(Some(padding));
    Ok(embedding)
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(super) fn download_coreml_embedding(root: &Path) -> Result<(), AppError> {
    let api = ApiBuilder::new()
        .with_cache_dir(root.to_path_buf())
        .with_progress(false)
        .build()
        .map_err(|error| {
            AppError::model(format!("failed to initialize model download: {error}"))
        })?;
    api.model("Xenova/all-MiniLM-L6-v2".to_owned())
        .get(COREML_MODEL_FILE)
        .map(|_| ())
        .map_err(|error| AppError::model(format!("failed to download Core ML model: {error}")))
}
