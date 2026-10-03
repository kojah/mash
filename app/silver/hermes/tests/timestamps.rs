use super::*;
use crate::silver::enrichment::{Boundary, Evidence};

fn native(field: TimestampField) -> TimestampOrigin {
    TimestampOrigin::SourceMetadata {
        field,
        rule_version: VERSION,
    }
}

#[test]
fn legacy_metadata_evidence_requires_explicit_replay_and_survives_gold_rebuild() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.db");
    drop(fixture(&path));
    crate::silver::tests::timestamps::assert_replay(
        Provider::Hermes,
        &SourceIdentity {
            path: path.clone(),
            session: Some("alpha".into()),
        },
        &options(&path),
    );
}

#[test]
fn timestamp_selection_preserves_native_priority_and_normalized_ties() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.db");
    let upstream = fixture(&path);
    let source = SourceIdentity {
        path,
        session: Some("alpha".into()),
    };
    for (ended, activity, expected, field) in [
        (
            Some(1_790_726_402.0),
            Some(1_790_726_403.0),
            1_790_726_402_000,
            TimestampField::SessionsEndedAt,
        ),
        (
            Some(1_790_726_401.000_9),
            Some(1_790_726_403.0),
            1_790_726_401_000,
            TimestampField::SessionsEndedAt,
        ),
        (
            None,
            Some(1_790_726_401.0),
            1_790_726_401_000,
            TimestampField::SessionsLastActivityAt,
        ),
    ] {
        upstream
            .execute(
                "UPDATE sessions SET ended_at=?1,last_activity_at=?2 WHERE id='alpha'",
                rusqlite::params![ended, activity],
            )
            .unwrap();
        let parsed = parse(&source).unwrap().conversation.unwrap();
        let evidence = Evidence::capture(&parsed).unwrap();
        assert_eq!(parsed.created_at, Some(1_790_726_400_000));
        assert_eq!(parsed.updated_at, Some(expected));
        assert_eq!(
            evidence.timestamps,
            Some(Timestamps {
                created_at: Some(native(TimestampField::SessionsStartedAt)),
                updated_at: Some(native(field)),
            })
        );
        assert!(evidence.is_current_for(Provider::Hermes));
    }
    // The original coalesce must not decode an unused native value.
    upstream.execute_batch("UPDATE sessions SET ended_at=1790726402,last_activity_at='unused malformed value' WHERE id='alpha'").unwrap();
    assert_eq!(
        parse(&source).unwrap().conversation.unwrap().updated_at,
        Some(1_790_726_402_000)
    );
}

#[test]
fn timestamp_message_maximum_missing_times_and_started_fallback_are_traceable() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.db");
    let upstream = fixture(&path);
    let source = SourceIdentity {
        path,
        session: Some("alpha".into()),
    };
    upstream.execute_batch("UPDATE sessions SET ended_at=NULL,last_activity_at=NULL WHERE id='alpha'; UPDATE messages SET timestamp=1790726401 WHERE session_id='alpha'").unwrap();
    let parsed = parse(&source).unwrap().conversation.unwrap();
    let selected = parsed
        .messages
        .inline()
        .iter()
        .map(|m| &m.id)
        .min()
        .unwrap();
    assert_eq!(
        Evidence::capture(&parsed)
            .unwrap()
            .timestamps
            .unwrap()
            .updated_at,
        Some(TimestampOrigin::MessageBoundary {
            message_id: selected.clone(),
            selection: Boundary::Maximum,
            rule_version: VERSION,
        })
    );
    upstream
        .execute_batch("UPDATE messages SET timestamp=NULL WHERE session_id='alpha'")
        .unwrap();
    let parsed = parse(&source).unwrap().conversation.unwrap();
    assert_eq!(parsed.updated_at, Some(1_790_726_400_000));
    assert_eq!(
        Evidence::capture(&parsed)
            .unwrap()
            .timestamps
            .unwrap()
            .updated_at,
        Some(native(TimestampField::SessionsStartedAt))
    );
    upstream
        .execute_batch("UPDATE sessions SET started_at=NULL WHERE id='alpha'")
        .unwrap();
    let parsed = parse(&source).unwrap().conversation.unwrap();
    assert_eq!((parsed.created_at, parsed.updated_at), (None, None));
    assert_eq!(
        Evidence::capture(&parsed).unwrap().timestamps,
        Some(Timestamps {
            created_at: None,
            updated_at: None
        })
    );
}
