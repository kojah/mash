use super::{AppError, Id, Statistics, TopicId, Tree};
use serde::Serialize;

pub(super) struct Node {
    pub(super) parent: Option<Id>,
    children: Option<[Id; 2]>,
    pub(super) center: Vec<f64>,
    sum: Vec<f64>,
    count: u64,
    first: usize,
}
impl Node {
    pub(super) fn point(vector: Vec<f64>, first: usize) -> Self {
        Self {
            parent: None,
            children: None,
            sum: vector.clone(),
            center: vector,
            count: 1,
            first,
        }
    }
    pub(super) fn branch(dimensions: usize) -> Self {
        Self {
            parent: None,
            children: None,
            center: vec![0.0; dimensions],
            sum: vec![0.0; dimensions],
            count: 0,
            first: 0,
        }
    }
    fn radius_squared(&self) -> f64 {
        #[allow(
            clippy::cast_precision_loss,
            reason = "bounded experimental point count"
        )]
        let count = self.count as f64;
        (1.0 - self.sum.iter().map(|x| (x / count).powi(2)).sum::<f64>()).max(0.0)
    }
}

#[derive(Serialize)]
pub(in crate::topics) struct Snapshot {
    pub(in crate::topics) statistics: Statistics,
    pub(in crate::topics) points: usize,
    pub(in crate::topics) depth: usize,
    nodes: Vec<Entry>,
}
#[derive(Serialize)]
struct Entry {
    id: Id,
    parent: Option<Id>,
    children: Option<[Id; 2]>,
    count: u64,
    radius: f64,
    first_point: usize,
}

impl Tree {
    pub(super) fn node(&self, id: Id) -> &Node {
        &self.nodes[id.0]
    }
    pub(super) fn contains(&self, ancestor: Id, mut node: Id) -> bool {
        loop {
            if ancestor == node {
                return true;
            }
            let Some(parent) = self.node(node).parent else {
                return false;
            };
            node = parent;
        }
    }
    pub(super) fn sibling(&self, node: Id) -> Option<Id> {
        let [a, b] = self.node(self.node(node).parent?).children.unwrap();
        Some(if a == node { b } else { a })
    }
    pub(super) fn lca(&self, a: Id, b: Id) -> Id {
        let mut ancestors = std::collections::BTreeSet::new();
        let mut current = Some(a);
        while let Some(node) = current {
            ancestors.insert(node);
            current = self.node(node).parent;
        }
        let mut current = b;
        while !ancestors.contains(&current) {
            current = self.node(current).parent.unwrap();
        }
        current
    }
    fn replace(&mut self, parent: Option<Id>, old: Id, new: Id) {
        if let Some(parent) = parent {
            let children = self.nodes[parent.0].children.as_mut().unwrap();
            let index = usize::from(children[0] != old);
            assert_eq!(children[index], old, "internal GRINCH edge invariant");
            children[index] = new;
        } else {
            self.root = Some(new);
        }
        self.nodes[new.0].parent = parent;
    }
    fn refresh(&mut self, mut current: Option<Id>) {
        while let Some(node) = current {
            if let Some([a, b]) = self.node(node).children {
                let sum: Vec<_> = self
                    .node(a)
                    .sum
                    .iter()
                    .zip(&self.node(b).sum)
                    .map(|(x, y)| x + y)
                    .collect();
                let norm = sum.iter().map(|x| x * x).sum::<f64>().sqrt();
                let center = if norm == 0.0 {
                    vec![0.0; sum.len()]
                } else {
                    sum.iter().map(|x| x / norm).collect()
                };
                let count = self.node(a).count + self.node(b).count;
                let first = self.node(a).first.min(self.node(b).first);
                let target = &mut self.nodes[node.0];
                target.sum = sum;
                target.center = center;
                target.count = count;
                target.first = first;
            }
            current = self.node(node).parent;
        }
    }
    pub(super) fn attach(&mut self, a: Id, b: Id, slot: Id) {
        let parent = self.node(a).parent;
        self.replace(parent, a, slot);
        self.nodes[slot.0].children = Some([a, b]);
        self.nodes[a.0].parent = Some(slot);
        self.nodes[b.0].parent = Some(slot);
        self.refresh(Some(slot));
    }
    pub(super) fn detach(&mut self, node: Id) -> Id {
        let parent = self.node(node).parent.unwrap();
        let grandparent = self.node(parent).parent;
        let sibling = self.sibling(node).unwrap();
        self.replace(grandparent, parent, sibling);
        self.nodes[node.0].parent = None;
        self.nodes[parent.0].parent = None;
        self.nodes[parent.0].children = None;
        self.refresh(grandparent);
        parent
    }
    pub(super) fn swap(&mut self, a: Id, b: Id) {
        let ap = self.node(a).parent;
        let bp = self.node(b).parent;
        assert!(ap != bp && !self.contains(a, b) && !self.contains(b, a));
        self.replace(ap, a, b);
        self.replace(bp, b, a);
        self.refresh(ap);
        self.refresh(bp);
    }
    pub(in crate::topics) fn snapshot(&self) -> Result<Snapshot, AppError> {
        if self.failed {
            return Err(AppError::usage("experimental GRINCH run already failed"));
        }
        let mut depth = 0;
        let mut stack: Vec<_> = self.root.into_iter().map(|root| (root, 1)).collect();
        while let Some((node, level)) = stack.pop() {
            depth = depth.max(level);
            if let Some(children) = self.node(node).children {
                stack.extend(children.map(|child| (child, level + 1)));
            }
        }
        Ok(Snapshot {
            statistics: self.statistics,
            points: self.leaves.len(),
            depth,
            nodes: self
                .nodes
                .iter()
                .enumerate()
                .map(|(id, n)| Entry {
                    id: Id(id),
                    parent: n.parent,
                    children: n.children,
                    count: n.count,
                    radius: n.radius_squared().sqrt(),
                    first_point: n.first,
                })
                .collect(),
        })
    }
    pub(in crate::topics) fn cut(
        &self,
        radius: f64,
    ) -> Result<Vec<(TopicId, Vec<f64>, u64)>, AppError> {
        if self.failed || !radius.is_finite() || !(0.0..1.0).contains(&radius) {
            return Err(AppError::usage("invalid experimental GRINCH cut"));
        }
        let mut stack: Vec<_> = self.root.into_iter().collect();
        let mut result = Vec::new();
        while let Some(id) = stack.pop() {
            let node = self.node(id);
            match node.children {
                Some(children) if node.radius_squared() > radius * radius => stack.extend(children),
                _ => result.push((TopicId(node.first as u64), node.center.clone(), node.count)),
            }
        }
        result.sort_by_key(|(id, _, _)| *id);
        Ok(result)
    }
}

#[cfg(test)]
impl Tree {
    pub(super) fn audit(&self) {
        let Some(root) = self.root else {
            assert!(self.nodes.is_empty());
            return;
        };
        assert!(self.node(root).parent.is_none());
        let mut seen = std::collections::BTreeSet::new();
        let mut stack = vec![root];
        let mut leaf_ids = Vec::new();
        while let Some(id) = stack.pop() {
            assert!(seen.insert(id), "cycle or repeated subtree");
            let node = self.node(id);
            if let Some([a, b]) = node.children {
                assert_eq!(self.node(a).parent, Some(id));
                assert_eq!(self.node(b).parent, Some(id));
                assert_eq!(node.count, self.node(a).count + self.node(b).count);
                for ((actual, a), b) in node
                    .sum
                    .iter()
                    .zip(&self.node(a).sum)
                    .zip(&self.node(b).sum)
                {
                    assert!((actual - a - b).abs() < 1e-10);
                }
                let norm = node.sum.iter().map(|x| x * x).sum::<f64>().sqrt();
                for (actual, sum) in node.center.iter().zip(&node.sum) {
                    let expected = if norm == 0.0 { 0.0 } else { sum / norm };
                    assert!((actual - expected).abs() < 1e-10);
                }
                assert_eq!(node.first, self.node(a).first.min(self.node(b).first));
                stack.extend([a, b]);
            } else {
                assert_eq!(node.count, 1);
                leaf_ids.push(id);
            }
        }
        assert_eq!(seen.len(), self.nodes.len());
        leaf_ids.sort();
        let mut expected = self.leaves.clone();
        expected.sort();
        assert_eq!(leaf_ids, expected);
        assert_eq!(self.nodes.len(), 2 * self.leaves.len() - 1);
        assert_eq!(self.node(root).count, self.leaves.len() as u64);
    }
}
