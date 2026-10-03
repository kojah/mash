//! Local and combined retrieval; validation precedes inference and HTTP.
use super::{SearchMode, SearchResponse, required_shared};
use crate::AppError;
use crate::config::ResolvedConfig;
use crate::semantic::{self, Models};
use crate::shared;
use crate::storage::Storage;
use std::path::Path;

pub(super) struct Input {
    pub(super) query: String,
    pub(super) limit: Option<usize>,
    pub(super) unit: Option<crate::search_results::SearchUnit>,
    pub(super) filters: crate::search_filter::Selection,
    pub(super) shared: bool,
    pub(super) local: bool,
}

pub(super) fn resolve(
    query: &str,
    filters: crate::search_filter::Selection,
    shared: bool,
    local: bool,
    node: &str,
) -> Result<crate::search_filter::Scope, AppError> {
    if shared && local {
        return Err(AppError::usage(
            "local and shared search are mutually exclusive",
        ));
    }
    crate::search_query::validate(query)?;
    filters.resolve(node)
}

#[tracing::instrument(name = "search", skip_all)]
pub(super) async fn run(
    configuration: &ResolvedConfig,
    database: &Path,
    models: &Path,
    input: Input,
) -> Result<SearchResponse, AppError> {
    let Input {
        query,
        limit,
        unit,
        filters,
        shared: use_shared,
        local: use_local,
    } = input;
    let (limit, unit) = configuration.gold.query.resolve(limit, unit);
    let filters = resolve(
        &query,
        filters,
        use_shared,
        use_local,
        configuration
            .shared
            .as_ref()
            .map_or("local", |c| c.node_id.as_str()),
    )?;
    if use_shared || (!use_local && configuration.shared.is_some()) {
        let (results, shared_error) = shared::search(
            required_shared(configuration)?,
            (!use_shared).then_some((database, &configuration.gold.build)),
            models,
            configuration.embeddings.as_ref(),
            &query,
            (limit, unit),
            &filters,
        )
        .await?;
        Ok(SearchResponse {
            realized_mode: search_mode(shared_error.is_some(), use_shared),
            unit,
            results: crate::search_results::collect(results, unit, limit, &query),
            query,
            shared: shared_error.is_none(),
            shared_error,
        })
    } else {
        let database = database.to_path_buf();
        let models = models.to_path_buf();
        let embeddings = configuration.embeddings.clone();
        let build = configuration.gold.build.clone();
        crate::blocking(move || {
            local_search(
                &database,
                &models,
                embeddings,
                query,
                (limit, unit),
                &filters,
                &build,
            )
        })
        .await
    }
}

#[tracing::instrument(name = "local_search", skip_all)]
fn local_search(
    database_path: &Path,
    models_dir: &Path,
    embeddings_config: Option<semantic::remote::Configuration>,
    query: String,
    options: (usize, crate::search_results::SearchUnit),
    filters: &crate::search_filter::Scope,
    build: &crate::gold::config::Build,
) -> Result<SearchResponse, AppError> {
    let models = Models::new(models_dir.to_path_buf()).with_remote(embeddings_config);
    models.require_installed()?;
    let generation = build.generation(&models.generation());
    let storage = Storage::open_search(database_path, &generation)?;
    storage.require_gold_build(build, &models.generation())?;
    let mut backend = models.load()?.ok_or_else(|| {
        AppError::model("semantic models are not installed; run `mash models install`")
    })?;
    let results = semantic::hybrid_search_filtered(
        &generation,
        &storage,
        &mut backend,
        &query,
        options,
        (None, None),
        Some(filters),
    )?;
    Ok(SearchResponse {
        realized_mode: SearchMode::Hybrid,
        unit: options.1,
        results: crate::search_results::collect(results, options.1, options.0, &query),
        query,
        shared: false,
        shared_error: None,
    })
}

fn search_mode(failed: bool, shared_only: bool) -> SearchMode {
    if failed {
        SearchMode::LocalHybrid
    } else if shared_only {
        SearchMode::Hybrid
    } else {
        SearchMode::MergedHybrid
    }
}
