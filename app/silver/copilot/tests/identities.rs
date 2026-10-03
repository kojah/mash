use super::*;

fn write_many(path: &Path, duplicate: bool) {
    let mut output = File::create(path).unwrap();
    for event in &fixture()[..4] {
        writeln!(output, "{event}").unwrap();
    }
    for index in 0..25_000 {
        let mut event = record(
            &format!("operational-{index:05}"),
            "assistant.message_delta",
            json!({"messageId":"answer","deltaContent":"ignored delta"}),
        );
        event["ephemeral"] = json!(true);
        writeln!(output, "{event}").unwrap();
    }
    if duplicate {
        // Header identities must still be tracked after migration to disk,
        // and ephemeral events must not bypass exact duplicate validation.
        let mut event = record(
            "start",
            "assistant.message_delta",
            json!({"deltaContent":"ignored"}),
        );
        event["ephemeral"] = json!(true);
        writeln!(output, "{event}").unwrap();
    }
}

#[test]
fn late_operational_duplicate_preserves_snapshot_and_checkpoint_until_repair() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("events.jsonl");
    let database = directory.path().join("index.sqlite3");
    write(&source, &fixture()[..4]);
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&source)).unwrap();
    storage.commit_writer().unwrap();
    let identity = SourceIdentity::file(&source);
    let stamp = source_stamp_for(&identity).unwrap();
    drop(storage);

    write_many(&source, true);
    let mut storage = Storage::open_writer(&database).unwrap();
    let error = index(&mut storage, &options(&source))
        .err()
        .expect("reject duplicate after spill");
    assert!(error.error.message.contains("duplicate"));
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 2);
    assert!(
        storage
            .source_checkpoint_matches(Provider::Copilot, &identity, &stamp)
            .unwrap()
    );
    assert!(
        !storage
            .search("importer", 10, None, None)
            .unwrap()
            .is_empty()
    );
    drop(storage);

    write_many(&source, false);
    let mut storage = Storage::open_writer(&database).unwrap();
    let summary = index(&mut storage, &options(&source)).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(summary.changed_messages, 0);
    assert_eq!(storage.counts().unwrap().messages, 2);
    assert!(
        storage
            .source_checkpoint_matches(
                Provider::Copilot,
                &identity,
                &source_stamp_for(&identity).unwrap()
            )
            .unwrap()
    );
}
