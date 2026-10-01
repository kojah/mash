//! Bounded discovery representatives for threads containing several subjects.
use super::{Segment, normalize};
use crate::AppError;

/// Prefer supported segments, then supported novelty. Keep at most `maximum`
/// representatives and stop when every remaining segment is already similar
/// to a selected representative. No all-pairs matrix or synthetic mean is needed.
pub(super) fn select(
    segments: &[Segment],
    dimensions: usize,
    maximum: usize,
    similarity: f64,
) -> Result<Vec<Segment>, AppError> {
    if !(1..=8).contains(&maximum)
        || !(1..=2048).contains(&dimensions)
        || !similarity.is_finite()
        || !(0.0..=1.0).contains(&similarity)
    {
        return Err(AppError::usage("invalid topic representative settings"));
    }
    let vectors = segments
        .iter()
        .map(|s| {
            if !(1..=128).contains(&s.messages) {
                return Err(AppError::usage("invalid topic segment support"));
            }
            normalize(&s.vector, dimensions)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut nearest = vec![-1.0_f64; segments.len()];
    let mut selected = Vec::new();
    for _ in 0..maximum {
        let best = (0..segments.len())
            .filter(|i| nearest[*i] < similarity)
            .max_by(|a, b| {
                // Segment support is validated <=128, exactly representable.
                let score = |i: usize| {
                    f64::from(u32::try_from(segments[i].messages).expect("bounded support"))
                        * (1.0 - nearest[i]).max(0.0)
                };
                score(*a).total_cmp(&score(*b)).then_with(|| {
                    // Canonical tie breaking is independent of source order.
                    vectors[*b]
                        .iter()
                        .zip(&vectors[*a])
                        .find_map(|(b, a)| {
                            let order = b.total_cmp(a);
                            (!order.is_eq()).then_some(order)
                        })
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            });
        let Some(best) = best else { break };
        let mut segment = segments[best].clone();
        segment.vector = vectors[best].clone();
        selected.push(segment);
        for (i, vector) in vectors.iter().enumerate() {
            let cosine = vector
                .iter()
                .zip(&vectors[best])
                .map(|(a, b)| a * b)
                .sum::<f64>();
            nearest[i] = nearest[i].max(cosine);
        }
        // Do not select an already chosen segment because of roundoff.
        nearest[best] = 1.0;
    }
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topics::labels::Terms;

    fn segment(vector: &[f64], messages: usize) -> Segment {
        Segment {
            vector: vector.to_vec(),
            words: Terms::new(),
            messages,
        }
    }

    #[test]
    fn representatives_preserve_distinct_subjects_without_repeated_anchors() {
        let segments = [
            segment(&[1.0, 0.0, 0.0], 8),
            segment(&[2.0, 0.0, 0.0], 4),
            segment(&[0.0, 1.0, 0.0], 3),
            segment(&[0.0, 0.0, 1.0], 1),
        ];
        let selected = select(&segments, 3, 4, 0.65).unwrap();
        assert_eq!(selected.len(), 3);
        assert_eq!(selected[0].messages, 8);
        assert_eq!(selected[1].messages, 3);
        assert_eq!(selected[2].messages, 1);
        assert_eq!(select(&segments, 3, 2, 0.65).unwrap().len(), 2);
        assert!(select(&[], 3, 4, 0.65).unwrap().is_empty());
    }

    #[test]
    fn representatives_break_ties_canonically_and_reject_invalid_input() {
        let mut segments = vec![segment(&[1.0, 0.0], 1), segment(&[0.0, 1.0], 1)];
        let first = select(&segments, 2, 1, 0.65).unwrap()[0].vector.clone();
        segments.reverse();
        assert_eq!(select(&segments, 2, 1, 0.65).unwrap()[0].vector, first);
        assert!(select(&segments, 2, 0, 0.65).is_err());
        assert!(select(&segments, 2, 4, f64::NAN).is_err());
        assert!(select(&[segment(&[0.0, 0.0], 1)], 2, 4, 0.65).is_err());
        assert!(select(&[segment(&[1.0, 0.0], 0)], 2, 4, 0.65).is_err());
        assert!(select(&[segment(&[1.0, 0.0], 129)], 2, 4, 0.65).is_err());
    }
}
