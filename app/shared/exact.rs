//! Model-free concurrent local/shared literal retrieval.
use super::{
    client::{Endpoint, post},
    protocol,
};
use crate::{
    AppError,
    config::SharedConfig,
    search_filter::Scope,
    search_results::SearchUnit,
    storage::SearchHit,
    views::exact::{self, Literal},
};
use std::path::Path;

#[cfg(test)]
mod tests;

pub(crate) async fn search(
    config: &SharedConfig,
    database: Option<(&Path, &crate::config::ResolvedConfig)>,
    query: &str,
    unit: SearchUnit,
    filters: &Scope,
    output_redaction: crate::redaction::Policy,
) -> Result<(Vec<SearchHit>, Option<String>), AppError> {
    let literal = Literal::with_mode(query, filters.match_mode)?;
    filters.filter.validate()?;
    let exclude_self = database.is_some();
    let local = async {
        let Some((path, configuration)) = database else {
            return Ok(vec![]);
        };
        let local = crate::views::local::Local::open_exact(path, configuration).await?;
        let mut hits = local
            .candidates(protocol::Search {
                match_mode: filters.match_mode,
                intent: exact::Intent::ExactMention,
                vector_policy: None,
                query: query.into(),
                unit,
                vector: vec![],
                generation: local.scope.generation.clone(),
                provider: None,
                days: None,
                filter: Some(filters.filter.clone()),
                exclude_self: false,
            })
            .await?
            .lexical;
        for hit in &mut hits {
            hit.node_id = None;
        }
        Ok(hits)
    };
    let remote = async {
        let mut measured = crate::observability::retrieval::Guard::begin(
            crate::observability::retrieval::Stage::SharedCandidates,
        );
        let candidates = post::<protocol::Candidates>(
            config,
            Endpoint::Search,
            &protocol::Search {
                match_mode: filters.match_mode,
                intent: exact::Intent::ExactMention,
                vector_policy: None,
                query: query.to_owned(),
                unit,
                vector: vec![],
                generation: String::new(),
                provider: None,
                days: None,
                filter: Some(filters.filter.clone()),
                exclude_self,
            },
        )
        .await?;
        if !candidates.semantic.is_empty()
            || !candidates.documents.is_empty()
            || candidates.lexical.len() > unit.candidate_limit()
            || candidates.lexical.iter().any(|hit| {
                hit.node_id
                    .as_ref()
                    .is_none_or(|n| n.is_empty() || (exclude_self && n == &config.node_id))
                    || hit.semantic_score.is_some()
                    || hit.rerank_score.is_some()
                    || hit.lexical_score.is_none_or(|s| !s.is_finite())
            })
        {
            return Err(AppError::shared("invalid exact-mention response"));
        }
        let mut hits = crate::search_results::eligible(candidates.lexical);
        if hits.iter().any(|hit| !literal.matches(&hit.content)) {
            return Err(AppError::shared(
                "shared exact-mention response contains a nonliteral match",
            ));
        }
        for hit in &mut hits {
            crate::views::matching::annotate(hit, &literal, output_redaction);
        }
        measured.complete(None, Some(hits.len()));
        Ok(hits)
    };
    let (mut local, remote) = tokio::try_join!(local, async { Ok::<_, AppError>(remote.await) })?;
    let error = match remote {
        Ok(remote) => {
            local.extend(remote);
            None
        }
        Err(error) if exclude_self => Some(error.error.message),
        Err(error) => return Err(error),
    };
    Ok((exact::rank(local, unit), error))
}
