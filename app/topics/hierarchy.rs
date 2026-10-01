//! Candidate semantic ancestry, independent of BIRCH's CF storage topology.
mod build;
#[cfg(test)]
mod tests;

use super::{TopicId, normalize};
use crate::AppError;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub(super) struct GroupId(pub(super) u64);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub(super) enum NodeId {
    Topic(TopicId),
    Group(GroupId),
}

#[derive(Serialize)]
pub(super) struct Node {
    pub(super) id: NodeId,
    pub(super) parent: Option<GroupId>,
    pub(super) children: Vec<NodeId>,
    pub(super) minimum_cosine: f64,
    #[serde(skip)]
    pub(super) center: Vec<f64>,
}

#[derive(Serialize)]
pub(super) struct Hierarchy {
    pub(super) levels: Vec<f64>,
    pub(super) roots: Vec<NodeId>,
    pub(super) nodes: Vec<Node>,
    #[serde(skip)]
    parents: BTreeMap<NodeId, Option<GroupId>>,
}

impl Hierarchy {
    /// Complete-link agglomeration followed by nested cosine cuts. Every parent
    /// meets its cosine bound for every pair of descendant topic centroids.
    /// IDs are experiment-local; they are not a persisted topic namespace.
    pub(super) fn new(
        dimensions: usize,
        topics: &[(TopicId, Vec<f64>)],
        levels: &[f64],
    ) -> Result<Self, AppError> {
        if !(1..=2048).contains(&dimensions)
            || topics.len() > 1024
            || levels.is_empty()
            || levels.len() > 8
            || levels
                .iter()
                .any(|x| !x.is_finite() || !(0.0..=1.0).contains(x))
            || levels.windows(2).any(|pair| pair[0] <= pair[1])
        {
            return Err(AppError::usage("invalid semantic hierarchy settings"));
        }
        let mut ids = BTreeSet::new();
        let mut inputs = Vec::new();
        for (id, vector) in topics {
            if !ids.insert(*id) {
                return Err(AppError::usage("duplicate semantic topic identity"));
            }
            inputs.push((*id, normalize(vector, dimensions)?));
        }
        inputs.sort_by(|(a_id, a), (b_id, b)| {
            a.iter()
                .zip(b)
                .find_map(|(a, b)| {
                    let order = a.total_cmp(b);
                    (!order.is_eq()).then_some(order)
                })
                .unwrap_or_else(|| a_id.cmp(b_id))
        });
        let nodes = build::nodes(inputs, levels)?;
        let parents = nodes.iter().map(|node| (node.id, node.parent)).collect();
        let roots = nodes
            .iter()
            .filter(|node| node.parent.is_none())
            .map(|node| node.id)
            .collect();
        Ok(Self {
            levels: levels.to_vec(),
            roots,
            nodes,
            parents,
        })
    }

    /// Keep direct tags separate; overlapping children inherit a shared ancestor
    /// only once. An unknown topic fails instead of disappearing silently.
    pub(super) fn inherit(
        &self,
        direct: &BTreeSet<TopicId>,
    ) -> Result<BTreeSet<GroupId>, AppError> {
        let mut inherited = BTreeSet::new();
        for topic in direct {
            let mut current = NodeId::Topic(*topic);
            loop {
                let parent = self
                    .parents
                    .get(&current)
                    .ok_or_else(|| AppError::usage("topic is outside the semantic hierarchy"))?;
                let Some(parent) = parent else { break };
                if !inherited.insert(*parent) {
                    break;
                }
                current = NodeId::Group(*parent);
            }
        }
        Ok(inherited)
    }
}
