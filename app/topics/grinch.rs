//! Offline exact cosine-linkage GRINCH experiment. See Monath et al., KDD 2019.
//! Insertion rotation follows the author's pre-insertion ancestor climb. Grafts
//! require mutual preference and invoke restructuring (paper Algorithms 1–2).
#[cfg(test)]
mod tests;
pub(super) mod topology;

use super::{AppError, TopicId, normalize};
use serde::Serialize;
use topology::Node;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
struct Id(usize);

#[derive(Clone, Copy)]
pub(super) struct Settings {
    pub(super) dimensions: usize,
    pub(super) maximum_points: usize,
    pub(super) maximum_scores: u64,
}

#[derive(Clone, Copy, Default, Serialize)]
pub(super) struct Statistics {
    scores: u64,
    rotations: u64,
    grafts: u64,
    restructures: u64,
    swaps: u64,
}

pub(super) struct Tree {
    settings: Settings,
    nodes: Vec<Node>,
    leaves: Vec<Id>,
    root: Option<Id>,
    statistics: Statistics,
    failed: bool,
}

impl Tree {
    pub(super) fn new(settings: Settings) -> Result<Self, AppError> {
        if !(1..=2048).contains(&settings.dimensions)
            || !(1..=8192).contains(&settings.maximum_points)
            || !(1..=100_000_000).contains(&settings.maximum_scores)
        {
            return Err(AppError::usage("invalid experimental GRINCH settings"));
        }
        Ok(Self {
            settings,
            nodes: Vec::new(),
            leaves: Vec::new(),
            root: None,
            statistics: Statistics::default(),
            failed: false,
        })
    }

    fn score(&mut self, a: Id, b: Id) -> Result<f64, AppError> {
        if self.statistics.scores >= self.settings.maximum_scores {
            return Err(AppError::usage("experimental GRINCH score budget exceeded"));
        }
        self.statistics.scores += 1;
        Ok(self
            .node(a)
            .center
            .iter()
            .zip(&self.node(b).center)
            .map(|(x, y)| x * y)
            .sum::<f64>()
            .clamp(-1.0, 1.0))
    }

    fn nearest(&mut self, query: Id, exclude: Option<Id>) -> Result<Option<Id>, AppError> {
        // Exact leaf search; tie-breaking by insertion identity is explicit.
        let mut best = None;
        let mut maximum = f64::NEG_INFINITY;
        for index in 0..self.leaves.len() {
            let leaf = self.leaves[index];
            if exclude.is_some_and(|root| self.contains(root, leaf)) {
                continue;
            }
            let score = self.score(query, leaf)?;
            if score > maximum {
                maximum = score;
                best = Some(leaf);
            }
        }
        Ok(best)
    }

    pub(super) fn insert(&mut self, vector: &[f64]) -> Result<(), AppError> {
        if self.failed {
            return Err(AppError::usage("experimental GRINCH run already failed"));
        }
        if self.leaves.len() == self.settings.maximum_points {
            return Err(AppError::usage("experimental GRINCH point cap exceeded"));
        }
        // Invalid input never mutates the tree. A work-budget failure during
        // restructuring poisons the experiment; partial results cannot publish.
        let vector = normalize(vector, self.settings.dimensions)?;
        let result = self.insert_normalized(vector);
        self.failed = result.is_err();
        result
    }

    fn insert_normalized(&mut self, vector: Vec<f64>) -> Result<(), AppError> {
        let point = Id(self.nodes.len());
        self.nodes.push(Node::point(vector, self.leaves.len()));
        let Some(mut neighbor) = self.nearest(point, None)? else {
            self.leaves.push(point);
            self.root = Some(point);
            return Ok(());
        };
        // Equivalent to moving the arriving leaf above its aunt until the
        // existing siblings no longer prefer each other to the arriving leaf.
        while let Some(parent) = self.node(neighbor).parent {
            let sibling = self.sibling(neighbor).unwrap();
            if self.score(point, neighbor)? >= self.score(neighbor, sibling)? {
                break;
            }
            neighbor = parent;
            self.statistics.rotations += 1;
        }
        let parent = Id(self.nodes.len());
        self.nodes.push(Node::branch(self.settings.dimensions));
        self.attach(neighbor, point, parent);
        self.leaves.push(point);
        let mut current = parent;
        while self.node(current).parent.is_some() {
            current = self.graft(current)?;
        }
        Ok(())
    }

    fn graft(&mut self, start: Id) -> Result<Id, AppError> {
        let Some(mut other) = self.nearest(start, Some(start))? else {
            return Ok(self.root.unwrap());
        };
        let limit = self.lca(start, other);
        let mut current = start;
        while current != limit && other != limit && self.sibling(current) != Some(other) {
            let sibling = self.sibling(current).unwrap();
            let other_sibling = self.sibling(other).unwrap();
            let score = self.score(current, other)?;
            let current_score = self.score(current, sibling)?;
            let other_score = self.score(other, other_sibling)?;
            if score > current_score && score > other_score {
                // Reuse the detached parent: live arena stays exactly 2*n-1.
                let slot = self.detach(other);
                self.attach(current, other, slot);
                self.statistics.grafts += 1;
                // If the old sibling was other's immediate parent, it was
                // collapsed/reused by detach. The author implementation skips
                // restructuring this case; it is not a surviving residual node.
                if sibling != slot {
                    let boundary = self.lca(sibling, slot);
                    self.restructure(sibling, boundary)?;
                }
                return Ok(slot);
            }
            // Strict mutual preference, inclusive rejection prevents tie loops.
            // Move one side then recompute scores on the next iteration.
            if score <= other_score {
                other = self.node(other).parent.unwrap();
            } else {
                current = self.node(current).parent.unwrap();
            }
        }
        Ok(if current == start { limit } else { current })
    }

    fn restructure(&mut self, mut current: Id, boundary: Id) -> Result<(), AppError> {
        self.statistics.restructures += 1;
        while current != boundary {
            let sibling = self.sibling(current).unwrap();
            let mut best = sibling;
            let mut maximum = self.score(current, sibling)?;
            let mut ancestor = current;
            while ancestor != boundary {
                let candidate = self.sibling(ancestor).unwrap();
                let score = self.score(current, candidate)?;
                if score > maximum {
                    maximum = score;
                    best = candidate;
                }
                ancestor = self.node(ancestor).parent.unwrap();
            }
            if best != sibling {
                self.swap(sibling, best);
                self.statistics.swaps += 1;
            }
            current = self.node(current).parent.unwrap();
        }
        Ok(())
    }
}
