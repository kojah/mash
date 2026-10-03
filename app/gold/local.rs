//! Gold: rebuildable local retrieval state from stored silver history.
use crate::AppError;
use crate::progress::milliseconds as elapsed_milliseconds;
use crate::semantic::{self, Models};
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

#[tracing::instrument(name = "gold_build", skip_all)]
pub(crate) fn build(
    storage: &mut Storage,
    models: &Models,
    summary: &crate::silver::IndexSummary,
    build: &crate::gold::config::Build,
) -> Result<Build, AppError> {
    let silver = storage.silver_certificate()?;
    let source_complete = silver.is_complete();
    let model = models.generation();
    let manifest = crate::gold::manifest::Manifest::new(build.clone(), model.clone(), silver);
    let previous = storage.gold_manifest()?;
    let representations_changed = previous
        .as_ref()
        .is_some_and(|m| m.build.representations != build.representations);
    if representations_changed
        || (build != &crate::gold::config::Build::default()
            && !previous.is_some_and(|m| m.matches(build, &model, &manifest.silver)))
    {
        // Title changes are canonical metadata changes, so rebuild all affected
        // representations conservatively until a finer dependency map exists.
        storage.mark_derived_search_dirty()?;
    }
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
    if source_complete {
        storage.publish_lexical_manifest()?;
    }
    if !build.dense_enabled() {
        storage.clear_dense_gold()?;
        storage.publish_gold_manifest(&manifest)?;
        return Ok(Build {
            counts: storage.counts()?,
            ready: source_complete,
            embeddings: semantic::EmbeddingSummary::default(),
            model_load_milliseconds: 0,
            search_refresh_milliseconds,
            embedding_milliseconds: 0,
        });
    }
    emit_index_phase(crate::progress::Phase::SemanticEmbeddings, summary);
    let embedding_started = Instant::now();
    let generation = build.generation(&model);
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
        let progress = |event| crate::progress::emit(&crate::progress::Event::embedding(&event));
        if build == &crate::gold::config::Build::default() {
            semantic::rebuild_embeddings(storage, &mut embedding_pool, &generation, progress)?
        } else {
            semantic::rebuild_with_build(
                storage,
                &mut embedding_pool,
                &generation,
                build,
                progress,
            )?
        }
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
    if source_complete {
        storage.publish_gold_manifest(&manifest)?;
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
