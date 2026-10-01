use super::{AssignmentModel, Settings, TopicId, birch::Tree, labels, normalize};

fn settings() -> Settings {
    Settings {
        dimensions: 2,
        radius: 0.05,
        branching: 2,
        max_topics: 64,
    }
}

#[test]
fn cf_tree_splits_keep_exact_centroids_counts_and_stable_leaf_ids() {
    let mut tree = Tree::new(settings()).unwrap();
    let mut expected = std::collections::BTreeMap::new();
    for i in 0..20 {
        let angle = f64::from(i) * 0.3;
        let vector = [angle.cos(), angle.sin()];
        let id = tree.insert(&vector).unwrap();
        expected.insert(id, vector);
        assert_eq!(tree.insert(&vector).unwrap(), id);
    }
    assert!(tree.depth() >= 3);
    assert_eq!(tree.leaves().len(), 20);
    for (id, centroid, count) in tree.leaves() {
        assert_eq!(count, 2);
        for (actual, expected) in centroid.iter().zip(expected[&id]) {
            assert!((actual - expected).abs() < 1e-12);
        }
    }
}

#[test]
fn capacity_and_invalid_vectors_do_not_mutate_existing_topic_state() {
    let mut tree = Tree::new(Settings {
        max_topics: 1,
        ..settings()
    })
    .unwrap();
    tree.insert(&[1.0, 0.0]).unwrap();
    let before = tree.leaves();
    for vector in [
        vec![0.0, 1.0],
        vec![0.0, 0.0],
        vec![f64::NAN, 1.0],
        vec![1.0],
        vec![f64::INFINITY, 0.0],
    ] {
        assert!(tree.insert(&vector).is_err());
        assert_eq!(tree.leaves(), before);
    }
    assert_eq!(tree.insert(&[2.0, 0.0]).unwrap(), TopicId(0));
    assert_eq!(tree.leaves()[0].2, 2);
}

#[test]
fn overlapping_cosine_assignment_requires_absolute_and_relative_thresholds() {
    let model = AssignmentModel::new(
        2,
        vec![(TopicId(0), vec![1.0, 0.0]), (TopicId(1), vec![0.0, 1.0])],
    )
    .unwrap();
    let hits = model.assign(&[1.0, 1.0], 0.65, 0.9).unwrap();
    assert_eq!(
        hits.iter().map(|a| a.topic).collect::<Vec<_>>(),
        [TopicId(0), TopicId(1)]
    );
    assert!(model.assign(&[-1.0, -1.0], 0.5, 0.9).unwrap().is_empty());
    assert_eq!(model.assign(&[1.0, 0.2], 0.1, 0.9).unwrap().len(), 1);
    assert!(model.assign(&[1.0], 0.5, 0.9).is_err());
    assert!(model.assign(&[1.0, 0.0], f64::NAN, 0.9).is_err());
    assert!(model.assign(&[1.0, 0.0], 0.5, 1.1).is_err());
    assert!(AssignmentModel::new(2, vec![(TopicId(0), vec![0.0, 0.0])]).is_err());
}

#[test]
fn normalized_coordinates_are_scale_invariant_including_extreme_finite_values() {
    assert_eq!(
        normalize(&[f64::MAX, f64::MAX], 2).unwrap(),
        normalize(&[1.0, 1.0], 2).unwrap()
    );
    assert_eq!(normalize(&[f64::MIN_POSITIVE, 0.0], 2).unwrap(), [1.0, 0.0]);
    for config in [
        Settings {
            dimensions: 0,
            ..settings()
        },
        Settings {
            radius: f64::NAN,
            ..settings()
        },
        Settings {
            radius: 1.0,
            ..settings()
        },
        Settings {
            branching: 1,
            ..settings()
        },
        Settings {
            max_topics: 0,
            ..settings()
        },
    ] {
        assert!(Tree::new(config).is_err());
    }
}

#[test]
fn capacity_rejection_preserves_split_tree_and_existing_assignments() {
    let mut tree = Tree::new(Settings {
        max_topics: 20,
        ..settings()
    })
    .unwrap();
    let vectors: Vec<_> = (0..20)
        .map(|i| {
            let angle = f64::from(i) * 0.3;
            [angle.cos(), angle.sin()]
        })
        .collect();
    for vector in &vectors {
        tree.insert(vector).unwrap();
    }
    let before = tree.leaves();
    let depth = tree.depth();
    let model = AssignmentModel::new(
        2,
        before
            .iter()
            .map(|(id, vector, _)| (*id, vector.clone()))
            .collect(),
    )
    .unwrap();
    let assignments: Vec<_> = vectors
        .iter()
        .map(|vector| {
            model
                .assign(vector, 0.99, 0.99)
                .unwrap()
                .into_iter()
                .map(|a| a.topic)
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(tree.insert(&[6.15_f64.cos(), 6.15_f64.sin()]).is_err());
    assert_eq!(tree.leaves(), before);
    assert_eq!(tree.depth(), depth);
    let after = AssignmentModel::new(
        2,
        tree.leaves()
            .into_iter()
            .map(|(id, vector, _)| (id, vector))
            .collect(),
    )
    .unwrap();
    for (vector, expected) in vectors.iter().zip(assignments) {
        assert_eq!(
            after
                .assign(vector, 0.99, 0.99)
                .unwrap()
                .into_iter()
                .map(|a| a.topic)
                .collect::<Vec<_>>(),
            expected
        );
    }
    tree.insert(vectors.last().unwrap()).unwrap();
    assert_eq!(
        tree.leaves().iter().map(|(_, _, count)| count).sum::<u64>(),
        21
    );
}

#[test]
fn class_tfidf_labels_promote_specific_terms_and_keep_deterministic_ties() {
    let classes = std::collections::BTreeMap::from([
        (
            TopicId(0),
            labels::terms("the database database timeout and common common"),
        ),
        (
            TopicId(1),
            labels::terms("the frontend frontend layout and common common"),
        ),
    ]);
    let result = labels::labels(&classes, 3);
    assert_eq!(result[&TopicId(0)][0].text, "database");
    assert_eq!(result[&TopicId(1)][0].text, "frontend");
    assert!(labels::labels(&std::collections::BTreeMap::new(), 5).is_empty());
    assert!(result.values().flatten().all(|term| term.score.is_finite()));
}
