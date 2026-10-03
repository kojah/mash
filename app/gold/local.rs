//! Gold: rebuildable local retrieval state from stored silver history.
use crate::AppError;
use crate::progress::milliseconds as elapsed_milliseconds;
use crate::semantic::{self, Models};
use crate::silver::local as silver;
use crate::storage::{Counts, Storage};
use std::time::Instant;

pub(crate) struct Build {
    pub(crate) counts: Counts,
    pub(crate) ready: bool,
    pub(crate) embeddings: semantic::EmbeddingSummary,
    pub(crate) model_load_milliseconds: u64,
    pub(crate) search_refresh_milliseconds: u64,
    pub(crate) embedding_milliseconds: u64,
}

#[tracing::instrument(name = "gold_build", skip_all, fields(source_complete = source.is_complete()))]
pub(crate) fn build(
    storage: &mut Storage,
    models: &Models,
    source: &silver::Ingestion,
) -> Result<Build, AppError> {
    let summary = &source.summary;
    let source_complete = storage.silver_certificate()?.is_complete();
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
    if !source_complete {
        storage.resume_unchanged_semantic_snapshot(&generation)?;
    }
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
        semantic::rebuild_embeddings(storage, &mut embedding_pool, &generation, |event| {
            crate::progress::emit(&crate::progress::Event::embedding(&event));
        })?
    } else {
        semantic::EmbeddingSummary::default()
    };
    let embedding_milliseconds = elapsed_milliseconds(embedding_started);
    let counts = storage.counts()?;
    if !already_published && source_complete {
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

fn emit_index_phase(phase: crate::progress::Phase, summary: &crate::silver::IndexSummary) {
    crate::progress::emit(&crate::progress::Event::files(
        phase,
        summary,
        0,
        None,
        crate::progress::EtaScope::Phase,
    ));
}
