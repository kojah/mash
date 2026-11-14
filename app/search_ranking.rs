//! Built-in views: bounded thread selection by fused order, then one evidence reranking pass, across transports.
use crate::AppError;
use crate::observability::retrieval::{Guard, RerankerEngine, RerankerFallback, Stage};
use crate::search_results::SearchUnit;
use crate::semantic::LoadedBackend;
use crate::storage::SearchHit;
use crate::views::config::Fusion;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

#[cfg(test)]
mod tests;

/// Threads whose evidence is reranked in chat mode. Fixed so the final
/// order, and therefore every page of it, is independent of the page size.
const RERANK_THREADS: usize = 10;
/// Messages of each such thread that are reranked.
const THREAD_MESSAGES: usize = 6;

pub struct Selection {
    pub hits: Vec<SearchHit>,
    pub count: usize,
}

/// Combine the relevance order with recency, by reciprocal rank like
/// candidate fusion. Newer messages move up among comparably relevant ones,
/// while a strong old match still outranks a weak new one. Undated messages
/// get no recency credit.
pub fn favour_recent(hits: &mut [SearchHit], fusion: Fusion) {
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

/// Preserve fused ordering and all evidence; result collection pages it.
pub fn finish_fusion(mut hits: Vec<SearchHit>, fusion: Fusion) -> Vec<SearchHit> {
    favour_recent(&mut hits, fusion);
    hits
}

/// Choose the evidence to rerank. Chat searches take the best-fused threads
/// in fused order, each with its first messages; the contextual reranker then
/// scores that evidence once. Selecting needs no inference.
pub fn select(hits: Vec<SearchHit>, unit: SearchUnit) -> Selection {
    if matches!(unit, SearchUnit::Messages) {
        return Selection {
            count: hits.len().min(unit.rerank_limit()),
            hits,
        };
    }
    let mut taken = HashMap::new();
    let selected = hits
        .iter()
        .map(|hit| {
            let threads = taken.len();
            let Some(messages) = (match taken.entry((hit.provider, &hit.chat_id)) {
                Entry::Occupied(entry) => Some(entry.into_mut()),
                Entry::Vacant(entry) => (threads < RERANK_THREADS).then(|| entry.insert(0_usize)),
            }) else {
                return false;
            };
            *messages += 1;
            *messages <= THREAD_MESSAGES
        })
        .collect::<Vec<_>>();
    let (mut chosen, remaining): (Vec<_>, Vec<_>) = hits
        .into_iter()
        .enumerate()
        .partition(|(index, _)| selected[*index]);
    let count = chosen.len();
    chosen.extend(remaining);
    Selection {
        hits: chosen.into_iter().map(|(_, hit)| hit).collect(),
        count,
    }
}

/// Hits ranked without inference, or the evidence that still needs one
/// contextual reranking pass.
pub enum Ranked {
    Finished(Vec<SearchHit>),
    Rerank(Pending),
}

pub struct Pending {
    selection: Selection,
    query: String,
    /// Query-focused windows of the selected evidence, in selection order.
    windows: Vec<String>,
    unit: SearchUnit,
    fusion: Fusion,
}

impl Pending {
    pub fn new(
        query: &str,
        selection: Selection,
        documents: &[String],
        unit: SearchUnit,
        fusion: Fusion,
    ) -> Result<Self, AppError> {
        if documents.len() != selection.count {
            return Err(AppError::internal("incomplete reranking documents"));
        }
        Ok(Self {
            selection,
            query: query.to_owned(),
            windows: documents
                .iter()
                .map(|document| {
                    crate::search_results::rerank_document(document, query).into_owned()
                })
                .collect(),
            unit,
            fusion,
        })
    }

    /// Order the selection by one engine's scores, given in selection order.
    fn order(mut self, scores: &[f32]) -> Vec<SearchHit> {
        let (unit, fusion) = (self.unit, self.fusion);
        let selection = &mut self.selection;
        for (hit, score) in selection.hits[..selection.count].iter_mut().zip(scores) {
            hit.rerank_score = Some(*score);
        }
        selection.hits[..selection.count].sort_by(|a, b| {
            b.rerank_score
                .unwrap_or(f32::NEG_INFINITY)
                .total_cmp(&a.rerank_score.unwrap_or(f32::NEG_INFINITY))
                .then_with(|| b.fusion_score.total_cmp(&a.fusion_score))
                .then_with(|| a.node_id.cmp(&b.node_id))
                .then_with(|| a.id.cmp(&b.id))
        });
        favour_recent(&mut selection.hits[..selection.count], fusion);
        if matches!(unit, SearchUnit::Chats) {
            selection.hits[selection.count..].sort_by(|a, b| {
                b.fusion_score
                    .total_cmp(&a.fusion_score)
                    .then_with(|| a.node_id.cmp(&b.node_id))
                    .then_with(|| a.id.cmp(&b.id))
            });
        }
        self.selection.hits
    }
}

/// Finish a search: rerank its selected evidence once, on the configured
/// remote worker or the local CPU, and release the backend off the runtime.
pub async fn finish(backend: LoadedBackend, ranked: Ranked) -> Result<Vec<SearchHit>, AppError> {
    let pending = match ranked {
        Ranked::Rerank(pending) if !pending.windows.is_empty() => pending,
        Ranked::Rerank(pending) => {
            return crate::blocking(move || {
                drop(backend);
                Ok(pending.order(&[]))
            })
            .await;
        }
        Ranked::Finished(hits) => {
            // Returning a loaded backend to the cache can destroy the old model.
            return crate::blocking(move || {
                drop(backend);
                Ok(hits)
            })
            .await;
        }
    };
    let mut measured = Guard::begin(Stage::Rerank);
    let inputs = pending.windows.len();
    let attempt = match backend.remote_reranker() {
        None => None,
        Some(Err(reason)) => Some(Err(reason)),
        Some(Ok(client)) => {
            let refs = pending
                .windows
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            Some(client.rerank(&pending.query, &refs).await)
        }
    };
    let (hits, engine, fallback) = crate::blocking(move || {
        let mut backend = backend;
        let chosen = choose(attempt, || {
            let refs = pending
                .windows
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            backend.rerank_cpu(&pending.query, &refs)
        });
        drop(backend);
        let (scores, engine, fallback) = chosen?;
        Ok((pending.order(&scores), engine, fallback))
    })
    .await?;
    measured.complete(Some(inputs), Some(inputs));
    drop(measured);
    crate::observability::retrieval::record_reranker(engine, fallback);
    Ok(hits)
}

/// Use every remote score or none: a failed remote attempt reranks the whole
/// pass on the CPU, so scores from different engines are never compared.
fn choose(
    attempt: Option<Result<crate::semantic::remote_reranker::Scores, RerankerFallback>>,
    cpu: impl FnOnce() -> Result<Vec<f32>, AppError>,
) -> Result<(Vec<f32>, RerankerEngine, Option<RerankerFallback>), AppError> {
    match attempt {
        Some(Ok(remote)) => Ok((remote.scores, RerankerEngine::Remote, None)),
        Some(Err(reason)) => {
            tracing::warn!(?reason, "remote reranking unavailable; reranking on CPU");
            Ok((cpu()?, RerankerEngine::CpuFallback, Some(reason)))
        }
        None => Ok((cpu()?, RerankerEngine::Cpu, None)),
    }
}
