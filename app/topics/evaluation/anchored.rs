use super::{Mode, Segment, Thread, labels, run};
use crate::provider::Provider;
use std::collections::BTreeMap;

pub(super) fn thread(axes: &[usize]) -> Thread {
    let user_messages: Vec<_> = axes
        .iter()
        .map(|axis| {
            let mut vector = vec![0.0; 384];
            vector[*axis] = 1.0;
            Segment {
                vector,
                words: labels::terms(if *axis == 0 { "database" } else { "frontend" }),
                messages: 1,
            }
        })
        .collect();
    Thread {
        provider: Provider::Codex,
        title: String::new(),
        vectors: user_messages.iter().map(|m| m.vector.clone()).collect(),
        words: labels::Terms::new(),
        user_messages,
        user_sources: axes.iter().map(|_| "database frontend".into()).collect(),
        ignored_user_messages: 0,
    }
}

#[test]
fn thread_discovery_with_segment_assignment_preserves_secondary_topic_matches() {
    let threads = BTreeMap::from([
        ("database".into(), thread(&[0])),
        ("frontend".into(), thread(&[1])),
        ("mixed".into(), thread(&[0, 1])),
    ]);
    let report = run(&threads, "model-v1", Mode::UserAnchored, 0.2, false, false).unwrap();
    assert_eq!(report.samples, 3); // includes mixed thread's mean, not two repeated messages
    assert_eq!(report.memberships["database"].len(), 1);
    assert_eq!(report.memberships["frontend"].len(), 1);
    assert_ne!(
        report.memberships["database"],
        report.memberships["frontend"]
    );
    assert_eq!(
        report.memberships["mixed"],
        report.memberships["database"]
            .union(&report.memberships["frontend"])
            .copied()
            .collect()
    );
    let database_id = report.memberships["database"].first().unwrap();
    let database = report
        .clusters
        .iter()
        .find(|cluster| &cluster.id == database_id)
        .unwrap();
    assert_eq!(database.threads, 2);
    assert!(database.terms.iter().any(|term| term.text == "database"));
    assert!(!database.terms.iter().any(|term| term.text == "frontend"));
}

#[test]
fn representative_discovery_covers_a_mixed_thread_without_single_topic_founders() {
    let threads = BTreeMap::from([("mixed".into(), thread(&[0, 1]))]);
    let report = run(
        &threads,
        "model-v1",
        Mode::UserRepresentatives,
        0.2,
        false,
        false,
    )
    .unwrap();
    assert_eq!(report.samples, 2);
    assert_eq!(report.memberships["mixed"].len(), 2);
    // A whole-thread mean is below the assignment threshold of either subject
    // when the mixed thread covers four unrelated axes.
    let threads = BTreeMap::from([("mixed".into(), thread(&[0, 1, 2, 3]))]);
    let anchored = run(&threads, "model-v1", Mode::UserAnchored, 0.2, false, false).unwrap();
    assert!(anchored.memberships["mixed"].is_empty());
    let representatives = run(
        &threads,
        "model-v1",
        Mode::UserRepresentatives,
        0.2,
        false,
        false,
    )
    .unwrap();
    assert_eq!(representatives.memberships["mixed"].len(), 4);
}
