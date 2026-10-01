//! Explicit offline label experiment with bounded, generation-scoped vector reuse.
use super::{Cluster, labels};
use crate::AppError;
use crate::semantic::{EmbeddingPool, Models};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

#[derive(Serialize)]
pub(super) struct Summary {
    unique_candidates: usize,
    newly_embedded: usize,
    cached_candidates: usize,
    labeled_topics: usize,
    elapsed_ms: f64,
}

pub(super) struct Labeler {
    pool: EmbeddingPool,
    cache: BTreeMap<String, Vec<f64>>,
}
impl Labeler {
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
        }))
    }

    pub(super) fn apply(&mut self, clusters: &mut [Cluster]) -> Result<Summary, AppError> {
        let started = Instant::now();
        let unique: BTreeSet<_> = clusters
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
            let vectors = self.pool.shared_vectors(&texts)?;
            if vectors.len() != batch.len() {
                return Err(AppError::model("unexpected topic label embedding count"));
            }
            for (text, vector) in batch.iter().zip(vectors) {
                self.cache.insert(text.clone(), vector);
            }
        }
        let mut labeled = 0;
        for cluster in clusters {
            let terms =
                labels::ranking::rank(&cluster.center, &cluster.candidates, &self.cache, 8)?;
            labeled += usize::from(!terms.is_empty());
            cluster.semantic_terms = Some(terms);
        }
        Ok(Summary {
            unique_candidates: unique.len(),
            newly_embedded: missing.len(),
            cached_candidates: self.cache.len(),
            labeled_topics: labeled,
            elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
        })
    }
}
