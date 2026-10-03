use super::*;
use crate::silver::enrichment::{
    Boundary, Evidence, TimestampField, TimestampOrigin, Timestamps, VERSION,
};

fn native(field: TimestampField) -> TimestampOrigin {
    TimestampOrigin::SourceMetadata {
        field,
        rule_version: VERSION,
    }
}

fn evidence(path: &Path) -> (Conversation, Timestamps) {
    let parsed = parse(path).unwrap().conversation.unwrap();
    let captured = Evidence::capture(&parsed).unwrap();
    assert!(captured.is_current_for(Provider::GeminiCli));
    (parsed, captured.timestamps.unwrap())
}

#[test]
fn native_timestamp_fields_win_over_equal_or_later_messages_and_follow_recorded_updates() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    for updated in ["2026-09-30T10:00:01Z", "2026-09-30T09:00:00.123Z"] {
        write(
            &path,
            &[
                header(),
                message("message", "user", json!("payload")),
                json!({"$set":{"lastUpdated":updated}}),
            ],
        );
        let (parsed, times) = evidence(&path);
        assert_eq!(parsed.updated_at, Some(timestamp(updated).unwrap()));
        assert_eq!(
            times,
            Timestamps {
                created_at: Some(native(TimestampField::SessionStartTime)),
                updated_at: Some(native(TimestampField::SessionLastUpdated)),
            }
        );
        let wire = serde_json::to_value(&times).unwrap();
        assert_eq!(wire["created_at"]["field"], "session.startTime");
        assert_eq!(wire["updated_at"]["field"], "session.lastUpdated");
        assert_eq!(serde_json::from_value::<Timestamps>(wire).unwrap(), times);
    }
}

#[test]
fn missing_native_fields_select_stable_canonical_boundaries_independent_of_arrival_order() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    let mut expected = None;
    for order in [["a", "z"], ["z", "a"]] {
        let mut start = header();
        start.as_object_mut().unwrap().remove("startTime");
        start.as_object_mut().unwrap().remove("lastUpdated");
        write(
            &path,
            &[
                start,
                message(order[0], "user", json!("payload")),
                message(order[1], "user", json!("payload")),
            ],
        );
        let (parsed, times) = evidence(&path);
        let selected = parsed
            .messages
            .inline()
            .iter()
            .map(|m| &m.id)
            .min()
            .unwrap();
        for (origin, selection) in [
            (&times.created_at, Boundary::Minimum),
            (&times.updated_at, Boundary::Maximum),
        ] {
            assert_eq!(
                origin.as_ref().unwrap(),
                &TimestampOrigin::MessageBoundary {
                    message_id: selected.clone(),
                    selection,
                    rule_version: VERSION,
                }
            );
        }
        assert_eq!(parsed.created_at, parsed.updated_at);
        if let Some(previous) = &expected {
            assert_eq!(previous, &times);
        }
        expected = Some(times);
    }
}

#[test]
fn message_fallback_uses_time_extrema_rather_than_record_order() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    let mut start = header();
    start.as_object_mut().unwrap().remove("startTime");
    start.as_object_mut().unwrap().remove("lastUpdated");
    let mut early = message("early", "user", json!("early payload"));
    early["timestamp"] = json!("2026-09-30T08:00:00.123Z");
    let mut late = message("late", "user", json!("late payload"));
    late["timestamp"] = json!("2026-09-30T12:00:00.456Z");
    write(&path, &[start, late, early]);
    let (parsed, times) = evidence(&path);
    assert_eq!(
        parsed.created_at,
        Some(timestamp("2026-09-30T08:00:00.123Z").unwrap())
    );
    assert_eq!(
        parsed.updated_at,
        Some(timestamp("2026-09-30T12:00:00.456Z").unwrap())
    );
    for (origin, index, selection) in [
        (times.created_at, 1, Boundary::Minimum),
        (times.updated_at, 0, Boundary::Maximum),
    ] {
        assert_eq!(
            origin,
            Some(TimestampOrigin::MessageBoundary {
                message_id: parsed.messages.inline()[index].id.clone(),
                selection,
                rule_version: VERSION,
            })
        );
    }
}

#[test]
fn empty_history_preserves_missing_timestamps_and_start_time_fallback() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    for (start_present, updated_present) in [(false, false), (true, false), (false, true)] {
        let mut start = header();
        if !start_present {
            start.as_object_mut().unwrap().remove("startTime");
        }
        if !updated_present {
            start.as_object_mut().unwrap().remove("lastUpdated");
        }
        write(&path, &[start]);
        let (parsed, times) = evidence(&path);
        assert_eq!(parsed.created_at.is_some(), start_present);
        assert_eq!(
            parsed.updated_at.is_some(),
            start_present || updated_present
        );
        assert_eq!(
            times.created_at,
            start_present.then(|| native(TimestampField::SessionStartTime))
        );
        assert_eq!(
            times.updated_at,
            if updated_present {
                Some(native(TimestampField::SessionLastUpdated))
            } else {
                start_present.then(|| native(TimestampField::SessionStartTime))
            }
        );
    }
}

#[test]
fn legacy_timestamp_evidence_remains_readable_but_explicit_replay_repairs_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    write(
        &path,
        &[header(), message("request", "user", json!("payload"))],
    );
    crate::silver::tests::timestamps::assert_replay(
        Provider::GeminiCli,
        &SourceIdentity::file(&path),
        &options(&path),
    );
}
