//! Hybrid candidates, reciprocal rank fusion, reranking, and exact vector scoring.
use super::embeddings::embedding_generation;
use super::models::Backend;
use super::remote;
use crate::AppError;
use crate::gold::config::{Fusion, Options};
use crate::observability::retrieval::{self as measurements, Guard, Stage};
use crate::search_results::{HitKey, hit_key};
use crate::storage::{SearchHit, SemanticChunks, Storage};
use std::collections::HashMap;

pub(super) struct QuantizedVector {
    pub(super) values: Vec<u8>,
    pub(super) norm: f32,
}

#[tracing::instrument(name = "hybrid_search", skip_all)]
pub(crate) fn hybrid_search_filtered(
    generation: &str,
    storage: &Storage,
    backend: &mut Backend,
    query: &str,
    options: Options,
    legacy: (Option<crate::provider::Provider>, Option<u32>),
    filters: Option<&crate::search_filter::Scope>,
) -> Result<Vec<SearchHit>, AppError> {
    let (provider, days) = legacy;
    let Options {
        limit,
        unit,
        fusion,
        ranking,
        shared_vector_search: _,
        output_redaction: _,
    } = options;
    let candidate_limit = unit.candidate_limit();
    let lexical = measurements::measure_vec(Stage::LocalLexical, None, || {
        tracing::info_span!("lexical_candidates")
            .in_scope(|| storage.search_filtered(query, candidate_limit, provider, days, filters))
    })?;
    let query_vector = backend
        .embed(&[query])?
        .pop()
        .ok_or_else(|| AppError::model("embedding model returned no query vector"))?;
    let query_vector = quantize_vector(&query_vector);
    let mut dense_stage = Guard::begin(Stage::LocalDense);
    let (semantic_hits, scanned) = dense_candidates(
        storage,
        generation,
        &query_vector,
        (provider, days, filters),
        candidate_limit,
    )?;
    dense_stage.complete(Some(scanned), Some(semantic_hits.len()));
    drop(dense_stage);
    if semantic_hits.is_empty() {
        return Ok(crate::search_results::diversify(lexical, unit));
    }
    let fused = crate::search_results::diversify(fuse(lexical, semantic_hits, unit, fusion), unit);
    if ranking == crate::gold::config::Ranking::Fusion {
        return Ok(crate::search_ranking::finish_fusion(fused, unit, limit));
    }
    let selection = crate::search_ranking::select(backend, query, fused, unit, limit)?;
    let documents = selection.hits[..selection.count]
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
        .collect::<Result<Vec<_>, AppError>>()?;
    crate::search_ranking::score(backend, query, selection, &documents, unit, limit)
}

/// Model-space identity shared by the CPU ONNX and Apple Core ML executions.
/// Unlike the local storage generation, batching and chunk layout do not affect it.
pub(crate) fn model_space(model: &str, dimensions: usize) -> String {
    if model == "sentence-transformers/all-MiniLM-L6-v2" && dimensions == 384 {
        SHARED_EMBEDDING_GENERATION.to_owned()
    } else {
        format!("remote:{model}:{dimensions}:i8-cosine:v1")
    }
}

pub(crate) fn configured_generation(config: Option<&remote::Configuration>) -> String {
    config.map_or_else(
        || embedding_generation().to_owned(),
        |c| model_space(&c.model, c.dimensions),
    )
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

// Shared query vectors contain bounded, quantized integers; conversion is exact.
#[allow(clippy::cast_possible_truncation)]
#[tracing::instrument(name = "vector_candidates", skip_all)]
pub(crate) fn local_candidates(
    storage: &Storage,
    generation: &str,
    query: &str,
    vector: &[f64],
    selection: (
        Option<crate::provider::Provider>,
        Option<u32>,
        crate::search_results::SearchUnit,
    ),
    filters: Option<&crate::search_filter::Scope>,
    fusion: Fusion,
) -> Result<(Vec<SearchHit>, Vec<String>), AppError> {
    let (provider, days, unit) = selection;
    let candidate_limit = unit.candidate_limit();
    let lexical = measurements::measure_vec(Stage::LocalLexical, None, || {
        storage.search_filtered(query, candidate_limit, provider, days, filters)
    })?;
    let vector = vector.iter().map(|v| *v as f32).collect::<Vec<_>>();
    let mut dense_stage = Guard::begin(Stage::LocalDense);
    let (semantic, scanned) = dense_candidates(
        storage,
        generation,
        &quantize_vector(&vector),
        (provider, days, filters),
        candidate_limit,
    )?;
    dense_stage.complete(Some(scanned), Some(semantic.len()));
    drop(dense_stage);
    let fused = fuse(lexical, semantic, unit, fusion);
    let ids = fused.iter().map(|hit| hit.id.as_str()).collect::<Vec<_>>();
    let documents = storage.search_documents(&ids)?;
    Ok((fused, documents))
}

/// Native and connector vectors scored against one query, best first.
/// Returns the hits and the number of vector chunks scanned.
fn dense_candidates(
    storage: &Storage,
    generation: &str,
    query: &QuantizedVector,
    (provider, days, filters): (
        Option<crate::provider::Provider>,
        Option<u32>,
        Option<&crate::search_filter::Scope>,
    ),
    limit: usize,
) -> Result<(Vec<SearchHit>, usize), AppError> {
    let native_chunks = tracing::info_span!("load_vectors")
        .in_scope(|| storage.semantic_chunks_filtered(generation, provider, days, filters))?;
    let native = tracing::info_span!("score_vectors")
        .in_scope(|| rank_quantized(query, &native_chunks, limit));
    let mut scanned = native_chunks.chunks.len();
    // A provider selection names native history only.
    let connector = if provider.is_none() {
        let chunks = connector_chunks(
            storage.connector_vectors(
                generation,
                crate::storage::cutoff_timestamp(days)?,
                filters,
            )?,
            query.values.len(),
        );
        scanned += chunks.chunks.len();
        rank_quantized(query, &chunks, limit)
    } else {
        Vec::new()
    };
    let mut ranked = native
        .into_iter()
        .map(|(id, score)| (false, id, score))
        .chain(connector.into_iter().map(|(id, score)| (true, id, score)))
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .2
            .total_cmp(&left.2)
            .then_with(|| (left.0, left.1).cmp(&(right.0, right.1)))
    });
    ranked.truncate(limit);
    let mut hits = Vec::with_capacity(ranked.len());
    for (connector, id, score) in ranked {
        let mut hit = if connector {
            storage.connector_hits(&[id])?
        } else {
            storage.search_hits(&[id])?
        }
        .pop()
        .ok_or_else(|| AppError::database_data("semantic index references a missing record"))?;
        hit.semantic_score = Some(score);
        hits.push(hit);
    }
    Ok((hits, scanned))
}

/// One chunk of connector vectors, shaped like native chunks for shared scoring.
fn connector_chunks(vectors: Vec<(i64, f32, Vec<u8>)>, dimensions: usize) -> SemanticChunks {
    let mut chunk = crate::storage::SemanticChunk {
        message_rowids: Vec::with_capacity(vectors.len()),
        values: Vec::with_capacity(vectors.len() * dimensions),
        norms: Vec::with_capacity(vectors.len()),
        eligible: Vec::with_capacity(vectors.len()),
        dimensions,
    };
    for (id, norm, vector) in vectors {
        // A vector of another width belongs to a stale generation; skip it.
        if vector.len() == dimensions {
            chunk.message_rowids.push(id);
            chunk.values.extend_from_slice(&vector);
            chunk.norms.push(norm);
            chunk.eligible.push(true);
        }
    }
    SemanticChunks {
        chunks: if chunk.message_rowids.is_empty() {
            Vec::new()
        } else {
            vec![chunk]
        },
        dimensions,
    }
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
            norm: 0.0,
        };
    }
    let scale = 127.0 / max_abs;
    let mut norm_squared = 0_i32;
    let values = vector
        .iter()
        .map(|value| {
            let quantized = quantize_component(*value, scale);
            let quantized_i32 = i32::from(quantized);
            norm_squared += quantized_i32 * quantized_i32;
            quantized.cast_unsigned()
        })
        .collect();
    QuantizedVector {
        values,
        norm: dot_to_f32(norm_squared).sqrt(),
    }
}

pub(super) fn quantized_cosine(left: &QuantizedVector, right: &[u8], right_norm: f32) -> f32 {
    if left.values.len() != right.len() || left.values.is_empty() {
        return 0.0;
    }
    let denominator = left.norm * right_norm;
    if denominator <= f32::EPSILON {
        return 0.0;
    }
    let product = left
        .values
        .iter()
        .zip(right)
        .map(|(&left, &right)| i32::from(left.cast_signed()) * i32::from(right.cast_signed()))
        .sum::<i32>();
    (dot_to_f32(product) / denominator).clamp(-1.0, 1.0)
}

fn quantize_component(value: f32, scale: f32) -> i8 {
    let rounded = (value * scale).round().clamp(-127.0, 127.0);
    debug_assert!((-127.0..=127.0).contains(&rounded));
    #[allow(clippy::cast_possible_truncation)]
    {
        rounded as i8
    }
}

fn dot_to_f32(value: i32) -> f32 {
    // Larger configured embedding spaces can exceed f32's exact-integer range.
    // Cosine scores are f32 approximations; rounding here must not panic merely
    // because a valid model has more dimensions than the original local model.
    #[allow(clippy::cast_precision_loss)]
    {
        value as f32
    }
}

pub(super) fn rank_quantized(
    query: &QuantizedVector,
    vectors: &SemanticChunks,
    limit: usize,
) -> Vec<(i64, f32)> {
    if limit == 0 || vectors.dimensions == 0 || query.values.len() != vectors.dimensions {
        return Vec::new();
    }
    let mut ranked = Vec::new();
    for chunk in &vectors.chunks {
        if chunk.dimensions != vectors.dimensions
            || chunk.norms.len() != chunk.message_rowids.len()
            || chunk.eligible.len() != chunk.message_rowids.len()
            || chunk.values.len() != chunk.message_rowids.len().saturating_mul(chunk.dimensions)
        {
            return Vec::new();
        }
        for (index, vector) in chunk.values.chunks_exact(chunk.dimensions).enumerate() {
            if chunk.eligible[index] {
                ranked.push((
                    chunk.message_rowids[index],
                    quantized_cosine(query, vector, chunk.norms[index]),
                ));
            }
        }
    }
    let compare = |left: &(i64, f32), right: &(i64, f32)| {
        right
            .1
            .total_cmp(&left.1)
            .then_with(|| left.0.cmp(&right.0))
    };
    if ranked.len() > limit {
        ranked.select_nth_unstable_by(limit, compare);
        ranked.truncate(limit);
    }
    ranked.sort_unstable_by(compare);
    ranked
}
