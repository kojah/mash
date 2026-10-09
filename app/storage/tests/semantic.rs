use super::*;

#[test]
fn coverage_requires_membership_as_well_as_cardinality() {
    let directory = tempfile::tempdir().unwrap();
    let mut writer = Storage::open_writer(&directory.path().join("coverage.sqlite3")).unwrap();
    let mut source = conversation_with_messages(&[
        ("first", "first visible message"),
        ("second", "second visible message"),
        ("hidden", "context-only content"),
    ]);
    source.messages.inline_mut()[2].dialogue_text = Some(String::new());
    writer.replace_conversation(&source).unwrap();
    let embedding = |message_id| EmbeddingWrite {
        message_id,
        vector: &[127],
        norm: 127.0,
    };
    writer
        .replace_embeddings("generation", &[embedding("first"), embedding("hidden")])
        .unwrap();
    // Two vectors for two eligible messages still fails: one is the wrong member.
    assert!(!writer.semantic_coverage_is_complete("generation").unwrap());
    writer
        .connection
        .execute(
            "DELETE FROM message_embeddings WHERE message_id='hidden'",
            [],
        )
        .unwrap();
    writer
        .replace_embeddings("other-generation", &[embedding("second")])
        .unwrap();
    assert!(!writer.semantic_coverage_is_complete("generation").unwrap());
    writer
        .replace_embeddings("generation", &[embedding("second")])
        .unwrap();
    assert!(writer.semantic_coverage_is_complete("generation").unwrap());
    writer
        .connection
        .execute(
            "UPDATE objects SET search_projection='' WHERE origin='provider' AND id='second'",
            [],
        )
        .unwrap();
    // Surplus vectors must fail even when every remaining eligible row is covered.
    assert!(!writer.semantic_coverage_is_complete("generation").unwrap());
    writer
        .connection
        .execute(
            "DELETE FROM message_embeddings WHERE message_id='second'",
            [],
        )
        .unwrap();
    assert!(writer.semantic_coverage_is_complete("generation").unwrap());
}

#[test]
fn filtered_dense_rows_use_searchable_index_and_preserve_fallback_and_boolean_scope() {
    use crate::search_filter::{Filter, Scope};
    let directory = tempfile::tempdir().unwrap();
    let mut writer = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    let mut source = conversation_with_messages(&[
        ("recent", "recent visible message"),
        ("fallback", "legacy visible message"),
        ("older", "older visible message"),
        ("hidden", "private tool payload"),
    ]);
    for message in source.messages.inline_mut() {
        message.created_at = Some(if message.id == "older" { 1000 } else { 3000 });
        if message.id == "hidden" {
            message.role = crate::role::Role::Tool;
            message.dialogue_text = Some(String::new());
        }
    }
    writer.replace_conversation(&source).unwrap();
    writer
        .connection
        .execute(
            "UPDATE objects SET search_projection=NULL WHERE origin='provider' AND id='fallback'",
            [],
        )
        .unwrap();
    let embeddings = ["recent", "fallback", "older"].map(|message_id| EmbeddingWrite {
        message_id,
        vector: &[127],
        norm: 127.0,
    });
    writer
        .replace_embeddings("generation", &embeddings)
        .unwrap();
    writer.mark_semantic_index_ready("generation").unwrap();
    let scope = Scope {
        match_mode: crate::gold::matching::Mode::Strict,
        node: "local".into(),
        filter: Filter::Any(vec![Filter::After(2), Filter::Chat(vec!["absent".into()])]),
    };
    let query = super::super::semantic::semantic_filter_query(&scope);
    let plans = writer
        .connection
        .prepare(&format!("EXPLAIN QUERY PLAN {query}"))
        .unwrap()
        .query_map([], |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        plans
            .iter()
            .any(|plan| plan.contains("objects_searchable_metadata")),
        "{plans:?}"
    );
    let chunks = writer
        .semantic_chunks_filtered("generation", None, None, Some(&scope))
        .unwrap();
    let eligible = chunks
        .chunks
        .iter()
        .flat_map(|chunk| {
            chunk
                .message_rowids
                .iter()
                .zip(&chunk.eligible)
                .filter_map(|(id, allowed)| allowed.then_some(*id))
        })
        .collect::<Vec<_>>();
    let mut ids = writer
        .search_hits(&eligible)
        .unwrap()
        .into_iter()
        .map(|hit| hit.id)
        .collect::<Vec<_>>();
    ids.sort();
    assert_eq!(ids, ["fallback", "recent"]);
    let excluded = Scope {
        match_mode: crate::gold::matching::Mode::Strict,
        node: "local".into(),
        filter: Filter::Node(vec!["another-node".into()]),
    };
    let chunks = writer
        .semantic_chunks_filtered("generation", None, None, Some(&excluded))
        .unwrap();
    assert!(
        chunks
            .chunks
            .iter()
            .all(|chunk| chunk.eligible.iter().all(|allowed| !allowed))
    );
}

#[test]
fn dense_queue_selection_preserves_users_and_all_role_recipes() {
    use crate::gold::config::{Build, DenseContent};
    use crate::role::Role;

    let directory = tempfile::tempdir().unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    let mut source = conversation_with_messages(&[
        ("user", "question"),
        ("assistant", "answer"),
        ("tool", "tool result"),
        ("unknown", "provider-defined role"),
    ]);
    for (message, role) in source.messages.inline_mut().iter_mut().zip([
        Role::User,
        Role::Assistant,
        Role::Tool,
        Role::Other("custom".to_owned()),
    ]) {
        message.role = role;
    }
    storage.replace_conversation(&source).unwrap();
    let queued = |storage: &Storage| {
        storage
            .connection
            .prepare("SELECT message_id FROM pending_embeddings ORDER BY message_id")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    let initial = queued(&storage);
    assert_eq!(initial.len(), 4);
    storage
        .restrict_dense_queue(&Build {
            dense_content: DenseContent::AllVisible,
            ..Build::default()
        })
        .unwrap();
    assert_eq!(queued(&storage), initial);
    storage
        .restrict_dense_queue(&Build {
            dense_content: DenseContent::UserMessages,
            ..Build::default()
        })
        .unwrap();
    assert_eq!(queued(&storage), ["user"]);
    assert_eq!(storage.counts().unwrap().messages, 4);
    storage
        .connection
        .execute("DELETE FROM pending_embeddings", [])
        .unwrap();
    storage.restrict_dense_queue(&Build::default()).unwrap();
    assert_eq!(queued(&storage), Vec::<String>::new());
}

#[test]
fn empty_index_requires_an_explicit_completed_generation_and_search_is_read_only() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("empty.sqlite3");
    let mut writer = Storage::open_writer(&path).unwrap();
    writer.commit_writer().unwrap();
    drop(writer);
    assert_eq!(
        Storage::open_search(&path, "generation")
            .err()
            .unwrap()
            .code,
        8
    );
    let mut writer = Storage::open_writer(&path).unwrap();
    writer.rebuild_derived_search_state().unwrap();
    writer.mark_semantic_index_ready("generation").unwrap();
    writer.commit_writer().unwrap();
    drop(writer);
    let reader = Storage::open_search(&path, "generation").unwrap();
    assert!(
        reader
            .connection
            .execute("DELETE FROM derived_state", [])
            .is_err()
    );
    assert!(Storage::open_search(&path, "another-generation").is_err());
}

#[test]
fn search_does_not_create_or_migrate_an_index() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing.sqlite3");
    assert_eq!(
        Storage::open_search(&missing, "generation")
            .err()
            .unwrap()
            .code,
        3
    );
    assert!(!missing.exists());
    let old = directory.path().join("old.sqlite3");
    Connection::open(&old)
        .unwrap()
        .execute_batch("PRAGMA user_version = 1")
        .unwrap();
    let before = std::fs::read(&old).unwrap();
    assert_eq!(
        Storage::open_search(&old, "generation").err().unwrap().code,
        7
    );
    assert_eq!(before, std::fs::read(old).unwrap());
}

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
            "INSERT INTO objects(
                storage_id, origin, source, object_role, kind, parent_id, parent_storage_id,
                id, conversation_id, ordinal, role, content, search_projection, created_at
             ) SELECT 4097, 'provider', source, 'entry', source || ':message', id, storage_id,
                       'message-4097', 'session-1', 4097, 'assistant',
                       'second chunk', NULL, 123
                 FROM objects WHERE origin = 'provider' AND object_role = 'container' AND id = 'session-1'",
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
    assert_eq!(chunks.chunks[0].message_rowids, [2]);
    assert_eq!(
        storage
            .semantic_chunks(
                "generation",
                Some(crate::provider::Provider::ClaudeCode),
                None
            )
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
            .search_hits(&[2])
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
    assert_eq!(vectors.chunks[0].message_rowids, [2]);
    assert_eq!(vectors.chunks[0].values, [0, 127]);
    assert_eq!(vectors.chunks[0].norms, [127.0]);
    assert_eq!(vectors.dimensions, 2);
    assert_eq!(
        writer.search_hits(&[2]).expect("hydrate semantic hit")[0].content,
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
