//! Explicit offline label experiment with bounded, generation-scoped vector reuse.
use super::labels;
use crate::AppError;
use crate::semantic::{EmbeddingPool, Models};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

#[derive(Serialize)]
pub(super) struct Summary {
    inference: Inference,
    scoring: labels::ranking::Weights,
    representation_policy: super::representatives::Policy,
    representative_topics: usize,
    unique_candidates: usize,
    newly_embedded: usize,
    cached_candidates: usize,
    labeled_topics: usize,
    elapsed_ms: f64,
}

pub(super) struct Labeler {
    pool: EmbeddingPool,
    cache: BTreeMap<String, Vec<f64>>,
    inference: Inference,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Inference {
    Batched,
    Singleton,
}

#[derive(Serialize)]
pub(super) struct Name {
    pub(super) terms: Vec<labels::Term>,
    pub(super) semantic_terms: Option<Vec<labels::ranking::SemanticTerm>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) label_representation: Option<super::representatives::Selection>,
    #[serde(skip)]
    pub(super) candidates: Vec<labels::Term>,
    #[serde(skip)]
    pub(super) center: Vec<f64>,
}
impl Name {
    pub(super) fn new(center: Vec<f64>, candidates: Vec<labels::Term>) -> Self {
        Self {
            terms: candidates.iter().take(8).cloned().collect(),
            semantic_terms: None,
            label_representation: None,
            candidates,
            center,
        }
    }

    fn scoring_center(&self) -> &[f64] {
        self.label_representation
            .as_ref()
            .map_or(&self.center, |selection| &selection.center)
    }
}
impl Labeler {
    pub(super) fn inspect(
        &self,
        name: &Name,
    ) -> Result<Vec<labels::ranking::SemanticTerm>, AppError> {
        labels::ranking::inspect(name.scoring_center(), &name.candidates, &self.cache)
    }

    pub(super) fn project_vectors(&mut self, texts: &[&str]) -> Result<Vec<Vec<f64>>, AppError> {
        if texts.len() > 64 {
            return Err(AppError::usage("topic projection batch cap exceeded"));
        }
        self.pool.shared_vectors(texts)
    }

    pub(super) fn load(generation: &str) -> Result<Option<Self>, AppError> {
        if std::env::var_os("MASH_TEST_TOPIC_SEMANTIC_LABELS").is_none() {
            return Ok(None);
        }
        let root = std::env::var_os("MASH_TEST_MODELS_DIR").ok_or_else(|| {
            AppError::usage("explicit installed topic label model directory required")
        })?;
        let models = Models::new(root.into());
        if models.generation() != generation {
            return Err(AppError::model("topic label embedding generation mismatch"));
        }
        models.require_indexer()?;
        let pool = models
            .load_indexer()?
            .ok_or_else(|| AppError::model("topic label models are not installed"))?;
        Ok(Some(Self {
            pool,
            cache: BTreeMap::new(),
            inference: if std::env::var_os("MASH_TEST_TOPIC_SINGLETON_LABELS").is_some() {
                Inference::Singleton
            } else {
                Inference::Batched
            },
        }))
    }

    pub(super) fn apply(&mut self, names: &mut [&mut Name]) -> Result<Summary, AppError> {
        let started = Instant::now();
        let unique: BTreeSet<_> = names
            .iter()
            .flat_map(|c| c.candidates.iter().map(|t| t.text.clone()))
            .collect();
        let missing: Vec<_> = unique
            .iter()
            .filter(|term| !self.cache.contains_key(*term))
            .cloned()
            .collect();
        if self.cache.len() + missing.len() > 16384 {
            return Err(AppError::usage(
                "experimental topic label vocabulary cap exceeded",
            ));
        }
        for batch in missing.chunks(64) {
            let texts: Vec<_> = batch.iter().map(String::as_str).collect();
            let vectors = match self.inference {
                Inference::Batched => self.pool.shared_vectors(&texts),
                Inference::Singleton => self.pool.singleton_label_vectors(&texts),
            }?;
            if vectors.len() != batch.len() {
                return Err(AppError::model("unexpected topic label embedding count"));
            }
            for (text, vector) in batch.iter().zip(vectors) {
                self.cache.insert(text.clone(), vector);
            }
        }
        let mut labeled = 0;
        let representative_topics = names
            .iter()
            .filter(|name| name.label_representation.is_some())
            .count();
        for cluster in names {
            let terms = labels::ranking::rank(
                cluster.scoring_center(),
                &cluster.candidates,
                &self.cache,
                8,
            )?;
            labeled += usize::from(!terms.is_empty());
            cluster.semantic_terms = Some(terms);
        }
        Ok(Summary {
            inference: self.inference,
            scoring: labels::ranking::weights(labels::shortlist::policy()),
            representation_policy: super::representatives::policy(),
            representative_topics,
            unique_candidates: unique.len(),
            newly_embedded: missing.len(),
            cached_candidates: self.cache.len(),
            labeled_topics: labeled,
            elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
        })
    }
}
