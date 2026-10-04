use super::*;

#[derive(serde::Deserialize)]
struct Fixture {
    name: String,
    text: String,
    secrets: Vec<String>,
}

#[test]
fn synthetic_credentials_and_benign_chat_contract() {
    let mut fixtures: Vec<Fixture> =
        serde_json::from_str(include_str!("../../research/redaction/fixtures.json")).unwrap();
    fixtures.extend(
        serde_json::from_str::<Vec<Fixture>>(include_str!(
            "../../research/redaction/additional.json"
        ))
        .unwrap(),
    );
    fixtures.extend(
        serde_json::from_str::<Vec<Fixture>>(include_str!(
            "../../research/redaction/audit-regressions.json"
        ))
        .unwrap(),
    );
    for fixture in fixtures {
        let safe = text(&fixture.text);
        assert_eq!(text(&safe), safe, "idempotence: {}", fixture.name);
        for secret in &fixture.secrets {
            assert!(
                !safe.contains(secret),
                "missed credential: {}",
                fixture.name
            );
            let masked = projection(&fixture.text);
            for (start, value) in fixture.text.match_indices(secret.as_str()) {
                assert!(
                    masked[start..start + value.len()]
                        .chars()
                        .all(|c| matches!(c, '*' | '×' | '█' | '🟥')),
                    "partially masked credential: {}",
                    fixture.name
                );
            }
        }
        if fixture.secrets.is_empty() {
            assert_eq!(
                safe, fixture.text,
                "changed benign content: {}",
                fixture.name
            );
        }
    }
}

#[test]
fn nested_serialization_masks_values_without_eating_following_content() {
    for value in [
        r#"password="pa\"ssword" tail-kept"#,
        r#"password="pa\\" tail-kept"#,
    ] {
        let mut source = value.to_owned();
        let mut expected = "password=\"[REDACTED_SECRET]\" tail-kept".to_owned();
        for _ in 0..4 {
            assert_eq!(text(&source), expected);
            let masked = projection(&source);
            assert_eq!(masked.len(), source.len());
            assert_eq!(masked.chars().count(), source.chars().count());
            assert!(masked.contains("tail-kept"));
            source = serde_json::to_string(&source).unwrap();
            expected = serde_json::to_string(&expected).unwrap();
        }
    }
    assert_eq!(
        text(r#"password=\"unfinished"#),
        r#"password=\"[REDACTED_SECRET]"#
    );
}

#[test]
fn offset_masking_and_fail_closed_limits() {
    let source = "🔑 password=\"café-東京-🗝\" next";
    let safe = projection(source);
    assert_eq!(source.len(), safe.len());
    assert_eq!(source.chars().count(), safe.chars().count());
    assert!(!safe.contains("café"));
    let large = "x".repeat(2 * 1024 * 1024 + 1);
    assert_eq!(text(&large), "[REDACTED_SECRET]");
    let spam = "password=\"hunter2\" ".repeat(4100);
    assert_eq!(text(&spam), "[REDACTED_SECRET]");
    assert_eq!(
        text("-----BEGIN PRIVATE KEY-----\nunfinished body"),
        "[REDACTED_SECRET]"
    );
}

#[test]
fn clipping_never_exposes_a_partial_token() {
    let secret = "ghp_K9nR2xW7bT4qM8vC6zA3sF5uH1jL0dE9pQ2r";
    let source = format!("{} token: {secret} trailing", "padding ".repeat(30));
    let excerpt = crate::search_results::evidence_window(&source, "token", 180);
    assert!(!excerpt.contains("ghp_") && !excerpt.contains("K9nR2"));
    assert!(excerpt.contains("*****"));
}

#[test]
fn upload_serialization_masks_canonical_projection_and_metadata() {
    use crate::shared::protocol::{Batch, Event, MessageRecord};
    let event = Event::Batch(Batch::Messages {
        records: vec![MessageRecord {
            attachments: Vec::new(),
            id: "m".into(),
            conversation_id: "c".into(),
            ordinal: 0,
            context: crate::message_context::MessageContext::default(),
            role: crate::role::Role::User,
            content: "password=\"hunter2\"".into(),
            projection: "Authorization: Bearer opaque-value".into(),
            created_at: None,
            generation: None,
            vector: None,
        }],
    });
    let mut buffer = Vec::new();
    crate::shared::write_event(&mut buffer, &event).unwrap();
    let safe = String::from_utf8(buffer).unwrap();
    assert!(!safe.contains("hunter2") && !safe.contains("opaque-value"));
    assert!(safe.contains("[REDACTED_SECRET]"));
    let mut metadata = crate::search_results::ThreadMetadata {
        title: Some("password=\"hunter2\"".into()),
        project: Some("postgres://u:secret@host/db".into()),
        ..Default::default()
    };
    super::metadata(&mut metadata);
    assert!(!metadata.title.unwrap().contains("hunter2"));
    assert!(!metadata.project.unwrap().contains("secret@"));
    // Local canonical rows are intentionally retained.
    let Event::Batch(Batch::Messages { records }) = event else {
        panic!()
    };
    assert!(records[0].content.contains("hunter2"));
}

#[test]
fn policy_upgrade_is_pending_and_retries_keep_the_same_revision() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("index.sqlite3");
    let mut storage = crate::storage::Storage::open_writer(&path).unwrap();
    storage.commit_writer().unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE shared_publication SET revision=1 WHERE singleton=1",
            [],
        )
        .unwrap();
    storage.prepare_publication_target("first").unwrap();
    let revision = storage.with_publication_snapshot(|_| Ok(())).unwrap();
    storage.acknowledge_publication("first", revision).unwrap();
    assert!(storage.publication_target_matches("first").unwrap());
    assert!(!storage.publication_target_matches("redaction-v1").unwrap());
    assert!(!crate::storage::Storage::publication_is_pending(&path, "first").unwrap());
    storage.prepare_publication_target("redaction-v1").unwrap();
    let next = storage.with_publication_snapshot(|_| Ok(())).unwrap();
    assert_eq!(next, revision + 1);
    assert!(crate::storage::Storage::publication_is_pending(&path, "redaction-v1").unwrap());
    storage.prepare_publication_target("redaction-v1").unwrap();
    assert_eq!(storage.with_publication_snapshot(|_| Ok(())).unwrap(), next);
}
