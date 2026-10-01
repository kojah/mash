//! Keep labels tied to supporting segments, with one word contribution per thread.
use super::{AssignmentModel, BTreeMap, BTreeSet, Mode, Thread, TopicId, labels, thread_samples};
use crate::AppError;

#[derive(Default)]
struct Evidence {
    support: f64,
    words: labels::Terms,
}
pub(super) struct Assignments {
    pub(super) memberships: BTreeMap<String, BTreeSet<TopicId>>,
    pub(super) classes: BTreeMap<TopicId, labels::Terms>,
}

fn evidence(
    thread: &Thread,
    model: &AssignmentModel,
    mode: Mode,
) -> Result<BTreeMap<TopicId, Evidence>, AppError> {
    let mut evidence: BTreeMap<TopicId, Evidence> = BTreeMap::new();
    let (minimum, relative) = if mode.segmented() {
        (0.65, 0.92)
    } else {
        (0.55, 0.9)
    };
    for segment in thread_samples(thread, mode) {
        for hit in model.assign(&segment.vector, minimum, relative)? {
            let entry = evidence.entry(hit.topic).or_default();
            #[allow(clippy::cast_precision_loss)]
            {
                entry.support += hit.cosine * segment.messages as f64;
            }
            let words = if matches!(mode, Mode::Segments) {
                &thread.words
            } else {
                &segment.words
            };
            for word in words.keys() {
                entry.words.insert(word.clone(), 1);
            }
        }
    }
    let best = evidence
        .values()
        .map(|entry| entry.support)
        .fold(0.0, f64::max);
    if mode.segmented() {
        evidence.retain(|_, entry| entry.support >= best * 0.1);
    }
    Ok(evidence)
}

pub(super) fn assign(
    threads: &BTreeMap<String, Thread>,
    model: &AssignmentModel,
    mode: Mode,
) -> Result<Assignments, AppError> {
    let mut memberships = BTreeMap::new();
    let mut classes = BTreeMap::new();
    for (id, thread) in threads {
        let mut topics = BTreeSet::new();
        for (topic, entry) in evidence(thread, model, mode)? {
            topics.insert(topic);
            let class = classes.entry(topic).or_insert_with(labels::Terms::new);
            for word in entry.words.into_keys() {
                *class.entry(word).or_default() += 1;
            }
        }
        memberships.insert(id.clone(), topics);
    }
    Ok(Assignments {
        memberships,
        classes,
    })
}

#[test]
fn segment_labels_exclude_unrelated_terms_and_weak_incidental_topics() {
    use crate::provider::Provider;
    use crate::topics::segments::Segment;
    let vector = |axis: usize| {
        let mut v = vec![0.0; 384];
        v[axis] = 1.0;
        v
    };
    let mut user_messages = Vec::new();
    for (axis, word, count) in [
        (0, "database", 12),
        (1, "frontend", 2),
        (2, "incidental", 1),
    ] {
        user_messages.extend((0..count).map(|_| Segment {
            vector: vector(axis),
            words: labels::terms(word),
            messages: 1,
        }));
    }
    let threads = BTreeMap::from([(
        "thread".into(),
        Thread {
            provider: Provider::Codex,
            title: String::new(),
            vectors: Vec::new(),
            words: labels::Terms::new(),
            user_messages,
            ignored_user_messages: 0,
        },
    )]);
    let model = AssignmentModel::new(
        384,
        (0..3).map(|i| (TopicId(i as u64), vector(i))).collect(),
    )
    .unwrap();
    let result = assign(&threads, &model, Mode::UserSegments).unwrap();
    assert_eq!(
        result.memberships["thread"],
        BTreeSet::from([TopicId(0), TopicId(1)])
    );
    assert_eq!(result.classes[&TopicId(0)], labels::terms("database"));
    assert_eq!(result.classes[&TopicId(1)], labels::terms("frontend"));
    assert!(!result.classes.contains_key(&TopicId(2)));
}
