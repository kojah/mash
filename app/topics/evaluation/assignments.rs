//! Keep labels tied to supporting segments, with one word contribution per thread.
use super::{AssignmentModel, BTreeMap, BTreeSet, Mode, Thread, TopicId, labels, thread_samples};
use crate::AppError;

#[derive(Default)]
struct Evidence {
    support: f64,
    words: labels::Terms,
    scores: Option<super::provenance::Scores>,
}
pub(super) struct Assignments {
    pub(super) memberships: BTreeMap<String, BTreeSet<TopicId>>,
    pub(super) classes: BTreeMap<TopicId, labels::Terms>,
    pub(super) supporting_words: BTreeMap<String, BTreeMap<TopicId, labels::Terms>>,
    pub(super) assignment_evidence: Option<super::provenance::Report>,
}

fn evidence(
    thread: &Thread,
    model: &AssignmentModel,
    mode: Mode,
    capture: bool,
) -> Result<BTreeMap<TopicId, Evidence>, AppError> {
    if capture {
        super::provenance::Collector::validate(thread)?;
    }
    let mut evidence: BTreeMap<TopicId, Evidence> = BTreeMap::new();
    let (minimum, relative) = if mode.segmented() {
        (0.65, 0.92)
    } else {
        (0.55, 0.9)
    };
    let mut offset = 0;
    for (index, segment) in thread_samples(thread, mode).into_iter().enumerate() {
        let end = offset + segment.messages;
        if capture && end > thread.user_sources.len() {
            return Err(AppError::usage(
                "topic evidence segment exceeds source range",
            ));
        }
        for hit in model.assign(&segment.vector, minimum, relative)? {
            let entry = evidence.entry(hit.topic).or_default();
            #[allow(clippy::cast_precision_loss)]
            {
                let support = hit.cosine * segment.messages as f64;
                entry.support += support;
                if capture {
                    entry
                        .scores
                        .get_or_insert_with(super::provenance::Scores::default)
                        .add(super::provenance::SegmentScore::new(
                            index, offset, end, hit.cosine, support,
                        ));
                }
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
        offset = end;
    }
    if capture && offset != thread.user_sources.len() {
        return Err(AppError::usage(
            "topic evidence does not cover source sequence",
        ));
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
    mut collector: Option<super::provenance::Collector<'_>>,
) -> Result<Assignments, AppError> {
    let mut memberships = BTreeMap::new();
    let mut classes = BTreeMap::new();
    let mut supporting_words = BTreeMap::new();
    let capture = std::env::var_os("MASH_TEST_TOPIC_HIERARCHY").is_some();
    for (id, thread) in threads {
        let mut topics = BTreeSet::new();
        let mut words = BTreeMap::new();
        let selected = collector.as_ref().is_some_and(|c| c.selected(id));
        let mut scores = Vec::new();
        for (topic, entry) in evidence(thread, model, mode, selected)? {
            topics.insert(topic);
            if capture {
                words.insert(topic, entry.words.clone());
            }
            if let Some(review) = entry.scores {
                scores.push((topic, entry.support, review));
            }
            let class = classes.entry(topic).or_insert_with(labels::Terms::new);
            for word in entry.words.into_keys() {
                *class.entry(word).or_default() += 1;
            }
        }
        if selected {
            collector.as_mut().unwrap().record(id, thread, scores);
        }
        memberships.insert(id.clone(), topics);
        if capture {
            supporting_words.insert(id.clone(), words);
        }
    }
    Ok(Assignments {
        memberships,
        classes,
        supporting_words,
        assignment_evidence: collector.map(super::provenance::Collector::finish),
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
            source_id: super::partition::identity(&["database frontend incidental".into()]),
            provider: Provider::Codex,
            title: String::new(),
            vectors: Vec::new(),
            words: labels::Terms::new(),
            user_messages,
            user_sources: (0..15).map(|_| "fixture".into()).collect(),
            ignored_user_messages: 0,
        },
    )]);
    let model = AssignmentModel::new(
        384,
        (0..3).map(|i| (TopicId(i as u64), vector(i))).collect(),
    )
    .unwrap();
    let result = assign(&threads, &model, Mode::UserSegments, None).unwrap();
    assert_eq!(
        result.memberships["thread"],
        BTreeSet::from([TopicId(0), TopicId(1)])
    );
    assert_eq!(result.classes[&TopicId(0)], labels::terms("database"));
    assert_eq!(result.classes[&TopicId(1)], labels::terms("frontend"));
    assert!(!result.classes.contains_key(&TopicId(2)));
    let selection = super::provenance::Selection::parse("threads=['thread']").unwrap();
    let captured = assign(
        &threads,
        &model,
        Mode::UserSegments,
        Some(super::provenance::Collector::new(
            &selection,
            Mode::UserSegments,
        )),
    )
    .unwrap();
    assert_eq!(captured.memberships, result.memberships);
    assert_eq!(
        captured.assignment_evidence.unwrap().topic_ids("thread"),
        [TopicId(0), TopicId(1)]
    );
}
