//! Fixed, payload-free inclusive span lifetimes for one traced worker request.
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::{Id, Subscriber, span::Attributes};
use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Stage {
    Search,
    LocalSearch,
    LoadSearchModels,
    Embed,
    HybridSearch,
    LexicalCandidates,
    LoadVectors,
    ScoreVectors,
    CombinedSearch,
    PrepareSearch,
    RetrieveAndRank,
    LocalCandidates,
    SharedHttp,
    Rerank,
    RerankInference,
}

impl Stage {
    const ALL: [Self; 15] = [
        Self::Search,
        Self::LocalSearch,
        Self::LoadSearchModels,
        Self::Embed,
        Self::HybridSearch,
        Self::LexicalCandidates,
        Self::LoadVectors,
        Self::ScoreVectors,
        Self::CombinedSearch,
        Self::PrepareSearch,
        Self::RetrieveAndRank,
        Self::LocalCandidates,
        Self::SharedHttp,
        Self::Rerank,
        Self::RerankInference,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::LocalSearch => "local_search",
            Self::LoadSearchModels => "load_search_models",
            Self::Embed => "embed",
            Self::HybridSearch => "hybrid_search",
            Self::LexicalCandidates => "lexical_candidates",
            Self::LoadVectors => "load_vectors",
            Self::ScoreVectors => "score_vectors",
            Self::CombinedSearch => "combined_search",
            Self::PrepareSearch => "prepare_search",
            Self::RetrieveAndRank => "retrieve_and_rank",
            Self::LocalCandidates => "local_candidates",
            Self::SharedHttp => "shared_http_request",
            Self::Rerank => "rerank",
            Self::RerankInference => "rerank_inference",
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Timing {
    pub(crate) stage: Stage,
    pub(crate) calls: u64,
    pub(crate) total_us: u64,
    pub(crate) max_us: u64,
}

pub(crate) fn valid(timings: &[Timing]) -> bool {
    timings.len() <= Stage::ALL.len()
        && timings.iter().enumerate().all(|(index, timing)| {
            timing.calls > 0
                && timing.total_us >= timing.max_us
                && timings[..index]
                    .iter()
                    .all(|previous| previous.stage != timing.stage)
        })
}

#[derive(Clone, Default)]
pub(crate) struct Capture(Arc<Mutex<[Option<Timing>; 15]>>);

struct Started {
    index: usize,
    time: Instant,
}

impl Capture {
    pub(crate) fn dispatch(&self) -> tracing::Dispatch {
        use tracing_subscriber::layer::SubscriberExt;
        tracing::Dispatch::new(
            tracing_subscriber::registry().with(
                self.clone().with_filter(
                    tracing_subscriber::filter::Targets::new()
                        .with_target("mash", tracing::Level::INFO),
                ),
            ),
        )
    }

    pub(crate) fn finish(&self) -> Vec<Timing> {
        self.0.lock().map_or_else(
            |_| Vec::new(),
            |timings| timings.iter().flatten().cloned().collect(),
        )
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for Capture {
    fn on_new_span(&self, attributes: &Attributes<'_>, id: &Id, context: Context<'_, S>) {
        let Some(index) = Stage::ALL
            .iter()
            .position(|stage| stage.name() == attributes.metadata().name())
        else {
            return;
        };
        if let Some(span) = context.span(id) {
            span.extensions_mut().insert(Started {
                index,
                time: Instant::now(),
            });
        }
    }

    fn on_close(&self, id: Id, context: Context<'_, S>) {
        let Some(span) = context.span(&id) else {
            return;
        };
        let extensions = span.extensions();
        let Some(started) = extensions.get::<Started>() else {
            return;
        };
        let elapsed = u64::try_from(started.time.elapsed().as_micros()).unwrap_or(u64::MAX);
        if let Ok(mut timings) = self.0.lock() {
            let timing = timings[started.index].get_or_insert(Timing {
                stage: Stage::ALL[started.index],
                calls: 0,
                total_us: 0,
                max_us: 0,
            });
            timing.calls = timing.calls.saturating_add(1);
            timing.total_us = timing.total_us.saturating_add(elapsed);
            timing.max_us = timing.max_us.max(elapsed);
        }
    }
}

#[cfg(test)]
mod tests;
