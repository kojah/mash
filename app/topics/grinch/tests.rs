use super::*;

fn tree(dimensions: usize) -> Tree {
    Tree::new(Settings {
        dimensions,
        maximum_points: 8192,
        maximum_scores: 100_000_000,
    })
    .unwrap()
}

#[test]
fn exact_online_tree_recovers_separated_subjects_in_multiple_orders() {
    let points: Vec<_> = (0..48)
        .map(|i| {
            let mut v = vec![0.01; 3];
            v[i % 3] = 1.0;
            v[(i + 1) % 3] += f64::from(u32::try_from(i).unwrap()) * 0.0002;
            v
        })
        .collect();
    for stride in [1, 5, 7, 11, 17] {
        let mut tree = tree(3);
        for i in 0..48 {
            tree.insert(&points[i * stride % 48]).unwrap();
            tree.audit();
        }
        let cut = tree.cut(0.1).unwrap();
        assert_eq!(cut.len(), 3);
        assert!(cut.iter().all(|(_, _, count)| *count == 16));
        for (_, center, _) in cut {
            assert!(center.iter().copied().fold(0.0, f64::max) > 0.99);
        }
    }
}

#[test]
fn nonlocal_graft_and_restructure_preserve_all_points_and_features() {
    let mut tree = tree(3);
    // Deliberately interleaved wrong pairs; a and b mutually prefer a graft.
    for (index, vector) in [
        vec![1., 0., 0.],
        vec![0., 1., 0.],
        vec![1., 0., 0.],
        vec![0., 0., 1.],
    ]
    .into_iter()
    .enumerate()
    {
        let id = Id(tree.nodes.len());
        tree.nodes.push(Node::point(vector, index));
        tree.leaves.push(id);
    }
    for (a, b) in [(0, 1), (2, 3), (4, 5)] {
        let id = Id(tree.nodes.len());
        tree.nodes.push(Node::branch(3));
        tree.attach(Id(a), Id(b), id);
    }
    tree.audit();
    tree.graft(Id(0)).unwrap();
    tree.audit();
    assert_eq!(tree.statistics.grafts, 1);
    assert_eq!(tree.statistics.restructures, 1);
    let cut = tree.cut(0.1).unwrap();
    assert_eq!(cut.len(), 3);
    assert!(
        cut.iter()
            .any(|(_, v, n)| *n == 2 && (v[0] - 1.).abs() < 1e-10)
    );
}

#[test]
fn mixed_signed_stream_exercises_rotations_grafts_and_restructuring() {
    let mut tree = tree(8);
    let mut state = 71_u64;
    for _ in 0..160 {
        let vector: Vec<_> = (0..8)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                f64::from((state >> 32) as u32) / f64::from(u32::MAX) * 2. - 1.
            })
            .collect();
        tree.insert(&vector).unwrap();
        tree.audit();
    }
    assert!(tree.statistics.rotations > 0);
    assert!(tree.statistics.grafts > 0);
    assert!(tree.statistics.restructures > 0);
    for radius in [0.01, 0.4, 0.6, 0.9] {
        let cut = tree.cut(radius).unwrap();
        assert_eq!(cut.iter().map(|(_, _, n)| n).sum::<u64>(), 160);
        assert_eq!(
            cut.iter()
                .map(|(id, _, _)| id)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            cut.len()
        );
    }
}

#[test]
fn restructuring_swaps_a_better_ancestor_sibling_without_losing_descendants() {
    let mut tree = tree(3);
    for (index, vector) in [
        vec![1., 0., 0.],
        vec![0., 1., 0.],
        vec![1., 0., 0.],
        vec![0., 0., 1.],
    ]
    .into_iter()
    .enumerate()
    {
        let id = Id(tree.nodes.len());
        tree.nodes.push(Node::point(vector, index));
        tree.leaves.push(id);
    }
    for (a, b) in [(0, 1), (2, 3), (4, 5)] {
        let id = Id(tree.nodes.len());
        tree.nodes.push(Node::branch(3));
        tree.attach(Id(a), Id(b), id);
    }
    tree.restructure(Id(0), Id(6)).unwrap();
    tree.audit();
    assert_eq!(tree.statistics.swaps, 1);
    assert_eq!(tree.sibling(Id(0)), Some(Id(5)));
}

#[test]
fn ties_cancellation_and_invalid_inputs_are_bounded_and_fail_closed() {
    let mut tree = tree(2);
    tree.insert(&[1., 0.]).unwrap();
    tree.insert(&[-1., 0.]).unwrap();
    tree.audit();
    assert_eq!(tree.cut(0.9).unwrap().len(), 2);
    for _ in 0..10 {
        tree.insert(&[1., 0.]).unwrap();
        tree.audit();
    }
    let before = tree.nodes.len();
    for vector in [&[0., 0.][..], &[f64::NAN, 0.][..], &[1.][..]] {
        assert!(tree.insert(vector).is_err());
    }
    assert_eq!(tree.nodes.len(), before);
    tree.audit();
    assert!(tree.cut(f64::NAN).is_err());
    let mut limited = Tree::new(Settings {
        dimensions: 2,
        maximum_points: 2,
        maximum_scores: 1,
    })
    .unwrap();
    limited.insert(&[1., 0.]).unwrap();
    limited.insert(&[0., 1.]).unwrap();
    assert!(limited.insert(&[1., 1.]).is_err());
    limited.audit();
    let mut budget = Tree::new(Settings {
        dimensions: 2,
        maximum_points: 8,
        maximum_scores: 1,
    })
    .unwrap();
    budget.insert(&[1., 0.]).unwrap();
    budget.insert(&[0., 1.]).unwrap();
    assert!(budget.insert(&[1., 1.]).is_err());
    assert!(budget.snapshot().is_err());
    assert!(budget.cut(0.4).is_err());
    assert!(budget.insert(&[1., 0.]).is_err());
}
