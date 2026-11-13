use super::*;

#[test]
fn schema_identity_and_duplicate_wire_fields_fail_explicitly() {
    let directory = tempfile::tempdir().unwrap();
    let path = write(directory.path());
    let writer = Connection::open(&path).unwrap();
    let identity = source(&path);
    writer.pragma_update(None, "user_version", 2).unwrap();
    assert_eq!(discover(&path).unwrap()[0].identity, identity);
    let rejected = parse(&identity).unwrap();
    assert!(rejected.chat.is_none());
    assert_eq!(
        rejected.first_record_issue.unwrap().category,
        crate::silver::diagnostics::Category::UnsupportedFormat
    );
    writer.pragma_update(None, "user_version", 0).unwrap();
    let mut changed = identity.clone();
    changed.session = Some("wrong".into());
    assert!(parse(&changed).is_err());
    writer.execute("INSERT INTO trajectory_meta(trajectory_id,cascade_id,trajectory_type,source) VALUES('other','other',4,1)",[]).unwrap();
    assert!(discover(&path).is_err());
    writer
        .execute(
            "DELETE FROM trajectory_meta WHERE trajectory_id='other'",
            [],
        )
        .unwrap();
    let row = &fixture().root_before_mutation.dialogue_steps[0];
    let mut payload = row.step_payload.bytes();
    payload.extend_from_slice(&[8, 14]);
    assert!(
        wire::step(
            &payload,
            row.metadata.as_ref().map(Blob::bytes).as_deref(),
            &mut 65_536
        )
        .is_err()
    );
    drop(writer);
    let wrong = directory.path().join("wrong.db");
    std::fs::rename(&path, &wrong).unwrap();
    assert!(discover(&wrong).is_err());
}

#[test]
fn native_wire_limits_and_sql_blob_guards_reject_amplification() {
    let step = proto::Step {
        kind: 14,
        status: 3,
        user: Some(proto::User {
            items: vec![proto::Item::default(); 65_536],
            ..Default::default()
        }),
        ..Default::default()
    }
    .encode_to_vec();
    assert!(wire::step(&step, None, &mut 65_536).is_err());
    let directory = tempfile::tempdir().unwrap();
    let path = write(directory.path());
    let identity = source(&path);
    Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE steps SET step_payload=zeroblob(8388609) WHERE idx=0",
            [],
        )
        .unwrap();
    assert!(parse(&identity).is_err());
}

#[test]
fn identities_and_retained_replay_are_bounded() {
    let directory = tempfile::tempdir().unwrap();
    let path = write(directory.path());
    let identity = source(&path);
    let writer = Connection::open(&path).unwrap();
    writer
        .execute(
            "UPDATE trajectory_meta SET trajectory_id=?1",
            ["x".repeat(1025)],
        )
        .unwrap();
    assert!(discover(&path).is_err());
    writer
        .execute(
            "UPDATE trajectory_meta SET trajectory_id=?1",
            [fixture().root_before_mutation.trajectory_meta[0]
                .trajectory_id
                .as_str()],
        )
        .unwrap();
    let original = proto::Step::decode(
        fixture().root_before_mutation.dialogue_steps[0]
            .step_payload
            .bytes()
            .as_slice(),
    )
    .unwrap();
    for index in [0, 2, 5] {
        let mut step = original.clone();
        step.kind = proto::Kind::Assistant as i32;
        step.user = None;
        step.metadata.as_mut().unwrap().source = proto::Source::Model as i32;
        step.metadata
            .as_mut()
            .unwrap()
            .origin
            .as_mut()
            .unwrap()
            .step = index;
        step.assistant = Some(proto::Assistant {
            response: "x".repeat(6 * 1024 * 1024),
            ..Default::default()
        });
        replace_step(&path, i64::from(index), &step);
    }
    assert!(parse(&identity).is_err());
}
