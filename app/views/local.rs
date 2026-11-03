//! Built-in views: rebuildable local retrieval state from stored silver history.
use crate::AppError;
use crate::progress::milliseconds as elapsed_milliseconds;
use crate::semantic::{self, Models};
use crate::storage::{Counts, SnapshotCoverage, Storage};
use std::time::Instant;

pub(crate) struct Build {
    pub(crate) counts: Counts,
    pub(crate) ready: bool,
    pub(crate) embeddings: semantic::EmbeddingSummary,
    pub(crate) model_load_milliseconds: u64,
    pub(crate) search_refresh_milliseconds: u64,
    pub(crate) embedding_milliseconds: u64,
}

#[tracing::instrument(name = "views_build", skip_all)]
pub(crate) fn build(
    storage: &mut Storage,
    models: &Models,
    summary: &crate::silver::IndexSummary,
    build: &crate::views::config::Build,
) -> Result<Build, AppError> {
    let silver = storage.silver_certificate()?;
    let views_allowed = silver.permits_views();
    let model = models.generation();
    let manifest = crate::views::manifest::Manifest::new(build.clone(), model.clone(), silver);
    let previous = storage.views_manifest()?;
    let representations_changed = previous
        .as_ref()
        .is_some_and(|m| m.build.representations != build.representations);
    // The published views were certified for this recipe, model and silver
    // revision; silver revisions change with every canonical write.
    let certified_unchanged = previous
        .as_ref()
        .is_some_and(|m| m.matches(build, &model, &manifest.silver));
    if representations_changed
        || (build != &crate::views::config::Build::default() && !certified_unchanged)
    {
        // Title changes are canonical metadata changes, so rebuild all affected
        // representations conservatively until a finer dependency map exists.
        // A recipe change must replace a deferred ingestion delta with a full
        // rebuild; otherwise unchanged messages never enter the dense queue.
        storage.defer_search_updates()?;
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
    if views_allowed {
        storage.publish_lexical_manifest()?;
    }
    if !build.dense_enabled() {
        storage.clear_dense_views()?;
        storage.publish_views_manifest(&manifest)?;
        return Ok(Build {
            counts: storage.counts()?,
            ready: views_allowed,
            embeddings: semantic::EmbeddingSummary::default(),
            model_load_milliseconds: 0,
            search_refresh_milliseconds,
            embedding_milliseconds: 0,
        });
    }
    emit_index_phase(crate::progress::Phase::SemanticEmbeddings, summary);
    let embedding_started = Instant::now();
    let generation = build.generation(&model);
    storage.restrict_dense_queue(build)?;
    resume_snapshot(storage, &generation, views_allowed, certified_unchanged)?;
    let already_published = storage.semantic_generation_is_published(&generation)?
        && !storage.has_pending_embeddings()?;
    if !already_published {
        storage.invalidate_embedding_generation(&generation)?;
        storage.restrict_dense_queue(build)?;
    }
    let mut embeddings = if !already_published && storage.has_pending_embeddings()? {
        storage.commit_and_continue_writer()?;
        let model_started = Instant::now();
        let mut embedding_pool =
            crate::progress::during(crate::progress::Phase::ModelLoading, || {
                models.load_indexer()
            })?
            .ok_or_else(AppError::models_missing)?;
        model_load_milliseconds = elapsed_milliseconds(model_started);
        let progress = |event| crate::progress::emit(&crate::progress::Event::embedding(&event));
        if build == &crate::views::config::Build::default() {
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
    embed_connectors(storage, models, &generation, &mut embeddings)?;
    let embedding_milliseconds = elapsed_milliseconds(embedding_started);
    let counts = storage.counts()?;
    if !already_published && views_allowed {
        crate::progress::during(crate::progress::Phase::SemanticFinalize, || {
            storage.mark_semantic_build_ready(&generation, build)
        })?;
    }
    if views_allowed {
        storage.publish_views_manifest(&manifest)?;
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

/// Keep the ready snapshot from before this scan when silver did not change.
fn resume_snapshot(
    storage: &mut Storage,
    generation: &str,
    source_complete: bool,
    certified_unchanged: bool,
) -> Result<(), AppError> {
    if source_complete && certified_unchanged {
        // A scan that changed nothing keeps its certified snapshot instead of
        // re-certifying every vector against unchanged silver.
        storage.resume_unchanged_semantic_snapshot(generation, SnapshotCoverage::Certified)
    } else if !source_complete {
        storage.resume_unchanged_semantic_snapshot(generation, SnapshotCoverage::Recheck)
    } else {
        Ok(())
    }
}

/// Connector units carry their own vectors, independent of native recipes;
/// they count toward the index summary like native vectors.
fn embed_connectors(
    storage: &mut Storage,
    models: &Models,
    generation: &str,
    summary: &mut semantic::EmbeddingSummary,
) -> Result<(), AppError> {
    if storage.connector_embeddings_complete(generation)? {
        return Ok(());
    }
    storage.commit_and_continue_writer()?;
    let mut pool = models
        .load_indexer()?
        .ok_or_else(AppError::models_missing)?;
    let stored = semantic::embed_connector_units(storage, &mut pool, generation)?;
    summary.stored_vectors = summary.stored_vectors.saturating_add(stored);
    summary.model_inferences = summary.model_inferences.saturating_add(stored);
    Ok(())
}

#[cfg(test)]
mod selection_tests;
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
