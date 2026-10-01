//! Semantic grouping of CF leaves, independent of CF storage topology.
//! Complete-link admission prevents a chain of individually similar neighbors
//! from joining mutually dissimilar endpoints. IDs remain experiment-local.
use super::{TopicId, normalize};
use crate::AppError;
use std::collections::BTreeSet;

struct Group {
    sum: Vec<f64>,
    center: Vec<f64>,
    members: Vec<Vec<f64>>,
    count: u64,
}

fn cosine(a: &[f64], b: &[f64]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(a, b)| a * b)
        .sum::<f64>()
        .clamp(-1.0, 1.0)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "validated total sample count stays within 2^53"
)]
fn add(group: &mut Group, vector: Vec<f64>, count: u64) -> Result<(), AppError> {
    for (sum, value) in group.sum.iter_mut().zip(&vector) {
        *sum += value * count as f64;
    }
    group.center = normalize(&group.sum, vector.len())?;
    group.count += count;
    group.members.push(vector);
    Ok(())
}

/// Greedy complete-link grouping in canonical vector order. Bounded at 8192
/// leaves; worst-case O(leaves^2 * (dimensions + log(leaves))), including
/// candidate sorting, with linear vector storage.
pub(super) fn group(
    dimensions: usize,
    leaves: &[(TopicId, Vec<f64>, u64)],
    minimum: f64,
) -> Result<Vec<(TopicId, Vec<f64>, u64)>, AppError> {
    if !(1..=2048).contains(&dimensions)
        || leaves.len() > 8192
        || !minimum.is_finite()
        || !(0.0..=1.0).contains(&minimum)
    {
        return Err(AppError::usage(
            "invalid experimental semantic grouping settings",
        ));
    }
    let mut inputs = Vec::with_capacity(leaves.len());
    let mut ids = BTreeSet::new();
    let mut count = 0_u64;
    for (id, vector, samples) in leaves {
        count = count
            .checked_add(*samples)
            .ok_or_else(|| AppError::usage("topic sample count overflow"))?;
        if *samples == 0 || count > (1_u64 << 53) || !ids.insert(*id) {
            return Err(AppError::usage(
                "invalid topic leaf identities or sample counts",
            ));
        }
        inputs.push((normalize(vector, dimensions)?, *samples));
    }
    inputs.sort_by(|(a, ac), (b, bc)| {
        a.iter()
            .zip(b)
            .map(|(a, b)| a.total_cmp(b))
            .find(|ordering| !ordering.is_eq())
            .unwrap_or_else(|| ac.cmp(bc))
    });
    let mut groups: Vec<Group> = Vec::new();
    for (vector, count) in inputs {
        let mut candidates: Vec<_> = groups
            .iter()
            .enumerate()
            .map(|(index, group)| (index, cosine(&vector, &group.center)))
            .filter(|(_, score)| *score >= minimum)
            .collect();
        candidates.sort_by(|(a, ascore), (b, bscore)| bscore.total_cmp(ascore).then(a.cmp(b)));
        let destination = candidates.into_iter().find_map(|(index, _)| {
            groups[index]
                .members
                .iter()
                .all(|member| cosine(&vector, member) >= minimum)
                .then_some(index)
        });
        if let Some(index) = destination {
            add(&mut groups[index], vector, count)?;
        } else {
            let mut group = Group {
                sum: vec![0.0; dimensions],
                center: Vec::new(),
                members: Vec::new(),
                count: 0,
            };
            add(&mut group, vector, count)?;
            groups.push(group);
        }
    }
    Ok(groups
        .into_iter()
        .enumerate()
        .map(|(index, g)| {
            (
                TopicId(u64::try_from(index).expect("bounded group count")),
                g.center,
                g.count,
            )
        })
        .collect())
}

#[test]
fn grouping_is_order_independent_and_does_not_chain_dissimilar_endpoints() {
    let point = |angle: f64| vec![angle.cos(), angle.sin()];
    let leaves = vec![
        (TopicId(7), point(0.0), 1),
        (TopicId(3), point(0.6), 1),
        (TopicId(9), point(1.2), 1),
    ];
    let groups = group(2, &leaves, 0.7).unwrap();
    assert_eq!(groups.len(), 2); // neighbors match, endpoints do not
    assert_eq!(groups.iter().map(|(_, _, count)| count).sum::<u64>(), 3);
    let reversed: Vec<_> = leaves.into_iter().rev().collect();
    assert_eq!(groups, group(2, &reversed, 0.7).unwrap());
    assert!(group(2, &[(TopicId(0), vec![1.0, 0.0], 0)], 0.7).is_err());
    assert!(group(2, &[(TopicId(0), vec![1.0], 1)], 0.7).is_err());
    assert!(group(2, &[(TopicId(0), vec![0.0, 0.0], 1)], 0.7).is_err());
    assert!(group(2, &[], f64::NAN).is_err());
    assert!(
        group(
            2,
            &[
                (TopicId(0), vec![1.0, 0.0], 1),
                (TopicId(0), vec![0.0, 1.0], 1)
            ],
            0.7
        )
        .is_err()
    );
}

#[test]
fn grouping_preserves_sample_weight_and_rejects_unrepresentable_counts() {
    let leaves = [
        (TopicId(0), vec![1.0, 0.0], 3),
        (TopicId(1), vec![0.8, 0.6], 1),
    ];
    let groups = group(2, &leaves, 0.75).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].2, 4);
    let expected = normalize(&[3.8, 0.6], 2).unwrap();
    for (actual, expected) in groups[0].1.iter().zip(expected) {
        assert!((actual - expected).abs() < 1e-12);
    }
    assert!(group(2, &[(TopicId(0), vec![1.0, 0.0], 1_u64 << 54)], 0.75).is_err());
    assert!(group(2, &[(TopicId(0), vec![1.0, 0.0], u64::MAX)], 0.75).is_err());
    assert!(group(0, &[], 0.75).is_err());
}
