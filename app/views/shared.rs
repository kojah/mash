//! Shared-recipe embedding for publication and replay.
use crate::AppError;
use crate::semantic::{EmbeddingSummary, LoadedIndexer, Models};
use std::path::Path;
use std::time::Instant;

pub(crate) const BATCH_ROWS: usize = 32;
/// Cache I/O can span inference batches without changing model inputs.
pub(crate) const CACHE_ROWS: usize = 256;
const _: () = assert!(CACHE_ROWS.is_multiple_of(BATCH_ROWS));

#[derive(Default)]
pub(crate) struct Build {
    pub(crate) embeddings: EmbeddingSummary,
    pub(crate) model_load_milliseconds: u64,
    pub(crate) embedding_milliseconds: u64,
}

/// Materialization and publication share vector validation, inference accounting,
/// and association with the exact prepared input. Cache reads are owned by the
/// caller: a publishing destination does not depend on local views availability.
pub(crate) async fn embed_units(
    inputs: &[super::documents::EmbeddingInput<'_>],
    space: (&str, usize),
    model: Option<&Path>,
    pool: &mut Option<LoadedIndexer>,
    result: &mut Build,
    cached: Vec<Option<Vec<f64>>>,
) -> Result<Vec<(Vec<f64>, String)>, AppError> {
    if cached.len() != inputs.len() {
        return Err(AppError::internal(
            "embedding cache cardinality differs from input",
        ));
    }
    let texts = inputs
        .iter()
        .zip(&cached)
        .filter_map(|(input, cached)| cached.is_none().then_some(input.text))
        .collect::<Vec<_>>();
    let vectors = if texts.is_empty() {
        Vec::new()
    } else {
        let models = model.ok_or_else(AppError::models_missing)?;
        embed_texts(models, pool, &texts, result).await?
    };
    if vectors.len() != texts.len()
        || vectors
            .iter()
            .chain(cached.iter().flatten())
            .any(|vector| !super::retrieval::valid_vector(vector, space.1))
    {
        return Err(AppError::model("invalid prepared embeddings"));
    }
    result.embeddings.model_inferences += texts.len() as u64;
    result.embeddings.reused_vectors += (inputs.len() - texts.len()) as u64;
    let mut fresh = vectors.into_iter();
    inputs
        .iter()
        .zip(cached)
        .map(|(input, cached)| {
            let vector = cached
                .or_else(|| fresh.next())
                .ok_or_else(|| AppError::model("missing prepared embedding"))?;
            Ok((vector, input.fingerprint(space.0, space.1)))
        })
        .collect()
}

fn elapsed(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

pub(crate) async fn embed_texts(
    models_dir: &Path,
    pool: &mut Option<LoadedIndexer>,
    texts: &[&str],
    result: &mut Build,
) -> Result<Vec<Vec<f64>>, AppError> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let existing = pool.take();
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
                Models::new(root).load_indexer()
            })?
            .ok_or_else(AppError::models_missing)?,
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
