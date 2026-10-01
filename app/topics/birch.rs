//! Online count/linear-sum/squared-sum CF tree with bounded leaf growth.
use super::{Settings, TopicId, normalize};
use crate::AppError;

#[derive(Clone)]
struct Feature {
    count: u64,
    sum: Vec<f64>,
    squares: f64,
}
impl Feature {
    fn point(vector: &[f64]) -> Self {
        Self {
            count: 1,
            sum: vector.to_vec(),
            squares: vector.iter().map(|x| x * x).sum(),
        }
    }
    fn add(&mut self, other: &Self) {
        self.count += other.count;
        for (sum, value) in self.sum.iter_mut().zip(&other.sum) {
            *sum += value;
        }
        self.squares += other.squares;
    }
    fn centroid(&self) -> Vec<f64> {
        #[allow(clippy::cast_precision_loss)]
        let count = self.count as f64;
        self.sum.iter().map(|x| x / count).collect()
    }
    fn radius_squared(&self) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        let count = self.count as f64;
        (self.squares / count - self.sum.iter().map(|x| (x / count).powi(2)).sum::<f64>()).max(0.0)
    }
}

struct Entry {
    feature: Feature,
    kind: Kind,
}
enum Kind {
    Leaf(TopicId),
    Branch(Node),
}
struct Node {
    entries: Vec<Entry>,
}
impl Node {
    fn aggregate(&self) -> Feature {
        let mut feature = self.entries[0].feature.clone();
        for entry in &self.entries[1..] {
            feature.add(&entry.feature);
        }
        feature
    }
    fn branch(self) -> Entry {
        Entry {
            feature: self.aggregate(),
            kind: Kind::Branch(self),
        }
    }
    fn insert(
        &mut self,
        point: &Feature,
        settings: Settings,
        next: &mut u64,
    ) -> Result<(TopicId, Option<Self>), AppError> {
        let closest = self
            .entries
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                distance(&a.feature, point).total_cmp(&distance(&b.feature, point))
            })
            .map(|(i, _)| i);
        let id = if let Some(i) = closest {
            match &mut self.entries[i].kind {
                Kind::Branch(child) => {
                    let (id, split) = child.insert(point, settings, next)?;
                    self.entries[i].feature = child.aggregate();
                    if let Some(split) = split {
                        self.entries.push(split.branch());
                    }
                    id
                }
                Kind::Leaf(id) => {
                    let id = *id;
                    let mut combined = self.entries[i].feature.clone();
                    combined.add(point);
                    if combined.radius_squared() <= settings.radius.powi(2) {
                        self.entries[i].feature = combined;
                        id
                    } else {
                        self.new_leaf(point, settings, next)?
                    }
                }
            }
        } else {
            self.new_leaf(point, settings, next)?
        };
        let split = (self.entries.len() > settings.branching).then(|| self.split());
        Ok((id, split))
    }
    fn new_leaf(
        &mut self,
        point: &Feature,
        settings: Settings,
        next: &mut u64,
    ) -> Result<TopicId, AppError> {
        if *next >= u64::try_from(settings.max_topics).unwrap_or(u64::MAX) {
            return Err(AppError::usage(
                "experimental topic capacity reached; increase max_topics or radius",
            ));
        }
        let id = TopicId(*next);
        *next += 1;
        self.entries.push(Entry {
            feature: point.clone(),
            kind: Kind::Leaf(id),
        });
        Ok(id)
    }
    fn split(&mut self) -> Self {
        let mut seeds = (0, 1);
        let mut farthest = -1.0;
        for i in 0..self.entries.len() {
            for j in i + 1..self.entries.len() {
                let score = distance(&self.entries[i].feature, &self.entries[j].feature);
                if score > farthest {
                    farthest = score;
                    seeds = (i, j);
                }
            }
        }
        let first = self.entries[seeds.0].feature.clone();
        let second = self.entries[seeds.1].feature.clone();
        let mut sibling = Self {
            entries: Vec::new(),
        };
        for (i, entry) in std::mem::take(&mut self.entries).into_iter().enumerate() {
            if i == seeds.1
                || (i != seeds.0
                    && distance(&entry.feature, &second) < distance(&entry.feature, &first))
            {
                sibling.entries.push(entry);
            } else {
                self.entries.push(entry);
            }
        }
        sibling
    }
    fn leaves(&self, output: &mut Vec<(TopicId, Vec<f64>, u64)>) {
        for entry in &self.entries {
            match &entry.kind {
                Kind::Leaf(id) => output.push((*id, entry.feature.centroid(), entry.feature.count)),
                Kind::Branch(child) => child.leaves(output),
            }
        }
    }
    fn depth(&self) -> usize {
        1 + self
            .entries
            .iter()
            .filter_map(|e| match &e.kind {
                Kind::Leaf(_) => None,
                Kind::Branch(child) => Some(child.depth()),
            })
            .max()
            .unwrap_or(0)
    }
}

fn distance(first: &Feature, second: &Feature) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let (a, b) = (first.count as f64, second.count as f64);
    first
        .sum
        .iter()
        .zip(&second.sum)
        .map(|(x, y)| (x / a - y / b).powi(2))
        .sum()
}

pub(super) struct Tree {
    settings: Settings,
    root: Node,
    next: u64,
    samples: u64,
}
impl Tree {
    pub(super) fn new(settings: Settings) -> Result<Self, AppError> {
        settings.validate()?;
        Ok(Self {
            settings,
            root: Node {
                entries: Vec::new(),
            },
            next: 0,
            samples: 0,
        })
    }
    pub(super) fn insert(&mut self, vector: &[f64]) -> Result<TopicId, AppError> {
        let vector = normalize(vector, self.settings.dimensions)?;
        if self.samples >= (1_u64 << 53) {
            return Err(AppError::usage(
                "experimental topic sample capacity reached",
            ));
        }
        let (id, split) =
            self.root
                .insert(&Feature::point(&vector), self.settings, &mut self.next)?;
        if let Some(split) = split {
            let old = std::mem::replace(
                &mut self.root,
                Node {
                    entries: Vec::new(),
                },
            );
            self.root.entries = vec![old.branch(), split.branch()];
        }
        self.samples += 1;
        Ok(id)
    }
    pub(super) fn leaves(&self) -> Vec<(TopicId, Vec<f64>, u64)> {
        let mut output = Vec::new();
        self.root.leaves(&mut output);
        output.sort_by_key(|e| e.0);
        output
    }
    pub(super) fn depth(&self) -> usize {
        self.root.depth()
    }
}
