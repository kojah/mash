//! Rebuildable retrieval state from stored history; no provider discovery/options.
use super::{elapsed_milliseconds, emit_embedding_progress, emit_index_phase, silver};
use crate::AppError;
use crate::semantic::{self, Models};
use crate::storage::{Counts, Storage};
use std::time::Instant;

pub(super) struct Build {
    pub(super) counts: Counts,
    pub(super) ready: bool,
    pub(super) embeddings: semantic::EmbeddingSummary,
    pub(super) model_load_milliseconds: u64,
    pub(super) search_refresh_milliseconds: u64,
    pub(super) embedding_milliseconds: u64,
}

#[tracing::instrument(name = "gold_build", skip_all, fields(source_complete = source.is_complete()))]
pub(super) fn build(
    storage: &mut Storage,
    models: &Models,
    source: &silver::Ingestion,
) -> Result<Build, AppError> {
    let summary = &source.summary;
    let mut model_load_milliseconds = 0;
    let search_started = Instant::now();
    if storage.derived_search_is_dirty()? {
        emit_index_phase(crate::progress::Phase::SearchRebuild, summary);
        crate::progress::during(crate::progress::Phase::SearchRebuild, || {
            storage.refresh_derived_search_state()
        })?;
    } else {
        storage.refresh_derived_search_state()?;
    }
    let search_refresh_milliseconds = elapsed_milliseconds(search_started);
    emit_index_phase(crate::progress::Phase::SemanticEmbeddings, summary);
    let embedding_started = Instant::now();
    let generation = models.generation();
    let already_published = storage.semantic_generation_is_published(&generation)?
        && !storage.has_pending_embeddings()?;
    if !already_published {
        storage.invalidate_embedding_generation(&generation)?;
    }
    let embeddings = if !already_published && storage.has_pending_embeddings()? {
        storage.commit_and_continue_writer()?;
        let model_started = Instant::now();
        let mut embedding_pool =
            crate::progress::during(crate::progress::Phase::ModelLoading, || {
                models.load_indexer()
            })?
            .ok_or_else(|| {
                AppError::model("semantic models are not installed; run `mash models install`")
            })?;
        model_load_milliseconds = elapsed_milliseconds(model_started);
        semantic::rebuild_embeddings(
            storage,
            &mut embedding_pool,
            &generation,
            emit_embedding_progress,
        )?
    } else {
        semantic::EmbeddingSummary::default()
    };
    let embedding_milliseconds = elapsed_milliseconds(embedding_started);
    let counts = storage.counts()?;
    if !already_published && source.is_complete() {
        crate::progress::during(crate::progress::Phase::SemanticFinalize, || {
            storage.mark_semantic_index_ready(&generation)
        })?;
    }
    Ok(Build {
        counts,
        ready: storage.semantic_index_is_ready(&generation)?,
        embeddings,
        model_load_milliseconds,
        search_refresh_milliseconds,
        embedding_milliseconds,
    })
}

#[cfg(test)]
mod tests;
