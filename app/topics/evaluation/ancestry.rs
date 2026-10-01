//! Private semantic-forest diagnostics; counts use inherited thread sets.
use super::{BTreeMap, BTreeSet, Cluster, Thread, TopicId};
use crate::AppError;
use crate::provider::Provider;
use crate::topics::hierarchy::{GroupId, Hierarchy, NodeId};
use serde::Serialize;
use std::time::Instant;
mod names;
use super::{
    labels,
    names::{Labeler, Name, Summary},
};

#[derive(Serialize)]
pub(super) struct GroupStats {
    id: GroupId,
    threads: usize,
    providers: BTreeMap<Provider, usize>,
    #[serde(flatten)]
    name: Name,
}

#[derive(Serialize)]
pub(super) struct Report {
    pub(super) build_ms: f64,
    pub(super) inheritance_ms: f64,
    pub(super) groups: usize,
    pub(super) maximum_depth: usize,
    leaf_topics: usize,
    maximum_inherited_tags: usize,
    deduplicated_ancestor_contributions: usize,
    graph: Hierarchy,
    memberships: BTreeMap<String, BTreeSet<GroupId>>,
    stats: Vec<GroupStats>,
    parent_label_ms: f64,
    semantic_labels: Option<Summary>,
}

impl Report {
    pub(super) fn label(&mut self, labeler: &mut Labeler) -> Result<(), AppError> {
        let mut names: Vec<_> = self.stats.iter_mut().map(|g| &mut g.name).collect();
        self.semantic_labels = Some(labeler.apply(&mut names)?);
        Ok(())
    }
}

fn group_names(
    graph: &Hierarchy,
    ancestors: &BTreeMap<TopicId, BTreeSet<GroupId>>,
    direct: &BTreeMap<String, BTreeSet<TopicId>>,
    evidence: &BTreeMap<String, BTreeMap<TopicId, labels::Terms>>,
) -> Result<BTreeMap<GroupId, GroupStats>, AppError> {
    let classes = names::classes(ancestors, direct, evidence)?;
    let mut candidates = labels::labels(&classes, 64);
    Ok(graph
        .nodes
        .iter()
        .filter_map(|node| match node.id {
            NodeId::Group(id) => Some((
                id,
                GroupStats {
                    id,
                    threads: 0,
                    providers: BTreeMap::new(),
                    name: Name::new(
                        node.center.clone(),
                        candidates.remove(&id).unwrap_or_default(),
                    ),
                },
            )),
            NodeId::Topic(_) => None,
        })
        .collect())
}

pub(super) fn build(
    threads: &BTreeMap<String, Thread>,
    clusters: &[Cluster],
    direct: &BTreeMap<String, BTreeSet<TopicId>>,
    evidence: &BTreeMap<String, BTreeMap<TopicId, labels::Terms>>,
) -> Result<Report, AppError> {
    let start = Instant::now();
    let topics: Vec<_> = clusters
        .iter()
        .filter(|c| c.threads > 0)
        .map(|c| (c.id, c.name.center.clone()))
        .collect();
    let graph = Hierarchy::new(384, &topics, &[0.75, 0.6, 0.45])?;
    let build_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let mut ancestors = BTreeMap::new();
    let mut maximum_depth = 0;
    for node in &graph.nodes {
        if let NodeId::Topic(id) = node.id {
            let path = graph.inherit(&BTreeSet::from([id]))?;
            maximum_depth = maximum_depth.max(path.len());
            ancestors.insert(id, path);
        }
    }
    let mut memberships = BTreeMap::new();
    let named_start = Instant::now();
    let mut stats = group_names(&graph, &ancestors, direct, evidence)?;
    let parent_label_ms = named_start.elapsed().as_secs_f64() * 1000.0;
    let mut deduplicated = 0;
    for (thread_id, topics) in direct {
        let thread = threads
            .get(thread_id)
            .ok_or_else(|| AppError::usage("ancestry membership references unknown thread"))?;
        let mut inherited = BTreeSet::new();
        let mut contributions = 0;
        for topic in topics {
            let path = ancestors
                .get(topic)
                .ok_or_else(|| AppError::usage("ancestry membership references unknown topic"))?;
            contributions += path.len();
            inherited.extend(path);
        }
        deduplicated += contributions - inherited.len();
        for id in &inherited {
            let entry = stats.get_mut(id).expect("ancestor groups were prepared");
            entry.threads += 1;
            *entry.providers.entry(thread.provider).or_default() += 1;
        }
        memberships.insert(thread_id.clone(), inherited);
    }
    Ok(Report {
        build_ms,
        inheritance_ms: (start.elapsed().as_secs_f64() * 1000.0 - parent_label_ms).max(0.0),
        groups: graph
            .nodes
            .iter()
            .filter(|n| matches!(n.id, NodeId::Group(_)))
            .count(),
        maximum_depth,
        leaf_topics: topics.len(),
        maximum_inherited_tags: memberships.values().map(BTreeSet::len).max().unwrap_or(0),
        deduplicated_ancestor_contributions: deduplicated,
        graph,
        memberships,
        stats: stats.into_values().collect(),
        parent_label_ms,
        semantic_labels: None,
    })
}

#[test]
fn inherited_counts_use_distinct_threads_across_overlapping_direct_tags() {
    let topics: Vec<_> = [0.0_f64, 0.1, 0.9]
        .into_iter()
        .enumerate()
        .map(|(i, angle)| {
            let mut center = vec![angle.cos(), angle.sin()];
            center.resize(384, 0.0);
            Cluster {
                id: TopicId(u64::try_from(i).unwrap()),
                samples: 1,
                threads: 1,
                providers: BTreeMap::new(),
                name: Name::new(center, Vec::new()),
                examples: Vec::new(),
            }
        })
        .collect();
    let first = super::anchored::thread(&[0]);
    let mut second = super::anchored::thread(&[0]);
    second.provider = Provider::ClaudeCode;
    let threads = BTreeMap::from([("a".into(), first), ("b".into(), second)]);
    let direct: BTreeMap<String, BTreeSet<TopicId>> = BTreeMap::from([
        ("a".into(), BTreeSet::from([TopicId(0), TopicId(1)])),
        ("b".into(), BTreeSet::from([TopicId(2)])),
    ]);
    let evidence = direct
        .iter()
        .map(|(thread, topics)| {
            (
                thread.clone(),
                topics
                    .iter()
                    .map(|id| (*id, labels::terms("database")))
                    .collect(),
            )
        })
        .collect();
    let report = build(&threads, &topics, &direct, &evidence).unwrap();
    assert_eq!(report.maximum_depth, 2);
    assert_eq!(report.maximum_inherited_tags, 2);
    assert_eq!(report.deduplicated_ancestor_contributions, 2);
    let root = report.stats.iter().find(|g| g.threads == 2).unwrap();
    assert_eq!(root.providers[&Provider::Codex], 1);
    assert_eq!(root.providers[&Provider::ClaudeCode], 1);
    assert_eq!(
        report
            .stats
            .iter()
            .find(|g| g.threads == 1)
            .unwrap()
            .providers[&Provider::Codex],
        1
    );
    assert!(build(&BTreeMap::new(), &topics, &direct, &evidence).is_err());
}
