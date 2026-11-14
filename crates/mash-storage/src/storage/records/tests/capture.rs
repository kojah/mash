use super::*;
use crate::records::capture::{Policy, Request};

#[test]
fn native_record_capture_scan_cursor_reaches_work_beyond_completed_pages() {
    let (_directory, mut storage) = database();
    storage
        .connection
        .execute_batch("DELETE FROM messages; DELETE FROM chats;")
        .unwrap();
    let fingerprint = "a".repeat(64);
    let scope = Scope::Repository {
        identity: "repo".into(),
    };
    for index in 0..101 {
        let id = format!("history-{index:03}");
        storage.connection.execute("INSERT INTO chats(id,provider,repository,updated_at,source_fingerprint) VALUES(?1,'codex','repo',1000,?2)", params![id, fingerprint]).unwrap();
        if index < 100 {
            storage
                .connection
                .execute(
                    "INSERT INTO record_checkpoints VALUES(?1,?2,?3)",
                    params![encode(&scope).unwrap(), id, fingerprint],
                )
                .unwrap();
        }
    }
    let policy = Policy {
        enabled: true,
        backfill_since: Some(1000),
    };
    let page = storage
        .record_capture(
            &policy,
            &Request {
                backfill: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(page.backfill.is_none());
    assert_eq!(page.next_offset, Some(100));
    let request = Request {
        backfill: true,
        offset: page.next_offset.unwrap(),
        ..Default::default()
    };
    let last = storage
        .record_capture(&policy, &request)
        .unwrap()
        .backfill
        .unwrap();
    assert_eq!(last.chat_id, "history-100");
    storage
        .extract_records(&records::extraction::Request {
            operation_id: "last-review".into(),
            scope,
            chat_id: last.chat_id,
            source_fingerprint: fingerprint,
            records: vec![],
        })
        .unwrap();
    let done = storage.record_capture(&policy, &request).unwrap();
    assert!(done.backfill.is_none());
    assert!(done.next_offset.is_none());
}

#[test]
fn native_record_capture_discovers_current_and_resumable_backfill_without_writes() {
    let (_directory, mut storage) = database();
    let fingerprint = "a".repeat(64);
    storage
        .connection
        .execute(
            "UPDATE chats SET source_fingerprint=?1,updated_at=1000",
            [&fingerprint],
        )
        .unwrap();
    storage
        .connection
        .execute(
            "UPDATE chats SET source_session='host-session' WHERE id='chat'",
            [],
        )
        .unwrap();
    let request = Request {
        session_id: Some("host-session".into()),
        offset: 0,
        backfill: false,
    };
    let policy = Policy {
        enabled: true,
        backfill_since: Some(1000),
    };
    let request = Request {
        backfill: true,
        ..request
    };
    let offered = storage.record_capture(&policy, &request).unwrap();
    let task = offered.backfill.unwrap();
    assert_eq!(task.chat_id, "other");
    assert_eq!(task.source_fingerprint, fingerprint);
    assert!(matches!(&task.scope, Scope::Repository { identity } if identity == "other-repo"));
    assert_eq!(
        storage
            .connection
            .query_row("SELECT count(*) FROM record_checkpoints", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let extraction = records::extraction::Request {
        operation_id: "empty-capture-review".into(),
        scope: task.scope.clone(),
        chat_id: task.chat_id.clone(),
        source_fingerprint: fingerprint.clone(),
        records: vec![],
    };
    storage.extract_records(&extraction).unwrap();
    assert!(
        storage
            .record_capture(&policy, &request)
            .unwrap()
            .backfill
            .is_none()
    );
    storage
        .connection
        .execute(
            "UPDATE chats SET source_fingerprint=?1 WHERE id='other'",
            ["b".repeat(64)],
        )
        .unwrap();
    let mut stale = extraction.clone();
    stale.operation_id = "stale-new-operation".into();
    assert!(storage.extract_records(&stale).is_err());
    assert!(
        storage
            .record_capture(&policy, &request)
            .unwrap()
            .backfill
            .is_some()
    );
    assert!(
        storage
            .record_capture(
                &Policy {
                    enabled: true,
                    backfill_since: Some(1001)
                },
                &request
            )
            .unwrap()
            .backfill
            .is_none()
    );
    assert!(
        storage
            .record_capture(
                &policy,
                &Request {
                    offset: 100_001,
                    ..Default::default()
                }
            )
            .is_err()
    );
}

#[test]
fn native_record_capture_never_offers_setup_backfill_to_an_ordinary_conversation() {
    let (_directory, storage) = database();
    let policy = Policy {
        enabled: true,
        backfill_since: Some(0),
    };
    let ordinary = storage
        .record_capture(
            &policy,
            &Request {
                session_id: Some("chat".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(ordinary.current.unwrap().chat_id, "chat");
    assert!(ordinary.backfill.is_none());
    assert!(ordinary.next_offset.is_none());
    assert!(!ordinary.scan_limited);
}

#[test]
fn native_record_capture_honors_disabled_policy_and_rejects_ambiguous_sessions() {
    let (_directory, storage) = database();
    let disabled = storage
        .record_capture(
            &Policy {
                enabled: false,
                backfill_since: Some(0),
            },
            &Request::default(),
        )
        .unwrap();
    assert!(!disabled.enabled);
    assert!(disabled.current.is_none() && disabled.backfill.is_none());
    storage
        .connection
        .execute("UPDATE chats SET source_session='ambiguous'", [])
        .unwrap();
    assert!(
        storage
            .record_capture(
                &Policy::default(),
                &Request {
                    session_id: Some("ambiguous".into()),
                    ..Default::default()
                }
            )
            .is_err()
    );
    assert!(
        storage
            .record_capture(
                &Policy::default(),
                &Request {
                    session_id: Some(String::new()),
                    ..Default::default()
                }
            )
            .is_err()
    );
}
