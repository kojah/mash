//! Shared-recipe embedding for publication and replay.
use crate::AppError;
use crate::semantic::{self, EmbeddingSummary, LoadedIndexer, Models};
use std::path::Path;
use std::time::Instant;

pub(crate) const BATCH_ROWS: usize = 32;

#[derive(Default)]
pub(crate) struct Build {
    pub(crate) embeddings: EmbeddingSummary,
    pub(crate) model_load_milliseconds: u64,
    pub(crate) embedding_milliseconds: u64,
}

fn elapsed(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

pub(crate) async fn embed_texts(
    models_dir: &Path,
    embeddings: Option<&semantic::remote::Configuration>,
    pool: &mut Option<LoadedIndexer>,
    texts: &[&str],
    result: &mut Build,
) -> Result<Vec<Vec<f64>>, AppError> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let existing = pool.take();
    let remote = embeddings.cloned();
    let root = models_dir.to_path_buf();
    let input = texts
        .iter()
        .map(|text| (*text).to_owned())
        .collect::<Vec<_>>();
    let (loaded, vectors, load_ms, embed_ms) = crate::blocking(move || {
        let loading = existing.is_none();
        let load_started = Instant::now();
        let mut loaded = match existing {
            Some(pool) => pool,
            None => crate::progress::during(crate::progress::Phase::ModelLoading, || {
                Models::new(root).with_remote(remote).load_indexer()
            })?
            .ok_or_else(|| {
                AppError::model("semantic models are not installed; run `mash models install`")
            })?,
        };
        let load_ms = if loading { elapsed(load_started) } else { 0 };
        let references = input.iter().map(String::as_str).collect::<Vec<_>>();
        let embed_started = Instant::now();
        let vectors = loaded.shared_vectors(&references)?;
        Ok((loaded, vectors, load_ms, elapsed(embed_started)))
    })
    .await?;
    *pool = Some(loaded);
    result.model_load_milliseconds += load_ms;
    result.embedding_milliseconds += embed_ms;
    Ok(vectors)
}
