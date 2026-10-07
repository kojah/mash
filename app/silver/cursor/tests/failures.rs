use super::*;

#[test]
fn missing_corrupt_oversized_and_malformed_blobs_preserve_previous_index() {
    let dir = tempfile::tempdir().unwrap();
    let path = path(dir.path());
    let connection = fixture(&path);
    let database = dir.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&path)).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let sources = crate::silver::discovered_sources(Provider::Cursor, std::slice::from_ref(&path));
    let source = &sources[0];
    let stamp = source.stamp().unwrap();
    let root = root(&connection);
    let target = hex::encode(&root.messages[2]);
    let original: Vec<u8> = connection
        .query_row("SELECT data FROM blobs WHERE id=?1", [&target], |r| {
            r.get(0)
        })
        .unwrap();
    for data in [
        None,
        Some(b"corrupt".to_vec()),
        Some(vec![0; 8 * 1024 * 1024 + 1]),
    ] {
        connection
            .execute("DELETE FROM blobs WHERE id=?1", [&target])
            .unwrap();
        if let Some(data) = data {
            connection
                .execute("INSERT INTO blobs VALUES(?1,?2)", params![&target, data])
                .unwrap();
        }
        assert!(parse(&source.identity).is_err());
        let mut storage = Storage::open_writer(&database).unwrap();
        assert!(index(&mut storage, &options(&path)).is_err());
        storage.commit_writer().unwrap();
        assert_eq!(storage.counts().unwrap().messages, 6);
        assert!(
            storage
                .source_checkpoint_matches(Provider::Cursor, &source.identity, &stamp)
                .unwrap()
        );
        assert_eq!(
            storage
                .search("cbioportal", 10, Some(Provider::Cursor), None)
                .unwrap()
                .len(),
            2
        );
    }
    connection
        .execute(
            "INSERT OR REPLACE INTO blobs VALUES(?1,?2)",
            params![target, original],
        )
        .unwrap();
    let mut malformed = root.clone();
    malformed
        .messages
        .push(put(&connection, b"{\"role\":\"user\","));
    checkpoint(&connection, &malformed);
    assert!(parse(&source.identity).is_err());
    checkpoint(&connection, &root);
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&path)).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 6);
}

#[test]
fn native_protobuf_structural_amplification_is_rejected_before_decode() {
    let dir = tempfile::tempdir().unwrap();
    let path = path(dir.path());
    let connection = fixture(&path);
    let original = root(&connection);
    let oversized = [0x0a, 0].repeat(65_537);
    let id = put(&connection, &oversized);
    let mut meta = metadata(&connection);
    meta.latest_root_blob_id = hex::encode(id);
    set_metadata(&connection, &meta);
    assert!(
        parse(&discover(&path).unwrap()[0].identity)
            .err()
            .unwrap()
            .error
            .message
            .contains("field limit")
    );
    let nested = [0x12, 0].repeat(65_537);
    let mut envelope = vec![0x0a];
    prost::encoding::encode_varint(nested.len() as u64, &mut envelope);
    envelope.extend(nested);
    let id = put(&connection, &envelope);
    let mut root = original;
    root.turns.push(id);
    checkpoint(&connection, &root);
    assert!(
        parse(&discover(&path).unwrap()[0].identity)
            .err()
            .unwrap()
            .error
            .message
            .contains("field limit")
    );
}

#[test]
fn empty_initial_store_is_valid_but_missing_transcript_history_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let path = path(dir.path());
    let connection = fixture(&path);
    let mut populated = root(&connection);
    populated.messages.clear();
    populated.archives.clear();
    checkpoint(&connection, &populated);
    let error = parse(&discover(&path).unwrap()[0].identity).err().unwrap();
    assert!(
        error
            .error
            .message
            .contains("no reachable transcript history")
    );

    let mut meta = metadata(&connection);
    meta.latest_root_blob_id.clear();
    set_metadata(&connection, &meta);
    assert!(parsed(&path).messages.is_empty());
}

#[test]
fn wal_only_changes_are_checkpointed_and_unreachable_old_blobs_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let path = path(dir.path());
    let connection = fixture(&path);
    connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA wal_autocheckpoint=0;")
        .unwrap();
    let source = discover(&path).unwrap().remove(0);
    let before = source.stamp().unwrap();
    put(&connection, b"old-unreachable-invalid-json");
    assert_eq!(parsed(&path).messages.len(), 6);
    assert_ne!(before.fingerprint, source.stamp().unwrap().fingerprint);
    let mut metadata = metadata(&connection);
    metadata.name = "New native title".into();
    set_metadata(&connection, &metadata);
    assert_eq!(parsed(&path).title.as_deref(), Some("New native title"));
    assert!(path.with_file_name("store.db-wal").exists());
}
