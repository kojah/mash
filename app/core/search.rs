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
    pub(super) intent: Option<crate::gold::exact::Intent>,
    pub(super) limit: Option<usize>,
    pub(super) unit: Option<crate::search_results::SearchUnit>,
    pub(super) fusion: crate::gold::config::FusionOverrides,
    pub(super) diagnostics: bool,
    pub(super) filters: crate::search_filter::Selection,
    pub(super) shared: bool,
    pub(super) local: bool,
}

pub(super) fn resolve_fusion(
    overrides: crate::gold::config::FusionOverrides,
    intent: crate::gold::exact::Intent,
    defaults: crate::gold::config::Fusion,
) -> Result<crate::gold::config::Fusion, AppError> {
    if intent == crate::gold::exact::Intent::ExactMention && !overrides.is_empty() {
        return Err(AppError::usage(
            "fusion overrides require related-discussion intent",
        ));
    }
    overrides.resolve(defaults)
}

pub(super) fn resolve(
    query: &str,
    intent: crate::gold::exact::Intent,
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
    match intent {
        crate::gold::exact::Intent::RelatedDiscussion => crate::search_query::validate(query)?,
        crate::gold::exact::Intent::ExactMention => {
            crate::gold::exact::Literal::new(query)?;
        }
    }
    filters.resolve(node)
}

pub(super) async fn run(
    configuration: &ResolvedConfig,
    database: &Path,
    models: &Path,
    input: Input,
) -> Result<SearchResponse, AppError> {
    let policy = if input.diagnostics {
        let (limit, _) = configuration.gold.query.resolve(input.limit, input.unit);
        let fusion = resolve_fusion(
            input.fusion,
            input.intent.unwrap_or(configuration.gold.query.intent),
            configuration.gold.query.fusion,
        )?;
        let scope = if input.shared {
            crate::gold::diagnostics::Scope::Shared
        } else if input.local || configuration.shared.is_none() {
            crate::gold::diagnostics::Scope::Local
        } else {
            crate::gold::diagnostics::Scope::Combined
        };
        Some((limit, fusion, scope))
    } else {
        None
    };
    let mut response = run_inner(configuration, database, models, input).await?;
    if let Some((limit, fusion, scope)) = policy {
        response.diagnostics = Some(crate::gold::diagnostics::Diagnostics::new(
            &response, scope, limit, fusion,
        ));
    }
    Ok(response)
}

#[tracing::instrument(name = "search", skip_all)]
async fn run_inner(
    configuration: &ResolvedConfig,
    database: &Path,
    models: &Path,
    input: Input,
) -> Result<SearchResponse, AppError> {
    let Input {
        query,
        intent,
        limit,
        unit,
        fusion,
        diagnostics: _,
        filters,
        shared: use_shared,
        local: use_local,
    } = input;
    let (limit, unit) = configuration.gold.query.resolve(limit, unit);
    let mut options = crate::gold::config::Options::new(limit, unit);
    let intent = intent.unwrap_or(configuration.gold.query.intent);
    options.fusion = resolve_fusion(fusion, intent, configuration.gold.query.fusion)?;
    let filters = resolve(
        &query,
        intent,
        filters,
        use_shared,
        use_local,
        configuration
            .shared
            .as_ref()
            .map_or("local", |c| c.node_id.as_str()),
    )?;
    if intent == crate::gold::exact::Intent::ExactMention {
        if use_shared || (!use_local && configuration.shared.is_some()) {
            let (hits, shared_error) = shared::search_exact(
                required_shared(configuration)?,
                (!use_shared).then_some(database),
                &query,
                unit,
                &filters,
            )
            .await?;
            return Ok(SearchResponse {
                diagnostics: None,
                intent,
                realized_mode: if shared_error.is_some() {
                    SearchMode::LocalLexical
                } else if use_shared {
                    SearchMode::Lexical
                } else {
                    SearchMode::MergedLexical
                },
                unit,
                results: crate::search_results::collect(hits, unit, limit, &query),
                query,
                shared: shared_error.is_none(),
                shared_error,
            });
        }
        let literal = crate::gold::exact::Literal::new(&query)?;
        let database = database.to_path_buf();
        return crate::blocking(move || {
            local_exact(&database, &literal, query, (limit, unit), &filters)
        })
        .await;
    }
    if use_shared || (!use_local && configuration.shared.is_some()) {
        let (results, shared_error) = shared::search(
            required_shared(configuration)?,
            (!use_shared).then_some((database, &configuration.gold.build)),
            models,
            configuration.embeddings.as_ref(),
            &query,
            options,
            &filters,
        )
        .await?;
        Ok(SearchResponse {
            diagnostics: None,
            intent,
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
                &database, &models, embeddings, query, options, &filters, &build,
            )
        })
        .await
    }
}

fn local_exact(
    database: &Path,
    literal: &crate::gold::exact::Literal,
    query: String,
    (limit, unit): (usize, crate::search_results::SearchUnit),
    filters: &crate::search_filter::Scope,
) -> Result<SearchResponse, AppError> {
    let storage = Storage::open_lexical(database)?;
    let hits = storage.search_exact(literal, unit.candidate_limit(), filters)?;
    let hits = crate::gold::exact::rank(hits, unit);
    Ok(SearchResponse {
        diagnostics: None,
        intent: crate::gold::exact::Intent::ExactMention,
        realized_mode: SearchMode::Lexical,
        unit,
        results: crate::search_results::collect(hits, unit, limit, &query),
        query,
        shared: false,
        shared_error: None,
    })
}

#[tracing::instrument(name = "local_search", skip_all)]
fn local_search(
    database_path: &Path,
    models_dir: &Path,
    embeddings_config: Option<semantic::remote::Configuration>,
    query: String,
    options: crate::gold::config::Options,
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
        diagnostics: None,
        intent: crate::gold::exact::Intent::RelatedDiscussion,
        realized_mode: SearchMode::Hybrid,
        unit: options.unit,
        results: crate::search_results::collect(results, options.unit, options.limit, &query),
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
