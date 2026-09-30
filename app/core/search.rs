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
    pub(super) limit: usize,
    pub(super) unit: crate::search_results::SearchUnit,
    pub(super) filters: crate::search_filter::Selection,
    pub(super) shared: bool,
    pub(super) local: bool,
}

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
    if use_shared && use_local {
        return Err(AppError::usage(
            "local and shared search are mutually exclusive",
        ));
    }
    crate::search_query::validate(&query)?;
    let filters = filters.resolve(
        configuration
            .shared
            .as_ref()
            .map_or("local", |c| c.node_id.as_str()),
    )?;
    if use_shared || (!use_local && configuration.shared.is_some()) {
        let (results, shared_error) = shared::search(
            required_shared(configuration)?,
            (!use_shared).then_some(database),
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
        crate::blocking(move || {
            local_search(
                &database,
                &models,
                embeddings,
                query,
                (limit, unit),
                &filters,
            )
        })
        .await
    }
}

fn local_search(
    database_path: &Path,
    models_dir: &Path,
    embeddings_config: Option<semantic::remote::Configuration>,
    query: String,
    options: (usize, crate::search_results::SearchUnit),
    filters: &crate::search_filter::Scope,
) -> Result<SearchResponse, AppError> {
    let models = Models::new(models_dir.to_path_buf()).with_remote(embeddings_config);
    models.require_installed()?;
    let storage = Storage::open_search(database_path, &models.generation())?;
    let mut backend = models.load()?.ok_or_else(|| {
        AppError::model("semantic models are not installed; run `mash models install`")
    })?;
    let results = semantic::hybrid_search_filtered(
        &models.generation(),
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
