use super::source::{Origin, TextKind};
use super::*;

fn stored(text: &str, ordinal: i64) -> Source {
    Source {
        text: text.into(),
        origin: Origin::Stored {
            message_id: format!("message-{ordinal}"),
            ordinal,
            canonical_hash: *blake3::hash(text.as_bytes()).as_bytes(),
        },
        kind: TextKind::Canonical,
    }
}

#[test]
fn database_sources_fingerprint_canonical_content_separately_from_projection() {
    let connection = rusqlite::Connection::open_in_memory().unwrap();
    let source = connection
        .query_row(
            "SELECT 'full canonical request' AS canonical_content, 'message' AS message_id,
                7 AS ordinal, true AS stored_projection",
            [],
            |row| Source::stored(row, "stored search projection".into()),
        )
        .unwrap();
    let projected = source.projected("topic-only text".into());
    let mut remaining = 100;
    let reference = source::Reference::capture(&projected, &mut remaining);
    let Origin::Stored {
        message_id,
        ordinal,
        canonical_hash,
    } = reference.origin
    else {
        panic!("stored source must retain its canonical identity");
    };
    assert_eq!(message_id, "message");
    assert_eq!(ordinal, 7);
    assert_eq!(
        canonical_hash,
        *blake3::hash(b"full canonical request").as_bytes()
    );
    assert_eq!(
        reference.derived_hash,
        *blake3::hash(b"topic-only text").as_bytes()
    );
    assert!(matches!(source.kind, TextKind::SearchProjection));
    assert!(matches!(reference.text_kind, TextKind::TopicProjection));
}

#[test]
fn captured_evidence_preserves_assignments_and_points_to_retained_sources() {
    let mut thread = super::super::anchored::thread(&[0, 0, 1]);
    thread.user_sources = vec![
        stored("database", 0),
        stored("database failure", 2).projected("derived database evidence".into()),
        stored("frontend", 4),
    ];
    let threads = BTreeMap::from([("thread".into(), thread)]);
    let selection = Selection::parse("threads = ['thread']").unwrap();
    let plain = super::super::run(
        &threads,
        "model",
        Mode::UserRepresentatives,
        0.2,
        false,
        false,
        None,
    )
    .unwrap();
    let captured = super::super::run(
        &threads,
        "model",
        Mode::UserRepresentatives,
        0.2,
        false,
        false,
        Some(&selection),
    )
    .unwrap();
    assert_eq!(plain.memberships, captured.memberships);
    assert!(plain.assignment_evidence.is_none());
    let report = captured.assignment_evidence.unwrap();
    let review = &report.threads["thread"];
    assert_eq!(review.source_messages_total, 3);
    assert_eq!(review.topics.len(), 2);
    assert_eq!(
        review
            .topics
            .iter()
            .map(|t| t.supporting_messages)
            .sum::<usize>(),
        3
    );
    let db = review
        .topics
        .iter()
        .find(|t| t.supporting_messages == 2)
        .unwrap();
    assert_eq!(db.samples[0].score.start, 0);
    assert_eq!(db.samples[0].score.end, 2);
    assert_eq!(db.samples[0].score.support, 2.0);
    assert_eq!(db.samples[0].score.weight, 1.0);
    assert!(db.samples[0].complete_derived_text);
    assert!(db.samples[0].complete_canonical_references);
    assert!(matches!(
        review.sources[1].origin,
        Origin::Stored { ordinal: 2, .. }
    ));
    assert!(matches!(
        review.sources[1].text_kind,
        TextKind::TopicProjection
    ));
    let baseline = super::super::run(
        &threads,
        "model",
        Mode::ThreadMean,
        0.2,
        false,
        false,
        Some(&selection),
    )
    .unwrap();
    assert!(matches!(
        baseline.assignment_evidence.unwrap().state,
        State::UnsupportedBaseline
    ));
}

#[test]
fn source_budgets_unicode_prefixes_and_missing_grounding_are_explicit() {
    let selection = Selection::parse("threads = ['thread']").unwrap();
    let mut collector = Collector::new(&selection, Mode::UserMean);
    collector.text_remaining = 5;
    collector.refs_remaining = 1;
    let mut thread = super::super::anchored::thread(&[0, 1]);
    thread.user_sources = vec![stored("é😀remaining", 0), "synthetic".into()];
    let mut scores = Scores::default();
    scores.add(SegmentScore::new(0, 0, 2, 0.8, 1.6, 1.0));
    collector.record("thread", &thread, vec![(TopicId(0), 1.6, scores)]);
    let report = collector.finish();
    let evidence = &report.threads["thread"];
    assert_eq!(evidence.sources.len(), 1);
    assert_eq!(evidence.sources[0].text, "é");
    assert!(!evidence.sources[0].text_complete);
    assert_eq!(evidence.source_messages_total, 2);
    assert_eq!(evidence.omitted_sources, 1);
    assert_eq!(report.text_bytes, 2);
    assert_eq!(report.source_references, 1);
    assert!(!evidence.topics[0].samples[0].complete_canonical_references);
    assert!(!evidence.topics[0].samples[0].complete_derived_text);
    let mut remaining = 100;
    assert!(!source::Reference::capture(&thread.user_sources[1], &mut remaining).stored());
}

#[test]
fn retained_score_sampling_and_selection_fail_closed() {
    let mut scores = Scores::default();
    for index in 0..10 {
        scores.add(SegmentScore::new(index, index, index + 1, 0.9, 0.9, 1.0));
    }
    assert_eq!(scores.segments, 10);
    assert_eq!(scores.messages, 10);
    assert_eq!(
        scores
            .top
            .iter()
            .map(|s| s.segment_index)
            .collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    for text in [
        "threads=[]",
        "threads=['a','a']",
        "threads=['']",
        "threads=['a']\nunknown=true",
    ] {
        assert!(Selection::parse(text).is_err());
    }
    let selection = Selection::parse("threads=['missing']").unwrap();
    assert!(selection.validate(&BTreeMap::new()).is_err());
    let mut thread = super::super::anchored::thread(&[0]);
    thread.user_sources.clear();
    assert!(Collector::validate(&thread).is_err());
}

#[test]
fn topic_cap_and_segment_sampling_report_what_was_omitted() {
    let selection = Selection::parse("threads=['thread']").unwrap();
    let mut collector = Collector::new(&selection, Mode::UserSegments);
    let thread = super::super::anchored::thread(&[0; 10]);
    let scores = (0..70)
        .map(|topic| {
            let mut scores = Scores::default();
            for index in 0..10 {
                scores.add(SegmentScore::new(index, index, index + 1, 0.9, 0.9, 1.0));
            }
            (TopicId(topic), 9.0, scores)
        })
        .collect();
    collector.record("thread", &thread, scores);
    let report = collector.finish();
    let evidence = &report.threads["thread"];
    assert_eq!(evidence.topics.len(), 64);
    assert_eq!(evidence.omitted_topics, 6);
    assert_eq!(evidence.retained_topics_total, 70);
    for topic in &evidence.topics {
        assert_eq!(topic.matching_segments, 10);
        assert_eq!(topic.supporting_messages, 10);
        assert_eq!(topic.samples.len(), 4);
        assert_eq!(topic.omitted_segments, 6);
        assert!(!topic.samples[0].complete_canonical_references);
    }
}

#[test]
fn contextual_origins_share_the_reference_budget_without_losing_alignment() {
    let user = stored("what is this pipeline?", 0);
    let reply = stored(&"deadlock detection and runtime validation ".repeat(16), 1);
    let contextual = user.with_context(&user, &reply, true).unwrap();
    let selection = Selection::parse("threads=['thread']").unwrap();
    let mut thread = super::super::anchored::thread(&[0]);
    thread.user_sources = vec![contextual];
    for budget in [1, 2] {
        let mut collector = Collector::new(&selection, Mode::UserSegments);
        collector.refs_remaining = budget;
        let mut scores = Scores::default();
        scores.add(SegmentScore::new(0, 0, 1, 0.9, 0.9, 1.0));
        collector.record("thread", &thread, vec![(TopicId(0), 0.9, scores)]);
        let report = collector.finish();
        let evidence = &report.threads["thread"];
        assert_eq!(evidence.sources.len(), usize::from(budget == 2));
        assert_eq!(report.source_references, if budget == 2 { 2 } else { 0 });
        assert_eq!(evidence.omitted_sources, usize::from(budget == 1));
        assert_eq!(
            evidence.topics[0].samples[0].complete_canonical_references,
            budget == 2
        );
        assert!(!evidence.topics[0].samples[0].complete_derived_text);
        if budget == 2 {
            assert!(matches!(
                report.source_scope,
                SourceScope::ContextualUserRequests
            ));
            assert!(matches!(
                evidence.sources[0].origin,
                Origin::Contextual {
                    assistant_prefix_complete: true,
                    ..
                }
            ));
            assert_eq!(
                evidence.sources[0].derived_hash,
                *blake3::hash(thread.user_sources[0].text.as_bytes()).as_bytes()
            );
        }
    }
}
