use super::*;

const SEQUENCE: &[u32] = &[
    18, 53, 38, 42, 30, 44, 39, 45, 1, 35, 10, 22, 54, 1, 52, 57, 26, 51, 58, 34, 50, 19, 57, 14,
    7, 56, 3,
];

fn vector(angle: u32) -> [f64; 2] {
    let radians = f64::from(angle) * 0.1;
    [radians.cos(), radians.sin()]
}
fn settings(routing: Routing, max_topics: usize) -> Settings {
    Settings {
        dimensions: 2,
        radius: 0.001,
        branching: 2,
        max_topics,
        routing,
    }
}
fn populated(routing: Routing, maximum: usize) -> (Tree, TopicId) {
    let mut tree = Tree::new(settings(routing, maximum)).unwrap();
    let mut expected = None;
    for &angle in SEQUENCE {
        let topic = tree.insert(&vector(angle)).unwrap();
        if angle == 10 {
            expected = Some(topic);
        }
    }
    (tree, expected.unwrap())
}

fn consistent(node: &Node) {
    for entry in &node.entries {
        if let Kind::Branch(child) = &entry.kind {
            let computed = child.aggregate();
            assert_eq!(entry.feature.count, computed.count);
            for (a, b) in entry.feature.sum.iter().zip(computed.sum) {
                assert!((a - b).abs() < 1e-12);
            }
            assert!((entry.feature.squares - computed.squares).abs() < 1e-12);
            consistent(child);
        }
    }
}

#[test]
fn exact_routing_finds_an_existing_leaf_beyond_the_closest_branch_at_capacity() {
    // The nearby point is unique, so exact-vector dedup cannot prevent the miss.
    for query in [vector(10), [1.001_f64.cos(), 1.001_f64.sin()]] {
        let (mut branch, original) = populated(Routing::BranchCentroid, 64);
        let before = branch.leaves().len();
        assert_ne!(branch.insert(&query).unwrap(), original);
        assert_eq!(branch.leaves().len(), before + 1);
        let (mut branch, _) = populated(Routing::BranchCentroid, 25);
        assert!(branch.insert(&query).is_err());

        let (mut exact, original) = populated(Routing::ExactLeaf, 25);
        assert_eq!(exact.leaves().len(), 25);
        assert_eq!(exact.insert(&query).unwrap(), original);
        assert_eq!(exact.leaves().len(), 25);
        assert_eq!(exact.samples, 28);
        assert_eq!(exact.root.aggregate().count, 28);
        assert_eq!(exact.leaves().iter().map(|(_, _, n)| n).sum::<u64>(), 28);
        consistent(&exact.root);
    }
}

#[test]
fn failed_exact_insertions_leave_leaf_ids_counts_and_ancestor_statistics_unchanged() {
    let (mut tree, _) = populated(Routing::ExactLeaf, 25);
    let before = tree.leaves();
    let aggregate = tree.root.aggregate();
    let counters = (tree.samples, tree.next, tree.depth());
    for vector in [
        vec![1.0, 0.0],
        vec![0.0, 0.0],
        vec![f64::NAN, 1.0],
        vec![1.0],
        vec![f64::INFINITY, 0.0],
    ] {
        assert!(tree.insert(&vector).is_err());
        assert_eq!(tree.leaves(), before);
        assert_eq!(tree.root.aggregate().sum, aggregate.sum);
        assert_eq!(tree.root.aggregate().squares, aggregate.squares);
        assert_eq!((tree.samples, tree.next, tree.depth()), counters);
        consistent(&tree.root);
    }
    tree.samples = 1_u64 << 53;
    assert!(tree.insert(&vector(10)).is_err());
    assert_eq!(tree.leaves(), before);
}

#[test]
fn equal_leaf_distances_use_topic_id_instead_of_storage_order() {
    let node = Node {
        entries: vec![
            Entry {
                feature: Feature::point(&[1.0, 0.0]),
                kind: Kind::Leaf(TopicId(8)),
            },
            Entry {
                feature: Feature::point(&[-1.0, 0.0]),
                kind: Kind::Leaf(TopicId(3)),
            },
        ],
    };
    let mut nearest = None;
    node.nearest_leaf(&Feature::point(&[0.0, 1.0]), &mut nearest);
    assert_eq!(nearest, Some((TopicId(3), 2.0)));
}

fn leaf(id: u64, radians: f64, count: usize) -> Entry {
    let point = Feature::point(&[radians.cos(), radians.sin()]);
    let mut feature = point.clone();
    for _ in 1..count {
        feature.add(&point);
    }
    Entry {
        feature,
        kind: Kind::Leaf(TopicId(id)),
    }
}

#[test]
fn a_nearest_leaf_radius_rejection_creates_a_leaf_without_merging_a_farther_one() {
    let mut spread = leaf(0, 0.4, 1);
    spread
        .feature
        .add(&Feature::point(&[(-0.4_f64).cos(), (-0.4_f64).sin()]));
    let root = Node {
        entries: vec![
            Node {
                entries: vec![spread, leaf(1, std::f64::consts::PI, 100)],
            }
            .branch(),
            Node {
                entries: vec![leaf(2, 1.8, 100)],
            }
            .branch(),
        ],
    };
    let mut tree = Tree {
        root,
        settings: Settings {
            radius: 0.4,
            ..settings(Routing::ExactLeaf, 4)
        },
        next: 3,
        samples: 202,
    };
    let query = Feature::point(&vector(8));
    let mut nearest = None;
    tree.root.nearest_leaf(&query, &mut nearest);
    assert_eq!(nearest.unwrap().0, TopicId(0));
    assert!(!tree.root.merge_leaf(TopicId(0), &query, 0.4));
    assert_eq!(tree.insert(&vector(8)).unwrap(), TopicId(3));
    let counts: Vec<_> = tree.leaves().iter().map(|(id, _, n)| (*id, *n)).collect();
    assert_eq!(
        counts,
        [
            (TopicId(0), 2),
            (TopicId(1), 100),
            (TopicId(2), 100),
            (TopicId(3), 1)
        ]
    );
    assert_eq!(tree.samples, 203);
    assert_eq!(tree.root.aggregate().count, 203);
    consistent(&tree.root);
}
