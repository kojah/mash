//! Local and combined retrieval; validation precedes inference and HTTP.
use super::{SearchMode, SearchResponse, required_shared};
use crate::AppError;
use crate::config::ResolvedConfig;
use crate::semantic::{self, Models};
use crate::shared;
use crate::storage::Storage;
use std::path::Path;

pub(super) struct Input {
    pub(super) shared_vector_search: Option<crate::views::vector::Search>,
    pub(super) query: String,
    pub(super) intent: Option<crate::views::exact::Intent>,
    pub(super) limit: Option<usize>,
    pub(super) offset: usize,
    pub(super) unit: Option<crate::search_results::SearchUnit>,
    pub(super) fusion: crate::views::config::FusionOverrides,
    pub(super) ranking: Option<crate::views::config::Ranking>,
    pub(super) diagnostics: bool,
    pub(super) filters: crate::search_filter::Selection,
    pub(super) shared: bool,
    pub(super) local: bool,
}

pub(super) async fn preflight(
    configuration: &ResolvedConfig,
    operation: &super::Operation,
) -> Result<(), AppError> {
    let super::Operation::Search {
        query,
        intent,
        limit,
        offset,
        unit,
        fusion,
        ranking,
        shared_vector_search,
        filters,
        shared,
        local,
        ..
    } = operation
    else {
        return Ok(());
    };
    let intent = intent.unwrap_or(configuration.views.query.intent);
    configuration.views.query.resolve(*limit, *offset, *unit)?;
    resolve(
        query,
        intent,
        (**filters).clone(),
        *shared,
        *local,
        node(configuration),
    )?;
    if !shared {
        configuration.views.build.require_intent(intent)?;
    }
    resolve_fusion(*fusion, intent, configuration.views.query.fusion)?;
    resolve_ranking(*ranking, intent, configuration.views.query.ranking)?;
    validate_vector(
        *shared_vector_search,
        intent,
        *local,
        configuration.shared.is_some(),
    )?;
    if (*shared || shared_vector_search.is_some())
        && intent == crate::views::exact::Intent::RelatedDiscussion
    {
        shared::require_dense(required_shared(configuration)?).await?;
    }
    Ok(())
}

pub(super) fn validate_vector(
    policy: Option<crate::views::vector::Search>,
    intent: crate::views::exact::Intent,
    local: bool,
    configured: bool,
) -> Result<(), AppError> {
    if let Some(policy) = policy {
        policy.validate_request()?;
        if intent == crate::views::exact::Intent::ExactMention || local || !configured {
            return Err(AppError::usage(
                "shared vector overrides require related-discussion search against a configured shared index",
            ));
        }
    }
    Ok(())
}

pub(super) fn resolve_fusion(
    overrides: crate::views::config::FusionOverrides,
    intent: crate::views::exact::Intent,
    defaults: crate::views::config::Fusion,
) -> Result<crate::views::config::Fusion, AppError> {
    if intent == crate::views::exact::Intent::ExactMention && !overrides.is_empty() {
        return Err(AppError::usage(
            "fusion overrides require related-discussion intent",
        ));
    }
    overrides.resolve(defaults)
}

pub(super) fn resolve_ranking(
    ranking: Option<crate::views::config::Ranking>,
    intent: crate::views::exact::Intent,
    default: crate::views::config::Ranking,
) -> Result<crate::views::config::Ranking, AppError> {
    if intent == crate::views::exact::Intent::ExactMention && ranking.is_some() {
        return Err(AppError::usage(
            "ranking overrides require related-discussion intent",
        ));
    }
    Ok(ranking.unwrap_or(default))
}

pub(super) fn resolve(
    query: &str,
    intent: crate::views::exact::Intent,
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
        crate::views::exact::Intent::RelatedDiscussion => crate::search_query::validate(query)?,
        crate::views::exact::Intent::ExactMention => {
            crate::views::exact::Literal::new(query)?;
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
    let vector_override = input.shared_vector_search;
    let shared_only = input.shared;
    let sort = input.filters.sort;
    let (page, _) = configuration
        .views
        .query
        .resolve(input.limit, input.offset, input.unit)?;
    let policy = if input.diagnostics {
        let fusion = resolve_fusion(
            input.fusion,
            input.intent.unwrap_or(configuration.views.query.intent),
            configuration.views.query.fusion,
        )?;
        let scope = if input.shared {
            crate::views::diagnostics::Scope::Shared
        } else if input.local || configuration.shared.is_none() {
            crate::views::diagnostics::Scope::Local
        } else {
            crate::views::diagnostics::Scope::Combined
        };
        let ranking = resolve_ranking(
            input.ranking,
            input.intent.unwrap_or(configuration.views.query.intent),
            configuration.views.query.ranking,
        )?;
        Some((fusion, scope, ranking, input.filters.fields()))
    } else {
        None
    };
    let mut response = run_inner(configuration, database, models, input).await?;
    // Only the page's results are reordered: pages still partition the
    // relevance ranking, so paging never skips or repeats a result.
    if sort == crate::search_filter::Sort::Recent {
        response.results.newest_first();
    }
    crate::search_results::lookup::attach(
        &mut response.results,
        node(configuration),
        response.shared,
        shared_only,
    );
    if let Some((fusion, scope, ranking, filter_fields)) = policy {
        response.diagnostics = Some(crate::views::diagnostics::Diagnostics::new(
            &response, scope, page, fusion, ranking,
        ));
        if let Some(diagnostics) = &mut response.diagnostics {
            diagnostics.shared_vector_override = vector_override;
            diagnostics.filter_fields = filter_fields;
        }
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
        shared_vector_search,
        query,
        intent,
        limit,
        offset,
        unit,
        fusion,
        ranking,
        diagnostics: _,
        filters,
        shared: use_shared,
        local: use_local,
    } = input;
    let (page, unit) = configuration.views.query.resolve(limit, offset, unit)?;
    let mut options = crate::views::config::Options::new(unit);
    let intent = intent.unwrap_or(configuration.views.query.intent);
    validate_vector(
        shared_vector_search,
        intent,
        use_local,
        configuration.shared.is_some(),
    )?;
    options.shared_vector_search = shared_vector_search;
    if !use_shared {
        configuration.views.build.require_intent(intent)?;
    }
    options.fusion = resolve_fusion(fusion, intent, configuration.views.query.fusion)?;
    options.ranking = resolve_ranking(ranking, intent, configuration.views.query.ranking)?;
    options.output_redaction = configuration.redaction.output();
    let filters = resolve(
        &query,
        intent,
        filters,
        use_shared,
        use_local,
        node(configuration),
    )?;
    let output = options.output_redaction;
    if intent == crate::views::exact::Intent::ExactMention {
        let literal = crate::views::exact::Literal::with_mode(&query, filters.match_mode)?;
        if use_shared || (!use_local && configuration.shared.is_some()) {
            return shared_exact(
                configuration,
                (!use_shared).then_some(database),
                &literal,
                query,
                (page, unit),
                &filters,
                output,
            )
            .await;
        }
        let database = database.to_path_buf();
        return crate::blocking(move || {
            local_exact(&database, &literal, query, (page, unit), &filters, output)
        })
        .await;
    }
    if use_shared || (!use_local && configuration.shared.is_some()) {
        shared_hybrid(
            configuration,
            (!use_shared).then_some(database),
            models,
            query,
            (options, page),
            &filters,
            use_shared,
        )
        .await
    } else {
        let database = database.to_path_buf();
        let models = models.to_path_buf();
        let embeddings = configuration.embeddings.clone();
        let build = configuration.views.build.clone();
        crate::blocking(move || {
            local_search(
                &database,
                &models,
                embeddings,
                query,
                (options, page),
                &filters,
                &build,
            )
        })
        .await
    }
}

async fn shared_hybrid(
    configuration: &ResolvedConfig,
    database: Option<&Path>,
    models: &Path,
    query: String,
    (options, page): (crate::views::config::Options, crate::search_results::Page),
    filters: &crate::search_filter::Scope,
    shared_only: bool,
) -> Result<SearchResponse, AppError> {
    let unit = options.unit;
    let shared::SearchOutcome {
        hits: results,
        retrieval,
        context_error,
    } = shared::search(
        required_shared(configuration)?,
        database.map(|database| (database, &configuration.views.build)),
        models,
        configuration.embeddings.as_ref(),
        &query,
        options,
        filters,
    )
    .await?;
    let shared_error = retrieval.into_error();
    let collected =
        crate::search_results::collect(results, unit, page, &query, options.output_redaction);
    Ok(SearchResponse {
        diagnostics: None,
        intent: crate::views::exact::Intent::RelatedDiscussion,
        realized_mode: search_mode(shared_error.is_some(), shared_only),
        unit,
        results: collected.results,
        next_offset: collected.next_offset,
        query,
        shared: shared_error.is_none(),
        shared_error,
        shared_context_error: context_error,
    })
}

/// Shared, or combined when `database` is local, model-free literal search.
async fn shared_exact(
    configuration: &ResolvedConfig,
    database: Option<&Path>,
    literal: &crate::views::exact::Literal,
    query: String,
    (page, unit): (
        crate::search_results::Page,
        crate::search_results::SearchUnit,
    ),
    filters: &crate::search_filter::Scope,
    output: crate::redaction::Policy,
) -> Result<SearchResponse, AppError> {
    let (hits, shared_error) = shared::search_exact(
        required_shared(configuration)?,
        database,
        &query,
        unit,
        filters,
        output,
    )
    .await?;
    let collected = crate::search_results::collect_exact(hits, unit, page, literal, output);
    Ok(SearchResponse {
        diagnostics: None,
        intent: crate::views::exact::Intent::ExactMention,
        realized_mode: exact_mode(shared_error.is_some(), database.is_none()),
        unit,
        results: collected.results,
        next_offset: collected.next_offset,
        query,
        shared: shared_error.is_none(),
        shared_error,
        shared_context_error: None,
    })
}

fn local_exact(
    database: &Path,
    literal: &crate::views::exact::Literal,
    query: String,
    (page, unit): (
        crate::search_results::Page,
        crate::search_results::SearchUnit,
    ),
    filters: &crate::search_filter::Scope,
    output: crate::redaction::Policy,
) -> Result<SearchResponse, AppError> {
    let hits = crate::observability::retrieval::measure_vec(
        crate::observability::retrieval::Stage::LocalExact,
        None,
        || {
            Storage::open_lexical(database)?
                .with_output_redaction(output)
                .search_exact(literal, unit.candidate_limit(), filters)
        },
    )?;
    let hits = crate::views::exact::rank(hits, unit);
    let collected = crate::search_results::collect_exact(hits, unit, page, literal, output);
    Ok(SearchResponse {
        diagnostics: None,
        intent: crate::views::exact::Intent::ExactMention,
        realized_mode: SearchMode::Lexical,
        unit,
        results: collected.results,
        next_offset: collected.next_offset,
        query,
        shared: false,
        shared_error: None,
        shared_context_error: None,
    })
}

#[tracing::instrument(name = "local_search", skip_all)]
fn local_search(
    database_path: &Path,
    models_dir: &Path,
    embeddings_config: Option<semantic::remote::Configuration>,
    query: String,
    (options, page): (crate::views::config::Options, crate::search_results::Page),
    filters: &crate::search_filter::Scope,
    build: &crate::views::config::Build,
) -> Result<SearchResponse, AppError> {
    let models = Models::new(models_dir.to_path_buf()).with_remote(embeddings_config);
    models.require_search(options.ranking)?;
    let generation = build.generation(&models.generation());
    let storage = Storage::open_search(database_path, &generation)?
        .with_output_redaction(options.output_redaction);
    storage.require_views_build(build, &models.generation())?;
    let mut backend = models
        .load_search(options.ranking)?
        .ok_or_else(AppError::models_missing)?;
    let results = semantic::hybrid_search_filtered(
        &generation,
        &storage,
        &mut backend,
        &query,
        options,
        (None, None),
        Some(filters),
    )?;
    let collected = crate::search_results::collect(
        results,
        options.unit,
        page,
        &query,
        options.output_redaction,
    );
    Ok(SearchResponse {
        diagnostics: None,
        intent: crate::views::exact::Intent::RelatedDiscussion,
        realized_mode: SearchMode::Hybrid,
        unit: options.unit,
        results: collected.results,
        next_offset: collected.next_offset,
        query,
        shared: false,
        shared_error: None,
        shared_context_error: None,
    })
}

/// This node's ID, or `local` without a shared configuration.
fn node(configuration: &ResolvedConfig) -> &str {
    configuration
        .shared
        .as_ref()
        .map_or("local", |c| c.node_id.as_str())
}

fn exact_mode(failed: bool, shared_only: bool) -> SearchMode {
    if failed {
        SearchMode::LocalLexical
    } else if shared_only {
        SearchMode::Lexical
    } else {
        SearchMode::MergedLexical
    }
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
