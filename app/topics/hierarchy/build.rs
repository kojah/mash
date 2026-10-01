use super::{AppError, BTreeMap, GroupId, Node, NodeId, TopicId, normalize};
use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Clone, Copy)]
struct Pair {
    cosine: f64,
    left: usize,
    right: usize,
}
impl PartialEq for Pair {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for Pair {}
impl Ord for Pair {
    fn cmp(&self, other: &Self) -> Ordering {
        self.cosine
            .total_cmp(&other.cosine)
            .then_with(|| other.left.cmp(&self.left))
            .then_with(|| other.right.cmp(&self.right))
    }
}
impl PartialOrd for Pair {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

struct Pending {
    id: NodeId,
    parent: Option<usize>,
    children: Vec<usize>,
    sum: Vec<f64>,
    minimum: f64,
}
struct State {
    nodes: Vec<Pending>,
    active: Vec<bool>,
    matrix: Vec<f64>,
    stride: usize,
    queue: BinaryHeap<Pair>,
    leaves: usize,
}
impl State {
    fn new(inputs: Vec<(TopicId, Vec<f64>)>, minimum: f64) -> Self {
        let leaves = inputs.len();
        let stride = leaves.saturating_mul(2).saturating_sub(1);
        let nodes: Vec<_> = inputs
            .into_iter()
            .map(|(id, sum)| Pending {
                id: NodeId::Topic(id),
                parent: None,
                children: Vec::new(),
                sum,
                minimum: 1.0,
            })
            .collect();
        let mut state = Self {
            nodes,
            active: vec![true; leaves],
            matrix: vec![-1.0; stride * stride],
            stride,
            queue: BinaryHeap::new(),
            leaves,
        };
        for left in 0..leaves {
            for right in left + 1..leaves {
                let cosine = state.nodes[left]
                    .sum
                    .iter()
                    .zip(&state.nodes[right].sum)
                    .map(|(a, b)| a * b)
                    .sum::<f64>()
                    .clamp(-1.0, 1.0);
                state.connect(left, right, cosine, minimum);
            }
        }
        state
    }

    fn connect(&mut self, left: usize, right: usize, cosine: f64, minimum: f64) {
        self.matrix[left * self.stride + right] = cosine;
        self.matrix[right * self.stride + left] = cosine;
        if cosine >= minimum {
            self.queue.push(Pair {
                cosine,
                left: left.min(right),
                right: left.max(right),
            });
        }
    }

    fn merge(&mut self, pair: Pair, minimum: f64) {
        let index = self.nodes.len();
        let sum = self.nodes[pair.left]
            .sum
            .iter()
            .zip(&self.nodes[pair.right].sum)
            .map(|(a, b)| a + b)
            .collect();
        self.nodes[pair.left].parent = Some(index);
        self.nodes[pair.right].parent = Some(index);
        self.active[pair.left] = false;
        self.active[pair.right] = false;
        self.nodes.push(Pending {
            id: NodeId::Group(GroupId(
                u64::try_from(index - self.leaves).expect("bounded hierarchy"),
            )),
            parent: None,
            children: vec![pair.left, pair.right],
            sum,
            minimum: pair.cosine,
        });
        self.active.push(true);
        for other in 0..index {
            if self.active[other] {
                let cosine = self.matrix[pair.left * self.stride + other]
                    .min(self.matrix[pair.right * self.stride + other]);
                self.connect(index, other, cosine, minimum);
            }
        }
    }

    fn cuts(self, levels: &[f64]) -> Result<Vec<Node>, AppError> {
        let mut retained: Vec<_> = self.nodes.iter().map(|n| n.children.is_empty()).collect();
        for level in levels {
            let mut stack: Vec<_> = self
                .active
                .iter()
                .enumerate()
                .filter_map(|(i, active)| active.then_some(i))
                .collect();
            while let Some(index) = stack.pop() {
                let node = &self.nodes[index];
                if node.minimum >= *level {
                    retained[index] = true;
                } else {
                    stack.extend(&node.children);
                }
            }
        }
        let mut nodes = BTreeMap::new();
        for (index, pending) in self.nodes.iter().enumerate() {
            if !retained[index] {
                continue;
            }
            let mut parent = pending.parent;
            while let Some(index) = parent {
                if retained[index] {
                    break;
                }
                parent = self.nodes[index].parent;
            }
            let parent = parent.map(|index| match self.nodes[index].id {
                NodeId::Group(id) => id,
                NodeId::Topic(_) => unreachable!("only groups receive children"),
            });
            nodes.insert(
                pending.id,
                Node {
                    id: pending.id,
                    parent,
                    children: Vec::new(),
                    minimum_cosine: pending.minimum,
                    center: normalize(&pending.sum, pending.sum.len())?,
                },
            );
        }
        let edges: Vec<_> = nodes
            .values()
            .filter_map(|n| n.parent.map(|p| (n.id, p)))
            .collect();
        for (child, parent) in edges {
            nodes
                .get_mut(&NodeId::Group(parent))
                .expect("retained parent")
                .children
                .push(child);
        }
        Ok(nodes.into_values().collect())
    }
}

/// O(n^2 * (dimensions + log n)) work; the bounded matrix/heap have O(n^2)
/// storage. Lazy invalidation discards old pairs without mutating live identities.
pub(super) fn nodes(
    inputs: Vec<(TopicId, Vec<f64>)>,
    levels: &[f64],
) -> Result<Vec<Node>, AppError> {
    let minimum = *levels.last().expect("validated nonempty levels");
    let mut state = State::new(inputs, minimum);
    while let Some(pair) = state.queue.pop() {
        if state.active[pair.left] && state.active[pair.right] {
            state.merge(pair, minimum);
        }
    }
    state.cuts(levels)
}
