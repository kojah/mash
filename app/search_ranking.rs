//! Transport-independent, bounded thread selection followed by evidence ranking.
use crate::AppError;
use crate::search_results::{self, SearchUnit};
use crate::semantic::Backend;
use crate::storage::SearchHit;
use std::collections::HashMap;

#[cfg(test)]
mod tests;

pub(crate) struct Selection {
    pub(crate) hits: Vec<SearchHit>,
    pub(crate) count: usize,
}

pub(crate) fn select(
    backend: &mut Backend,
    query: &str,
    hits: Vec<SearchHit>,
    unit: SearchUnit,
    limit: usize,
) -> Result<Selection, AppError> {
    if matches!(unit, SearchUnit::Messages) {
        return Ok(Selection {
            count: hits.len().min(unit.rerank_limit()),
            hits,
        });
    }
    let mut positions = HashMap::new();
    let mut groups = Vec::<Vec<usize>>::new();
    for (index, hit) in hits.iter().enumerate() {
        let next = groups.len();
        let slot = *positions
            .entry((hit.provider, &hit.conversation_id))
            .or_insert_with(|| {
                groups.push(Vec::new());
                next
            });
        groups[slot].push(index);
    }
    let documents = groups
        .iter()
        .take(20)
        .map(|group| {
            group
                .iter()
                .take(6)
                .map(|&index| {
                    // Six short, query-focused anchors fit the model's input budget.
                    search_results::evidence_window(&hits[index].content, query, 160)
                })
                .collect::<Vec<_>>()
                .join("\nThread message: ")
        })
        .collect::<Vec<_>>();
    let refs = documents.iter().map(String::as_str).collect::<Vec<_>>();
    let mut ranked = backend.rerank_search(query, &refs)?;
    ranked.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.index.cmp(&b.index))
    });
    let mut selected = vec![false; hits.len()];
    for thread in ranked.into_iter().take(limit.clamp(5, 10)) {
        if let Some(group) = groups.get(thread.index) {
            for &index in group.iter().take(6) {
                selected[index] = true;
            }
        }
    }
    let (mut chosen, remaining): (Vec<_>, Vec<_>) = hits
        .into_iter()
        .enumerate()
        .partition(|(index, _)| selected[*index]);
    let count = chosen.len();
    chosen.extend(remaining);
    Ok(Selection {
        hits: chosen.into_iter().map(|(_, hit)| hit).collect(),
        count,
    })
}

pub(crate) fn score(
    backend: &mut Backend,
    query: &str,
    mut selection: Selection,
    documents: &[String],
    unit: SearchUnit,
    limit: usize,
) -> Result<Vec<SearchHit>, AppError> {
    if documents.len() != selection.count {
        return Err(AppError::internal("incomplete reranking documents"));
    }
    let refs = documents.iter().map(String::as_str).collect::<Vec<_>>();
    for result in backend.rerank_search(query, &refs)? {
        if let Some(hit) = selection.hits[..selection.count].get_mut(result.index) {
            hit.rerank_score = Some(result.score);
        }
    }
    selection.hits[..selection.count].sort_by(|a, b| {
        b.rerank_score
            .unwrap_or(f32::NEG_INFINITY)
            .total_cmp(&a.rerank_score.unwrap_or(f32::NEG_INFINITY))
            .then_with(|| b.fusion_score.total_cmp(&a.fusion_score))
            .then_with(|| a.node_id.cmp(&b.node_id))
            .then_with(|| a.id.cmp(&b.id))
    });
    if matches!(unit, SearchUnit::Messages) {
        selection.hits.truncate(limit);
    } else {
        selection.hits[selection.count..].sort_by(|a, b| {
            b.fusion_score
                .total_cmp(&a.fusion_score)
                .then_with(|| a.node_id.cmp(&b.node_id))
                .then_with(|| a.id.cmp(&b.id))
        });
    }
    Ok(selection.hits)
}
