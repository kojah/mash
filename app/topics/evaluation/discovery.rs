//! Fit only canonical training representatives; keep assignment/reporting separate.
use super::{Mode, Thread, anchors, held_out, routing};
use crate::AppError;
use crate::topics::{
    Settings,
    birch::{Routing, Tree},
};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

pub(super) struct Fit {
    pub(super) tree: Tree,
    pub(super) routing: Routing,
    pub(super) samples: usize,
    pub(super) duplicates: usize,
    pub(super) elapsed_ms: f64,
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
    let mut tree = Tree::new(Settings {
        dimensions: 384,
        radius,
        branching: 32,
        max_topics: 8192,
        routing,
    })?;
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
    Ok(Fit {
        tree,
        routing,
        samples,
        duplicates,
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
    })
}
