use super::*;

fn point(angle: f64) -> Vec<f64> {
    vec![angle.cos(), angle.sin()]
}

#[test]
fn semantic_cuts_inherit_all_ancestors_and_deduplicate_overlap() {
    let inputs = vec![
        (TopicId(0), point(0.0)),
        (TopicId(1), point(0.1)),
        (TopicId(2), point(0.7)),
    ];
    let tree = Hierarchy::new(2, &inputs, &[0.95, 0.7]).unwrap();
    let first = tree.inherit(&BTreeSet::from([TopicId(0)])).unwrap();
    let second = tree.inherit(&BTreeSet::from([TopicId(1)])).unwrap();
    let third = tree.inherit(&BTreeSet::from([TopicId(2)])).unwrap();
    assert_eq!(first.len(), 2);
    assert_eq!(first, second);
    assert_eq!(third.len(), 1);
    assert!(third.is_subset(&first));
    assert_eq!(
        tree.inherit(&BTreeSet::from([TopicId(0), TopicId(1), TopicId(2)]))
            .unwrap(),
        first
    );
    assert!(tree.inherit(&BTreeSet::from([TopicId(999)])).is_err());
    assert!(tree.inherit(&BTreeSet::new()).unwrap().is_empty());
    assert!(tree.nodes.iter().all(|node| node.center.len() == 2));
}

#[test]
fn semantic_parents_do_not_chain_dissimilar_endpoints_or_depend_on_input_order() {
    let mut inputs = vec![
        (TopicId(7), point(0.0)),
        (TopicId(3), point(0.6)),
        (TopicId(9), point(1.2)),
    ];
    let tree = Hierarchy::new(2, &inputs, &[0.7]).unwrap();
    assert_eq!(tree.roots.len(), 2); // endpoints cosine <0.7 cannot share a parent
    let groups = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.id, NodeId::Group(_)))
        .count();
    assert_eq!(groups, 1);
    inputs.reverse();
    assert_eq!(
        serde_json::to_value(&tree).unwrap(),
        serde_json::to_value(Hierarchy::new(2, &inputs, &[0.7]).unwrap()).unwrap()
    );
}

#[test]
fn semantic_hierarchy_bounds_inputs_and_keeps_namespaces_distinct() {
    assert!(Hierarchy::new(2, &vec![(TopicId(0), point(0.0)); 1025], &[0.7]).is_err());
    assert!(Hierarchy::new(0, &[], &[0.7]).is_err());
    assert!(Hierarchy::new(2, &[], &[]).is_err());
    assert!(Hierarchy::new(2, &[], &[0.4, 0.7]).is_err());
    assert!(Hierarchy::new(2, &[], &[f64::NAN]).is_err());
    assert!(Hierarchy::new(2, &[(TopicId(0), vec![0.0, 0.0])], &[0.7]).is_err());
    assert!(
        Hierarchy::new(
            2,
            &[(TopicId(0), point(0.0)), (TopicId(0), point(0.1))],
            &[0.7]
        )
        .is_err()
    );
    let tree = Hierarchy::new(
        2,
        &[(TopicId(0), point(0.0)), (TopicId(u64::MAX), point(0.1))],
        &[0.7],
    )
    .unwrap();
    assert_eq!(tree.nodes.len(), 3);
    assert_ne!(NodeId::Topic(TopicId(0)), NodeId::Group(GroupId(0)));
    assert!(Hierarchy::new(2, &[], &[0.7]).unwrap().nodes.is_empty());
}

#[test]
fn every_semantic_parent_satisfies_its_complete_link_bound() {
    let inputs: Vec<_> = (0_u64..16)
        .map(|i| {
            (
                TopicId(i),
                point(f64::from(u32::try_from(i).unwrap()) * 0.2),
            )
        })
        .collect();
    let tree = Hierarchy::new(2, &inputs, &[0.9, 0.7, 0.4]).unwrap();
    let by_id: BTreeMap<_, _> = tree.nodes.iter().map(|node| (node.id, node)).collect();
    let vectors: BTreeMap<_, _> = inputs.into_iter().collect();
    for node in &tree.nodes {
        if matches!(node.id, NodeId::Topic(_)) {
            continue;
        }
        let mut stack = node.children.clone();
        let mut leaves = BTreeSet::new();
        while let Some(id) = stack.pop() {
            match id {
                NodeId::Topic(topic) => {
                    leaves.insert(topic);
                }
                NodeId::Group(_) => stack.extend(&by_id[&id].children),
            }
        }
        assert!(leaves.len() >= 2);
        assert!(node.children.len() >= 2);
        for left in &leaves {
            for right in &leaves {
                let cosine = vectors[left]
                    .iter()
                    .zip(&vectors[right])
                    .map(|(a, b)| a * b)
                    .sum::<f64>();
                assert!(cosine + 1e-12 >= node.minimum_cosine);
            }
        }
        assert!((node.center.iter().map(|x| x * x).sum::<f64>() - 1.0).abs() < 1e-12);
    }
}
