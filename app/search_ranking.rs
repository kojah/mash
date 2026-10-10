//! Gold: bounded thread selection followed by evidence ranking across transports.
use crate::AppError;
use crate::gold::config::Fusion;
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

/// Combine the relevance order with recency, by reciprocal rank like
/// candidate fusion. Newer messages move up among comparably relevant ones,
/// while a strong old match still outranks a weak new one. Undated messages
/// get no recency credit.
pub(crate) fn favour_recent(hits: &mut [SearchHit], fusion: Fusion) {
    if fusion.recency_weight == 0 || hits.len() < 2 {
        return;
    }
    let mut by_age: Vec<usize> = (0..hits.len()).collect();
    // Newest first; undated last, keeping relevance order among equals.
    by_age.sort_by_key(|&index| std::cmp::Reverse(hits[index].created_at));
    let mut recency = vec![None; hits.len()];
    for (rank, &index) in by_age.iter().enumerate() {
        recency[index] = hits[index].created_at.map(|_| rank);
    }
    let weight = f32::from(fusion.recency_weight) / 100.0;
    let reciprocal = |rank: usize| {
        let rank = u16::try_from(rank).unwrap_or(u16::MAX);
        1.0 / (f32::from(fusion.rank_constant) + f32::from(rank) + 1.0)
    };
    let mut order: Vec<(f32, usize)> = (0..hits.len())
        .map(|index| {
            let recent = recency[index].map_or(0.0, |rank| weight * reciprocal(rank));
            (reciprocal(index) + recent, index)
        })
        .collect();
    order.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    // Apply the permutation in place: position i takes original order[i].
    let source: Vec<usize> = order.into_iter().map(|(_, index)| index).collect();
    for position in 0..source.len() {
        let mut from = source[position];
        while from < position {
            from = source[from];
        }
        hits.swap(position, from);
    }
}

/// Preserve fused ordering and all thread evidence; cap message results only.
pub(crate) fn finish_fusion(
    mut hits: Vec<SearchHit>,
    unit: SearchUnit,
    limit: usize,
    fusion: Fusion,
) -> Vec<SearchHit> {
    favour_recent(&mut hits, fusion);
    if matches!(unit, SearchUnit::Messages) {
        hits.truncate(limit);
    }
    hits
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
            .entry((&hit.source, &hit.conversation_id))
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
    fusion: Fusion,
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
    // Before truncation, so a newer message just past the limit can enter.
    favour_recent(&mut selection.hits[..selection.count], fusion);
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
