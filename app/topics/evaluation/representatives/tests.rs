use super::*;
use crate::provider::Provider;
use crate::topics::segments::Segment;

fn vector(axis: usize) -> Vec<f64> {
    let mut vector = vec![0.0; 384];
    vector[axis] = 1.0;
    vector
}

fn thread(identity: &str, segments: &[(usize, &str)]) -> Thread {
    Thread {
        source_id: partition::identity(&[identity.into()]),
        provider: Provider::Codex,
        title: String::new(),
        vectors: Vec::new(),
        words: labels::Terms::new(),
        user_messages: segments
            .iter()
            .map(|(axis, text)| Segment {
                vector: vector(*axis),
                words: labels::Terms::from([(text.to_string(), 1)]),
                messages: 1,
            })
            .collect(),
        user_sources: segments.iter().map(|(_, text)| (*text).into()).collect(),
        ignored_user_messages: 0,
    }
}

fn cluster() -> Cluster {
    Cluster {
        id: TopicId(0),
        samples: 1,
        threads: 1,
        providers: BTreeMap::new(),
        name: super::super::names::Name::new(
            vector(1),
            vec![labels::Term {
                text: "database".into(),
                score: 1.0,
                support: 1,
            }],
        ),
        examples: Vec::new(),
    }
}

#[test]
fn representatives_use_only_matching_subjects_and_count_canonical_copies_once() {
    let first = thread("first", &[(0, "database"), (1, "frontend")]);
    let threads: BTreeMap<String, Thread> = BTreeMap::from([
        ("a".into(), first.clone()),
        ("copy".into(), first),
        ("b".into(), thread("second", &[(2, "database")])),
    ]);
    let matching = threads
        .keys()
        .map(|id| (id.clone(), BTreeMap::from([(TopicId(0), vec![0])])))
        .collect();
    let mut clusters = vec![cluster()];
    apply(
        &threads,
        &matching,
        Mode::UserRepresentatives,
        false,
        &mut clusters,
    )
    .unwrap();
    assert_eq!(clusters[0].name.center, vector(1));
    let selection = clusters[0].name.label_representation.as_ref().unwrap();
    assert_eq!(selection.samples.len(), 2);
    assert!(
        selection
            .samples
            .iter()
            .all(|s| s.segment_index == 0 && s.start == 0 && s.end == 1)
    );
    assert!((selection.center[0] - 0.5_f64.sqrt()).abs() < 1e-12);
    assert!((selection.center[2] - 0.5_f64.sqrt()).abs() < 1e-12);
    assert_eq!(selection.center[1], 0.0);
    assert!(
        selection
            .samples
            .iter()
            .all(|s| (s.lexical_cosine - 1.0).abs() < 1e-12)
    );
}

#[test]
fn representative_selection_excludes_reserved_sources_and_validates_before_mutation() {
    let reserved = (0..100)
        .map(|i| thread(&format!("heldout {i}"), &[(0, "database")]))
        .find(partition::held_out)
        .unwrap();
    let threads = BTreeMap::from([("heldout".into(), reserved)]);
    let matching = BTreeMap::from([("heldout".into(), BTreeMap::from([(TopicId(0), vec![0])]))]);
    let mut clusters = vec![cluster()];
    apply(
        &threads,
        &matching,
        Mode::UserRepresentatives,
        true,
        &mut clusters,
    )
    .unwrap();
    assert!(clusters[0].name.label_representation.is_none());
    apply(
        &threads,
        &matching,
        Mode::UserRepresentatives,
        false,
        &mut clusters,
    )
    .unwrap();
    let previous = serde_json::to_vec(&clusters).unwrap();
    let invalid = BTreeMap::from([("heldout".into(), BTreeMap::from([(TopicId(0), vec![10])]))]);
    assert!(
        apply(
            &threads,
            &invalid,
            Mode::UserRepresentatives,
            false,
            &mut clusters
        )
        .is_err()
    );
    assert_eq!(serde_json::to_vec(&clusters).unwrap(), previous);
    assert!(apply(&threads, &matching, Mode::ThreadMean, false, &mut clusters).is_err());
    let mut bad = threads.clone();
    bad.get_mut("heldout").unwrap().source_id = None;
    assert!(
        apply(
            &bad,
            &matching,
            Mode::UserRepresentatives,
            false,
            &mut clusters
        )
        .is_err()
    );
}

#[test]
fn lexical_selection_is_length_normalized_scale_safe_and_bounded() {
    let mut candidates = vec![labels::Term {
        text: "database".into(),
        score: f64::MAX,
        support: 1,
    }];
    let short = labels::Terms::from([("database".into(), 1)]);
    let long = labels::Terms::from([("database".into(), 1), ("unrelated".into(), 1)]);
    assert_eq!(lexical(&short, &candidates).unwrap(), 1.0);
    assert!(lexical(&long, &candidates).unwrap() < 1.0);
    candidates[0].score = f64::NAN;
    assert!(lexical(&short, &candidates).is_err());
    let mut samples = Vec::new();
    for i in 0..10 {
        retain(
            &mut samples,
            Sample {
                source_id: partition::identity(&[format!("source {i}")]).unwrap(),
                thread_id: i.to_string(),
                segment_index: 0,
                start: 0,
                end: 1,
                lexical_cosine: f64::from(i) / 10.0,
                vector: vector(0),
            },
        );
    }
    assert_eq!(samples.len(), PER_TOPIC);
    assert_eq!(samples[0].lexical_cosine, 0.9);
    assert_eq!(samples.last().unwrap().lexical_cosine, 0.6);
}
