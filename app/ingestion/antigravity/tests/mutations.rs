use super::*;

#[test]
fn corrupt_native_updates_preserve_prior_checkpoint_and_search() {
    let directory = tempfile::tempdir().unwrap();
    let path = write(directory.path());
    let opts = options(&path);
    let database = directory.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &opts).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(
        storage
            .search("cbioportal", 20, Some(Provider::Antigravity), None)
            .unwrap()
            .len(),
        9
    );
    let checkpoint = direct_scans(&opts).unwrap().remove(0).sources.remove(0);
    let stamp = checkpoint.stamp().unwrap();
    drop(storage);
    let writer = Connection::open(&path).unwrap();
    for sql in [
        "UPDATE steps SET step_type=15 WHERE idx=0",
        "UPDATE steps SET step_format=99 WHERE idx=0",
        "UPDATE steps SET step_payload=x'80' WHERE idx=0",
        "UPDATE steps SET metadata=x'00' WHERE idx=0",
    ] {
        let row = &fixture().root_before_mutation.dialogue_steps.remove(0);
        writer.execute(sql, []).unwrap();
        let mut storage = Storage::open_writer(&database).unwrap();
        assert!(index(&mut storage, &opts).is_err());
        storage.commit_writer().unwrap();
        assert_eq!(
            storage
                .search("cbioportal", 20, Some(Provider::Antigravity), None)
                .unwrap()
                .len(),
            9
        );
        assert!(
            storage
                .source_checkpoint_matches(Provider::Antigravity, &checkpoint.identity, &stamp)
                .unwrap()
        );
        writer.execute("UPDATE steps SET step_type=?1,step_format=?2,step_payload=?3,metadata=?4 WHERE idx=0",params![row.step_type,row.step_format,row.step_payload.bytes(),row.metadata.as_ref().map(Blob::bytes)]).unwrap();
    }
}
#[test]
fn wal_writes_change_stamps_and_rewind_keeps_surviving_message_ids() {
    let directory = tempfile::tempdir().unwrap();
    let path = write(directory.path());
    let identity = source(&path);
    let before = conversation(&path);
    let old = crate::ingestion::source_stamp_for(&identity).unwrap();
    let writer = Connection::open(&path).unwrap();
    writer
        .execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; DELETE FROM steps WHERE idx>2;",
        )
        .unwrap();
    assert_ne!(old, crate::ingestion::source_stamp_for(&identity).unwrap());
    let after = conversation(&path);
    assert_eq!(after.messages.len(), 2);
    assert_eq!(before.messages[0].id, after.messages[0].id);
    assert_eq!(before.messages[1].id, after.messages[1].id);
}
