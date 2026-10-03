use super::*;
use crate::silver::enrichment::{
    Boundary, Evidence, TimestampField, TimestampOrigin, Timestamps, VERSION,
};

fn header_origin() -> TimestampOrigin {
    TimestampOrigin::SourceMetadata {
        field: TimestampField::SessionTimestamp,
        rule_version: VERSION,
    }
}

#[test]
fn equal_message_times_choose_stable_canonical_id_independently_of_arrival_order() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    let mut selected = None;
    for order in [["a", "z"], ["z", "a"]] {
        let mut file = File::create(&path).unwrap();
        writeln!(file, "{}", header()).unwrap();
        for id in order {
            writeln!(file, "{}", message(id, None, "user", "payload")).unwrap();
        }
        drop(file);
        let parsed = parse(&path).unwrap().conversation.unwrap();
        let minimum = parsed
            .messages
            .inline()
            .iter()
            .map(|m| &m.id)
            .min()
            .unwrap();
        let origin = Evidence::capture(&parsed)
            .unwrap()
            .timestamps
            .unwrap()
            .updated_at
            .unwrap();
        assert_eq!(
            origin,
            TimestampOrigin::MessageBoundary {
                message_id: minimum.clone(),
                selection: Boundary::Maximum,
                rule_version: VERSION
            }
        );
        if let Some(previous) = &selected {
            assert_eq!(&origin, previous);
        }
        selected = Some(origin);
    }
}

#[test]
fn legacy_timestamp_evidence_requires_explicit_replay_and_survives_gold_rebuild() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    fixture(&path);
    crate::silver::tests::timestamps::assert_replay(
        Provider::Pi,
        &SourceIdentity::file(&path),
        &options(&path),
    );
}

#[test]
fn timestamps_select_message_maximum_even_before_header_and_fall_back_only_when_empty() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    for records in [
        vec![],
        vec![{
            let mut record = message("before-header", None, "user", "payload");
            record["message"]["timestamp"] = 42.into();
            record
        }],
        vec![{
            let mut record = message("entry-fallback", None, "user", "payload");
            record["message"]
                .as_object_mut()
                .unwrap()
                .remove("timestamp");
            record
        }],
    ] {
        let mut file = File::create(&path).unwrap();
        writeln!(file, "{}", header()).unwrap();
        for record in records {
            writeln!(file, "{record}").unwrap();
        }
        drop(file);
        let parsed = parse(&path).unwrap().conversation.unwrap();
        let messages = parsed.messages.inline();
        let updated =
            messages
                .first()
                .map_or_else(header_origin, |m| TimestampOrigin::MessageBoundary {
                    message_id: m.id.clone(),
                    selection: Boundary::Maximum,
                    rule_version: VERSION,
                });
        assert_eq!(
            parsed.updated_at,
            messages.first().map_or(parsed.created_at, |m| m.created_at)
        );
        assert_eq!(
            Evidence::capture(&parsed).unwrap().timestamps,
            Some(Timestamps {
                created_at: Some(header_origin()),
                updated_at: Some(updated),
            })
        );
        assert!(
            Evidence::capture(&parsed)
                .unwrap()
                .is_current_for(Provider::Pi)
        );
    }
}
