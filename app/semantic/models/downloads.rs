//! Explicit Hugging Face downloads into the caller's isolated asset directory.
use super::{EMBEDDING_MODEL, RERANKER_MODEL};
use crate::AppError;
use fastembed::{TextEmbedding, TextRerank};
use hf_hub::api::sync::ApiBuilder;
use std::path::Path;

const TOKENIZER_FILES: [&str; 4] = [
    "tokenizer.json",
    "config.json",
    "special_tokens_map.json",
    "tokenizer_config.json",
];

pub(super) fn local(root: &Path) -> Result<(), AppError> {
    let info = TextEmbedding::get_model_info(&EMBEDDING_MODEL)
        .map_err(|error| AppError::model(format!("unknown embedding model: {error}")))?;
    download(
        root,
        &info.model_code,
        &info.model_file,
        &info.additional_files,
    )?;
    reranker(root)?;
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    download(
        root,
        &info.model_code,
        crate::semantic::COREML_MODEL_FILE,
        &[],
    )?;
    Ok(())
}

pub(super) fn reranker(root: &Path) -> Result<(), AppError> {
    let info = TextRerank::get_model_info(&RERANKER_MODEL);
    download(
        root,
        &RERANKER_MODEL.to_string(),
        &info.model_file,
        &info.additional_files,
    )
}

fn download(
    root: &Path,
    repository: &str,
    model_file: &str,
    additional: &[String],
) -> Result<(), AppError> {
    // FastEmbed's prebuilt-model constructor overrides its cache_dir with
    // HF_HOME. Use HF Hub explicitly so the installer owns every mutable write.
    // Mirror selection remains supported without mutating process environment.
    let endpoint =
        std::env::var("HF_ENDPOINT").unwrap_or_else(|_| "https://huggingface.co".to_owned());
    let api = ApiBuilder::new()
        .with_cache_dir(root.to_path_buf())
        .with_endpoint(endpoint.clone())
        .with_progress(false)
        .build()
        .map_err(|error| {
            AppError::model(format!("failed to initialize model download: {error}"))
        })?;
    let repo = api.model(repository.to_owned());
    for file in std::iter::once(model_file)
        .chain(additional.iter().map(String::as_str))
        .chain(TOKENIZER_FILES)
    {
        repo.get(file).map_err(|error| {
            AppError::model(format!(
                "failed to download model asset {file} from {endpoint}: {error}; semantic models (about 170 MB) need network access to {endpoint} or a Hugging Face mirror named by HF_ENDPOINT, and word-exact search works without them"
            ))
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
