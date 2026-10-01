use super::*;
use crate::topics::segments::Segment;

fn axis(index: usize) -> Vec<f64> {
    let mut vector = vec![0.0; 384];
    vector[index] = 1.0;
    vector
}

#[test]
fn conserved_support_rejects_ambiguous_many_way_matches_and_keeps_secondary_subject() {
    let mut thread = super::super::anchored::thread(&[1, 1, 1, 1, 14, 0]);
    for message in &mut thread.user_messages[..4] {
        message.words = labels::terms("database");
    }
    thread.user_messages[4].words = labels::terms("frontend");
    let mut centroids = (0..12)
        .map(|index| {
            let mut vector = vec![0.0; 384];
            vector[0] = 0.7;
            vector[index + 2] = 0.51_f64.sqrt();
            (TopicId(u64::try_from(index).unwrap()), vector)
        })
        .collect::<Vec<_>>();
    centroids.extend([(TopicId(12), axis(1)), (TopicId(13), axis(14))]);
    let model = AssignmentModel::new(384, centroids).unwrap();
    let independent = evidence(
        &thread,
        &model,
        Mode::UserSegments,
        Weighting::Independent,
        false,
    )
    .unwrap();
    assert_eq!(independent.len(), 14);
    let conserved = evidence(
        &thread,
        &model,
        Mode::UserSegments,
        Weighting::Conserved,
        true,
    )
    .unwrap();
    assert_eq!(
        conserved.keys().copied().collect::<Vec<_>>(),
        [TopicId(12), TopicId(13)]
    );
    assert_eq!(conserved[&TopicId(12)].support, 4.0);
    assert_eq!(conserved[&TopicId(13)].support, 1.0);
    assert_eq!(conserved[&TopicId(13)].words, labels::terms("frontend"));
}

#[test]
fn one_multisubject_segment_preserves_both_topics_and_conserves_evidence_mass() {
    let mut thread = super::super::anchored::thread(&[0]);
    let mut vector = axis(0);
    vector[1] = 1.0;
    thread.user_messages[0] = Segment {
        vector,
        words: labels::terms("database frontend"),
        messages: 1,
    };
    let model =
        AssignmentModel::new(384, vec![(TopicId(0), axis(0)), (TopicId(1), axis(1))]).unwrap();
    let evidence = evidence(
        &thread,
        &model,
        Mode::UserSegments,
        Weighting::Conserved,
        true,
    )
    .unwrap();
    assert_eq!(evidence.len(), 2);
    let mass = evidence.values().map(|entry| entry.support).sum::<f64>();
    assert!((mass - 0.5_f64.sqrt()).abs() < 1e-12);
    assert!(mass <= 1.0);
    assert!((evidence[&TopicId(0)].support - evidence[&TopicId(1)].support).abs() < 1e-12);
    assert!(evidence.values().all(|entry| entry.scores.is_some()));
}
