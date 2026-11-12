//! Hybrid candidates, reciprocal rank fusion, reranking, and exact vector scoring.
use super::models::Backend;
use crate::AppError;
use crate::observability::retrieval::{Guard, Stage};
use crate::search_results::{HitKey, hit_key};
use crate::storage::{SearchHit, Storage};
use crate::views::config::{Fusion, Options};
use std::collections::HashMap;

pub(super) struct QuantizedVector {
    pub(super) values: Vec<u8>,
}

pub(crate) fn finish_hybrid(
    storage: &Storage,
    backend: &mut Backend,
    query: &str,
    options: Options,
    lexical: Vec<SearchHit>,
    semantic_hits: Vec<SearchHit>,
) -> Result<Vec<SearchHit>, AppError> {
    let Options {
        unit,
        fusion,
        ranking,
        ..
    } = options;
    if semantic_hits.is_empty() {
        let mut lexical = lexical;
        crate::search_ranking::favour_recent(&mut lexical, fusion);
        return Ok(crate::search_results::diversify(lexical, unit));
    }
    let fused = crate::search_results::diversify(fuse(lexical, semantic_hits, unit, fusion), unit);
    if ranking == crate::views::config::Ranking::Fusion {
        return Ok(crate::search_ranking::finish_fusion(fused, fusion));
    }
    let selection = crate::search_ranking::select(fused, unit);
    let documents =
        tracing::info_span!("rerank_context", hits = selection.count).in_scope(|| {
            selection.hits[..selection.count]
                .iter()
                .map(|hit| {
                    let context = storage.view_content(
                        &hit.id,
                        2,
                        crate::storage::MessageContent::SearchProjection,
                    )?;
                    Ok(crate::search_results::contextual_document(
                        hit, query, &context,
                    ))
                })
                .collect::<Result<Vec<_>, AppError>>()
        })?;
    crate::search_ranking::score(backend, query, selection, &documents, unit, fusion)
}

pub(crate) const SHARED_EMBEDDING_GENERATION: &str = "all-MiniLM-L6-v2:384:i8-cosine:v1";

pub(crate) fn shared_query_vector(
    backend: &mut Backend,
    query: &str,
) -> Result<Vec<f64>, AppError> {
    let vector = backend
        .embed(&[query])?
        .pop()
        .ok_or_else(|| AppError::model("embedding model returned no query vector"))?;
    Ok(quantize_vector(&vector)
        .values
        .into_iter()
        .map(|value| f64::from(value.cast_signed()))
        .collect())
}

pub(crate) fn fuse(
    lexical: Vec<SearchHit>,
    semantic: Vec<SearchHit>,
    unit: crate::search_results::SearchUnit,
    fusion: Fusion,
) -> Vec<SearchHit> {
    let mut measured = Guard::begin(Stage::Fusion);
    let inputs = lexical.len().saturating_add(semantic.len());
    let mut hits = HashMap::new();
    let mut scores = HashMap::<HitKey, f32>::new();
    for (rank, hit) in crate::search_results::deduplicate(lexical, unit)
        .into_iter()
        .enumerate()
    {
        let id = hit_key(&hit, unit);
        scores.insert(
            id.clone(),
            reciprocal_rank(rank, fusion.rank_constant) * f32::from(fusion.lexical_weight),
        );
        hits.insert(id, hit);
    }
    for (rank, hit) in crate::search_results::deduplicate(semantic, unit)
        .into_iter()
        .enumerate()
    {
        let id = hit_key(&hit, unit);
        *scores.entry(id.clone()).or_default() +=
            reciprocal_rank(rank, fusion.rank_constant) * f32::from(fusion.semantic_weight);
        if let Some(existing) = hits.get_mut(&id) {
            existing.semantic_score = hit.semantic_score;
            crate::search_results::merge_copies(existing, hit);
        } else {
            hits.insert(id, hit);
        }
    }
    let mut fused: Vec<SearchHit> = hits
        .into_iter()
        .map(|(id, mut hit)| {
            hit.fusion_score = scores.get(&id).copied().unwrap_or_default();
            hit
        })
        .collect();
    fused.sort_by(|left, right| {
        right
            .fusion_score
            .total_cmp(&left.fusion_score)
            .then_with(|| left.id.cmp(&right.id))
    });
    measured.complete(Some(inputs), Some(fused.len()));
    fused
}

fn reciprocal_rank(zero_based_rank: usize, constant: u16) -> f32 {
    let rank = u16::try_from(zero_based_rank).unwrap_or(u16::MAX);
    1.0 / (f32::from(constant) + f32::from(rank) + 1.0)
}

pub(super) fn quantize_vector(vector: &[f32]) -> QuantizedVector {
    let max_abs = vector
        .iter()
        .map(|value| value.abs())
        .fold(0.0_f32, f32::max);
    if max_abs <= f32::EPSILON {
        return QuantizedVector {
            values: vec![0; vector.len()],
        };
    }
    let scale = 127.0 / max_abs;
    let values = vector
        .iter()
        .map(|value| {
            let quantized = quantize_component(*value, scale);
            quantized.cast_unsigned()
        })
        .collect();
    QuantizedVector { values }
}

fn quantize_component(value: f32, scale: f32) -> i8 {
    let rounded = (value * scale).round().clamp(-127.0, 127.0);
    debug_assert!((-127.0..=127.0).contains(&rounded));
    #[allow(clippy::cast_possible_truncation)]
    {
        rounded as i8
    }
}
