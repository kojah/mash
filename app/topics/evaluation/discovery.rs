//! Fit only canonical training representatives; keep assignment/reporting separate.
use super::{Mode, Thread, anchors, held_out, routing};
use crate::AppError;
use crate::topics::{
    Settings,
    birch::{Routing, Tree},
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Algorithm {
    Birch,
    Grinch,
}

pub(super) fn algorithm() -> Algorithm {
    if std::env::var_os("MASH_TEST_TOPIC_GRINCH").is_some() {
        Algorithm::Grinch
    } else {
        Algorithm::Birch
    }
}

enum Model {
    Birch(Tree),
    Grinch(crate::topics::grinch::Tree),
}
impl Model {
    fn insert(&mut self, vector: &[f64]) -> Result<(), AppError> {
        match self {
            Self::Birch(tree) => tree.insert(vector).map(|_| ()),
            Self::Grinch(tree) => tree.insert(vector),
        }
    }
}

pub(super) struct Fit {
    pub(super) leaves: Vec<(crate::topics::TopicId, Vec<f64>, u64)>,
    pub(super) depth: usize,
    pub(super) grinch: Option<crate::topics::grinch::topology::Snapshot>,
    pub(super) algorithm: Algorithm,
    pub(super) routing: Routing,
    pub(super) samples: usize,
    pub(super) duplicates: usize,
    pub(super) elapsed_ms: f64,
    pub(super) extraction_ms: f64,
    pub(super) input_fingerprint: String,
}

pub(super) fn fit(
    threads: &BTreeMap<String, Thread>,
    mode: Mode,
    radius: f64,
    reversed: bool,
    holdout: bool,
) -> Result<Fit, AppError> {
    let mut order: Vec<_> = threads.iter().collect();
    if reversed {
        order.reverse();
    }
    let routing = routing();
    Settings {
        dimensions: 384,
        radius,
        branching: 32,
        max_topics: 8192,
        routing,
    }
    .validate()?;
    let algorithm = algorithm();
    let mut tree = match algorithm {
        Algorithm::Birch => Model::Birch(Tree::new(Settings {
            dimensions: 384,
            radius,
            branching: 32,
            max_topics: 8192,
            routing,
        })?),
        Algorithm::Grinch => Model::Grinch(crate::topics::grinch::Tree::new(
            crate::topics::grinch::Settings {
                dimensions: 384,
                maximum_points: 8192,
                maximum_scores: 100_000_000,
            },
        )?),
    };
    let mut seen = BTreeSet::new();
    let mut samples = 0;
    let mut duplicates = 0;
    let start = Instant::now();
    for (_, thread) in &order {
        if holdout && held_out(thread) {
            continue;
        }
        for segment in anchors::discovery_samples(thread, mode)? {
            let vector = segment.vector;
            let mut hash = blake3::Hasher::new();
            for value in &vector {
                hash.update(&value.to_le_bytes());
            }
            if !seen.insert(*hash.finalize().as_bytes()) {
                duplicates += 1;
                continue;
            }
            tree.insert(&vector)?;
            samples += 1;
        }
    }
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let (leaves, depth, grinch) = match tree {
        Model::Birch(tree) => (tree.leaves(), tree.depth(), None),
        Model::Grinch(tree) => {
            let snapshot = tree.snapshot()?;
            (tree.cut(radius)?, snapshot.depth, Some(snapshot))
        }
    };
    let extraction_ms = start.elapsed().as_secs_f64() * 1000.0;
    let mut hash = blake3::Hasher::new();
    for fingerprint in &seen {
        hash.update(fingerprint);
    }
    let input_fingerprint = hash.finalize().to_hex().to_string();
    Ok(Fit {
        leaves,
        depth,
        grinch,
        algorithm,
        routing,
        samples,
        duplicates,
        elapsed_ms,
        extraction_ms,
        input_fingerprint,
    })
}
