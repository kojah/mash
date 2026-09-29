use super::*;

#[test]
fn semantic_readiness_changes_atomically_with_canonical_messages() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut writer = Storage::open_writer(&path).expect("writer");
    writer
        .replace_conversation(&conversation("original text"))
        .expect("seed conversation");
    writer
        .replace_embeddings(
            "generation",
            &[EmbeddingWrite {
                message_id: "message-1",
                vector: &[127],
                norm: 127.0,
            }],
        )
        .expect("seed embedding");
    writer
        .mark_semantic_index_ready("generation")
        .expect("mark initial index ready");
    writer.commit_writer().expect("commit ready index");

    let storage = Storage::open_existing(&path).expect("open ready database");
    assert!(
        storage
            .semantic_index_is_ready("generation")
            .expect("read initial readiness")
    );
    drop(storage);

    let mut rolled_back = Storage::open_writer(&path).expect("rollback writer");
    rolled_back
        .replace_conversation(&conversation("rolled back change"))
        .expect("change canonical message");
    assert!(
        !rolled_back
            .semantic_index_is_ready("generation")
            .expect("read transactional invalidation")
    );
    drop(rolled_back);

    let storage = Storage::open_existing(&path).expect("reopen after rollback");
    assert!(
        storage
            .semantic_index_is_ready("generation")
            .expect("read readiness after rollback")
    );
    drop(storage);

    let mut committed = Storage::open_writer(&path).expect("commit writer");
    committed
        .replace_conversation(&conversation("committed change"))
        .expect("change canonical message");
    committed.checkpoint_writer().expect("commit invalidation");
    assert!(
        !committed
            .semantic_index_is_ready("generation")
            .expect("read committed invalidation")
    );
    committed.commit_writer().expect("finish writer");

    let storage = Storage::open_existing(&path).expect("reopen incomplete database");
    assert!(
        !storage
            .semantic_index_is_ready("generation")
            .expect("read incomplete readiness")
    );
}

#[test]
fn dirty_search_state_survives_a_batch_and_rebuilds_after_resume() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut writer = Storage::open_writer(&path).expect("writer");
    writer
        .defer_search_updates()
        .expect("defer FTS maintenance");
    writer
        .replace_conversation(&conversation("resumable needle"))
        .expect("message batch");
    writer.checkpoint_writer().expect("durable message batch");
    drop(writer);

    let mut resumed = Storage::open_writer(&path).expect("resumed writer");
    assert!(
        resumed
            .derived_search_is_dirty()
            .expect("dirty search marker")
    );
    assert!(
        resumed
            .search("resumable", 10, None, None)
            .expect("stale FTS is readable")
            .is_empty()
    );
    resumed
        .rebuild_derived_search_state()
        .expect("bulk FTS rebuild");
    assert!(
        !resumed
            .derived_search_is_dirty()
            .expect("clean search marker")
    );
    assert_eq!(
        resumed
            .search("resumable", 10, None, None)
            .expect("rebuilt search")
            .len(),
        1
    );
    resumed.commit_writer().expect("commit rebuilt state");
}

#[test]
fn committed_embedding_checkpoint_resumes_from_only_missing_rows() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut writer = Storage::open_writer(&path).expect("writer");
    writer
        .replace_conversation(&conversation_with_messages(&[
            ("message-1", "first searchable message"),
            ("message-2", "second searchable message"),
        ]))
        .expect("seed canonical messages");
    writer
        .checkpoint_writer()
        .expect("commit canonical and FTS state");
    writer
        .replace_embeddings(
            "generation",
            &[EmbeddingWrite {
                message_id: "message-1",
                vector: &[127, 0],
                norm: 127.0,
            }],
        )
        .expect("first derived batch");
    writer
        .checkpoint_writer()
        .expect("commit first derived checkpoint");
    writer
        .replace_embeddings(
            "generation",
            &[EmbeddingWrite {
                message_id: "message-2",
                vector: &[0, 127],
                norm: 127.0,
            }],
        )
        .expect("uncommitted second derived batch");
    drop(writer);

    let mut resumed = Storage::open_writer(&path).expect("resumed writer");
    assert_eq!(
        resumed
            .search("searchable", 10, None, None)
            .expect("durable FTS rows")
            .len(),
        2
    );
    assert!(
        !resumed
            .semantic_coverage_is_complete("generation")
            .expect("partial coverage")
    );
    let missing = resumed
        .messages_needing_embeddings()
        .expect("missing embeddings");
    assert_eq!(
        missing
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        ["message-2"]
    );
    resumed
        .replace_embeddings(
            "generation",
            &[EmbeddingWrite {
                message_id: "message-2",
                vector: &[0, 127],
                norm: 127.0,
            }],
        )
        .expect("resumed derived batch");
    assert!(
        resumed
            .semantic_coverage_is_complete("generation")
            .expect("complete coverage")
    );
    resumed.commit_writer().expect("commit resumed embedding");
}

#[test]
fn quantized_embedding_blobs_are_validated() {
    let vector = [129_u8, 0, 127];
    assert_eq!(validate_quantized_vector(3, 181.0, &vector), Ok(()));
    assert!(validate_quantized_vector(2, 181.0, &vector).is_err());
    assert!(validate_quantized_vector(3, f32::NAN, &vector).is_err());
}

#[test]
fn malformed_semantic_chunk_metadata_is_rejected() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut writer = Storage::open_writer(&path).expect("writer");
    writer
        .replace_conversation(&conversation("packed vector"))
        .expect("seed message");
    writer
        .replace_embeddings(
            "generation",
            &[EmbeddingWrite {
                message_id: "message-1",
                vector: &[127, 0],
                norm: 127.0,
            }],
        )
        .expect("seed embedding");
    writer
        .mark_semantic_index_ready("generation")
        .expect("publish semantic chunk");
    writer.commit_writer().expect("commit semantic chunk");
    writer
        .connection
        .execute("UPDATE semantic_chunks SET norms = X'00'", [])
        .expect("corrupt norm bytes");

    assert!(writer.semantic_chunks("generation", None, None).is_err());
}

#[test]
fn semantic_chunks_bound_incremental_rewrites_to_affected_rowid_ranges() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut writer = Storage::open_writer(&path).expect("writer");
    writer
        .replace_conversation(&conversation("first chunk"))
        .expect("seed first message");
    writer
        .connection
        .execute(
            "INSERT INTO messages(
                rowid, id, conversation_id, ordinal, role, content,
                search_projection, created_at, fingerprint
             ) VALUES (4097, 'message-4097', 'session-1', 4097, 'assistant',
                       'second chunk', NULL, 123, 'fingerprint')",
            [],
        )
        .expect("seed second chunk message");
    writer
        .replace_embeddings(
            "generation",
            &[
                EmbeddingWrite {
                    message_id: "message-1",
                    vector: &[127, 0],
                    norm: 127.0,
                },
                EmbeddingWrite {
                    message_id: "message-4097",
                    vector: &[0, 127],
                    norm: 127.0,
                },
            ],
        )
        .expect("seed embeddings");
    writer
        .mark_semantic_index_ready("generation")
        .expect("publish semantic chunks");
    writer.commit_writer().expect("commit chunks");

    let chunk_count: i64 = writer
        .connection
        .query_row("SELECT count(*) FROM semantic_chunks", [], |row| row.get(0))
        .expect("count chunks");
    assert_eq!(chunk_count, 2);
    let untouched_before: Vec<u8> = writer
        .connection
        .query_row(
            "SELECT vectors FROM semantic_chunks WHERE chunk_id = 1",
            [],
            |row| row.get(0),
        )
        .expect("second chunk before update");

    let mut writer = Storage::open_writer(&path).expect("update writer");
    writer
        .replace_embeddings(
            "generation",
            &[EmbeddingWrite {
                message_id: "message-1",
                vector: &[0, 126],
                norm: 126.0,
            }],
        )
        .expect("update first chunk embedding");
    writer
        .mark_semantic_index_ready("generation")
        .expect("republish semantic chunks");
    writer
        .commit_writer()
        .expect("commit incremental chunk update");

    let untouched_after: Vec<u8> = writer
        .connection
        .query_row(
            "SELECT vectors FROM semantic_chunks WHERE chunk_id = 1",
            [],
            |row| row.get(0),
        )
        .expect("second chunk after update");
    assert_eq!(untouched_after, untouched_before);
    let updated: Vec<u8> = writer
        .connection
        .query_row(
            "SELECT vectors FROM semantic_chunks WHERE chunk_id = 0",
            [],
            |row| row.get(0),
        )
        .expect("updated first chunk");
    assert_eq!(updated, [0, 126]);
}

#[test]
fn current_embedding_generation_cleanup_preserves_packed_chunks() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut writer = Storage::open_writer(&path).expect("writer");
    writer
        .replace_conversation(&conversation("current generation"))
        .expect("seed message");
    writer
        .replace_embeddings(
            "generation",
            &[EmbeddingWrite {
                message_id: "message-1",
                vector: &[127, 0],
                norm: 127.0,
            }],
        )
        .expect("seed embedding");
    writer.commit_writer().expect("commit semantic chunk");

    let mut writer = Storage::open_writer(&path).expect("no-op generation writer");
    assert_eq!(
        writer
            .invalidate_embedding_generation("generation")
            .expect("retain current generation"),
        0
    );
    writer
        .commit_writer()
        .expect("commit no-op generation check");
    let preserved_chunks: i64 = writer
        .connection
        .query_row("SELECT count(*) FROM semantic_chunks", [], |row| row.get(0))
        .expect("count preserved chunks");
    assert_eq!(preserved_chunks, 1);
}

#[test]
fn explicit_semantic_storage_identifiers_survive_vacuum() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut writer = Storage::open_writer(&path).expect("writer");
    writer
        .replace_conversation(&conversation("stable identifier"))
        .expect("seed message");
    writer
        .replace_embeddings(
            "generation",
            &[EmbeddingWrite {
                message_id: "message-1",
                vector: &[127, 0],
                norm: 127.0,
            }],
        )
        .expect("seed embedding");
    writer
        .mark_semantic_index_ready("generation")
        .expect("publish semantic chunk");
    writer.commit_writer().expect("commit semantic chunk");
    drop(writer);
    let maintenance = Connection::open(&path).expect("maintenance connection");
    maintenance
        .execute_batch("VACUUM")
        .expect("vacuum database");
    drop(maintenance);
    let storage = Storage::open_existing(&path).expect("open vacuumed database");
    let chunks = storage
        .semantic_chunks("generation", None, None)
        .expect("read chunks after vacuum");
    assert_eq!(chunks.chunks[0].message_rowids, [1]);
    assert_eq!(
        storage
            .semantic_chunks("generation", Some("claude-code"), None)
            .expect("apply provider metadata filter")
            .chunks[0]
            .eligible,
        [false]
    );
    assert_eq!(
        storage
            .semantic_chunks("generation", None, Some(90))
            .expect("apply timestamp metadata filter")
            .chunks[0]
            .eligible,
        [true]
    );
    assert_eq!(
        storage
            .search_hits(&[1])
            .expect("hydrate stable storage identifiers")
            .iter()
            .map(|hit| hit.id.as_str())
            .collect::<Vec<_>>(),
        ["message-1"]
    );
}

#[test]
fn stale_embedding_generation_is_excluded_and_replaced() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut writer = Storage::open_writer(&path).expect("writer");
    writer
        .replace_conversation(&conversation("generation proof"))
        .expect("insert conversation");
    writer
        .replace_embeddings(
            "old-generation",
            &[EmbeddingWrite {
                message_id: "message-1",
                vector: &[127, 0],
                norm: 127.0,
            }],
        )
        .expect("old embedding");
    writer.commit_writer().expect("commit old generation");
    drop(writer);

    let storage = Storage::open(&path).expect("reader");
    assert_eq!(storage.embedding_count("new-generation").expect("count"), 0);
    assert_eq!(
        storage
            .semantic_chunks("new-generation", None, None)
            .expect("semantic documents")
            .chunks
            .len(),
        0
    );
    drop(storage);

    let mut writer = Storage::open_writer(&path).expect("replacement writer");
    assert_eq!(
        writer
            .invalidate_embedding_generation("new-generation")
            .expect("invalidate"),
        1
    );
    assert_eq!(
        writer
            .messages_needing_embeddings()
            .expect("re-embedding selection")
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        ["message-1"]
    );
    writer
        .replace_embeddings(
            "new-generation",
            &[EmbeddingWrite {
                message_id: "message-1",
                vector: &[0, 127],
                norm: 127.0,
            }],
        )
        .expect("new embedding");
    writer.commit_writer().expect("commit new generation");
    assert_eq!(writer.embedding_count("new-generation").expect("count"), 1);
    let vectors = writer
        .semantic_chunks("new-generation", None, None)
        .expect("stored quantized vectors");
    assert_eq!(vectors.chunks.len(), 1);
    assert_eq!(vectors.chunks[0].message_rowids, [1]);
    assert_eq!(vectors.chunks[0].values, [0, 127]);
    assert_eq!(vectors.chunks[0].norms, [127.0]);
    assert_eq!(vectors.dimensions, 2);
    assert_eq!(
        writer.search_hits(&[1]).expect("hydrate semantic hit")[0].content,
        "generation proof"
    );
}

#[test]
fn changed_searchable_messages_are_transactionally_queued_for_embedding() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut writer = Storage::open_writer(&path).expect("writer");
    writer
        .replace_conversation(&conversation("searchable message"))
        .expect("insert conversation");

    let pending: i64 = writer
        .connection
        .query_row("SELECT count(*) FROM pending_embeddings", [], |row| {
            row.get(0)
        })
        .expect("pending embedding count");
    assert_eq!(pending, 1);

    writer
        .replace_embeddings(
            "generation",
            &[EmbeddingWrite {
                message_id: "message-1",
                vector: &[127, 0],
                norm: 127.0,
            }],
        )
        .expect("store embedding");
    let pending: i64 = writer
        .connection
        .query_row("SELECT count(*) FROM pending_embeddings", [], |row| {
            row.get(0)
        })
        .expect("drained embedding count");
    assert_eq!(pending, 0);
}
