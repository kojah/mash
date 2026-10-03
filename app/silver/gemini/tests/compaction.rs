use super::*;

const CAPTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/docs/fixtures/gemini-compaction-38700b4.jsonl"
));

fn captured() -> Vec<Value> {
    CAPTURE
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn compact(summary: &str, ack: &str) -> Value {
    json!({"$set":{"messages":[
        message(summary,"user",json!("<state_snapshot>syntheticcontextmarker</state_snapshot>")),
        message(ack,"gemini",json!("Got it. Thanks for the additional context!"))
    ]}})
}

fn projections(path: &Path) -> Vec<String> {
    parse(path)
        .unwrap()
        .conversation
        .unwrap()
        .messages
        .inline()
        .to_vec()
        .into_iter()
        .filter_map(|m| m.dialogue_text.filter(|p| !p.is_empty()))
        .collect()
}

#[test]
fn first_party_recording_capture_recovers_dialogue_without_cancelled_or_rewound_turns() {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), CAPTURE).unwrap();
    let result = parse(file.path()).unwrap().conversation.unwrap();
    assert_eq!(result.messages.len(), 8);
    assert_eq!(
        result.messages.inline()[0].content,
        "How should the cBioPortal mutation importer validate metadata?"
    );
    assert_eq!(
        projections(file.path()),
        [
            "How should the cBioPortal mutation importer validate metadata?",
            "Reject incompatible reference genomes before importing mutations.",
            "Also validate structural variants and chromosome aliases.",
            "Normalize chromosome aliases before checking structural variants.",
            "Keep the importer changes limited to validation."
        ]
    );
    assert_eq!(std::fs::read_to_string(file.path()).unwrap(), CAPTURE);
    let original = captured().into_iter().take(10).collect::<Vec<_>>();
    let before = tempfile::NamedTempFile::new().unwrap();
    write(before.path(), &original);
    let earlier = parse(before.path()).unwrap().conversation.unwrap();
    assert_eq!(
        result.messages.inline()[0].id,
        earlier.messages.inline()[0].id
    );
    assert_eq!(
        result.messages.inline()[1].id,
        earlier.messages.inline()[1].id
    );
}

#[test]
fn repeated_compaction_keeps_only_recorded_evidence_and_reset_discards_the_archive() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let mut records = captured();
    records.push(compact("summary-2", "ack-2"));
    records.push(message(
        "new-user",
        "user",
        json!("Second compaction followup"),
    ));
    write(file.path(), &records);
    assert_eq!(projections(file.path()).len(), 6);
    let state = replay(file.path()).unwrap();
    assert_eq!(state.archived.len(), 5);
    assert_eq!(state.archived_ids.len(), 5);
    assert_eq!(
        state.retained_bytes,
        state
            .archived
            .iter()
            .chain(&state.messages)
            .map(retained_bytes)
            .sum::<usize>()
    );
    records.push(json!({"$set":{"messages":[]}}));
    write(file.path(), &records);
    let replay = replay(file.path()).unwrap();
    assert_eq!(replay.retained_bytes, 0);
    assert_eq!(replay.archived.len(), 0);
    assert_eq!(replay.archived_ids.len(), 0);
    assert!(replay.compression_root.is_none());
}

#[test]
fn rewinding_the_compaction_anchor_drops_archived_history_but_archived_ids_are_not_targets() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let mut records = captured();
    records.push(json!({"$rewindTo":"earlier-user"}));
    write(file.path(), &records);
    assert!(parse(file.path()).is_err());
    *records.last_mut().unwrap() = json!({"$rewindTo":"summary-1"});
    write(file.path(), &records);
    let state = replay(file.path()).unwrap();
    assert_eq!(state.retained_bytes, 0);
    assert_eq!(state.archived.len(), 0);
}

#[test]
fn checkpoint_reintroduced_ids_have_one_canonical_record_and_keep_recorded_provenance() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let mut records = captured();
    let mut messages = records[17]["$set"]["messages"].as_array().unwrap().clone();
    messages.push(records[1].clone());
    records.push(json!({"$set":{"messages":messages}}));
    write(file.path(), &records);
    let state = replay(file.path()).unwrap();
    assert_eq!(state.archived.len(), 1);
    assert!(matches!(
        state.positions["earlier-user"].origin,
        Origin::Recorded
    ));
    assert_eq!(
        projections(file.path())
            .iter()
            .filter(|p| p.contains("cBioPortal"))
            .count(),
        1
    );
}

#[test]
fn ambiguous_summary_or_wrong_acknowledgement_never_recovers_removed_history() {
    let file = tempfile::NamedTempFile::new().unwrap();
    for replacement in [
        "Plain summary with no compaction marker",
        "User quoting <state_snapshot> but not a compaction summary",
    ] {
        write(
            file.path(),
            &[
                header(),
                message("old", "user", json!("Old request")),
                json!({"$set":{"messages":[
                    message("summary","user",json!(replacement)),
                    message("ack","gemini",json!("Got it. Thanks for the additional context!"))
                ]}}),
            ],
        );
        assert_eq!(projections(file.path()).len(), 0);
    }
    let mut checkpoint = compact("summary", "ack");
    checkpoint["$set"]["messages"][1]["content"] = json!("Different answer");
    write(
        file.path(),
        &[
            header(),
            message("old", "user", json!("Old request")),
            checkpoint,
        ],
    );
    assert_eq!(projections(file.path()).len(), 0);
}

#[test]
fn recovered_turns_are_searchable_and_explicit_reset_removes_them_from_the_index() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session-compaction.jsonl");
    std::fs::write(&path, CAPTURE).unwrap();
    let database = directory.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&path)).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(
        storage.search("cBioPortal", 10, None, None).unwrap().len(),
        1
    );
    assert_eq!(
        storage
            .search("reference genomes", 10, None, None)
            .unwrap()
            .len(),
        1
    );
    for excluded in [
        "abortedrequestmarker",
        "abortedanswermarker",
        "cancelledaftercompactionmarker",
        "rewoundrequestmarker",
        "rewoundanswermarker",
        "syntheticsummarymarker",
    ] {
        assert_eq!(
            storage.search(excluded, 10, None, None).unwrap().len(),
            0,
            "{excluded}"
        );
    }
    drop(storage);
    let mut records = captured();
    records.push(json!({"$set":{"messages":[]}}));
    write(&path, &records);
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&path)).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(
        storage.search("cBioPortal", 10, None, None).unwrap().len(),
        0
    );
}

#[test]
fn compacted_dialogue_counts_toward_the_same_retained_memory_limit() {
    let mut state = Replay::start(serde_json::from_value(header()).unwrap()).unwrap();
    state
        .record(
            serde_json::from_value(message(
                "large-old",
                "user",
                json!("x".repeat(6 * 1024 * 1024)),
            ))
            .unwrap(),
        )
        .unwrap();
    state
        .record(serde_json::from_value(compact("summary", "ack")).unwrap())
        .unwrap();
    assert_eq!(state.archived.len(), 1);
    state
        .record(
            serde_json::from_value(message(
                "large-later",
                "user",
                json!("y".repeat(6 * 1024 * 1024)),
            ))
            .unwrap(),
        )
        .unwrap();
    assert!(
        state
            .record(
                serde_json::from_value(message(
                    "overflow",
                    "user",
                    json!("z".repeat(5 * 1024 * 1024))
                ))
                .unwrap()
            )
            .is_err()
    );
    state
        .record(Record::Rewind {
            target: "summary".into(),
        })
        .unwrap();
    assert_eq!(state.retained_bytes, 0);
}
