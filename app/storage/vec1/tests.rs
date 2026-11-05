use super::*;
use crate::search_filter::{Filter, Scope};
use crate::storage::EmbeddingWrite;

const GENERATION: &str = "vec1-fixture";

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("index.sqlite3");
    let mut writer = Storage::open_writer(&path).unwrap();
    let mut chat = crate::storage::tests::conversation_with_messages(&[
        ("near", "near conversation"),
        ("middle", "middle conversation"),
        ("far", "far conversation"),
        ("near-two", "another near conversation"),
        ("near-three", "third near conversation"),
    ]);
    chat.messages.inline_mut()[2].role = crate::role::Role::Assistant;
    writer.replace_conversation(&chat).unwrap();
    writer.refresh_derived_search_state().unwrap();
    vectors(&mut writer, GENERATION);
    writer.mark_semantic_index_ready(GENERATION).unwrap();
    writer.commit_writer().unwrap();
    (root, path)
}

fn vectors(writer: &mut Storage, generation: &str) {
    writer
        .replace_embeddings(
            generation,
            &[
                EmbeddingWrite {
                    message_id: "near",
                    vector: &[127, 0],
                    norm: 127.0,
                },
                EmbeddingWrite {
                    message_id: "middle",
                    vector: &[0, 127],
                    norm: 127.0,
                },
                EmbeddingWrite {
                    message_id: "far",
                    vector: &[129, 0],
                    norm: 127.0,
                },
                EmbeddingWrite {
                    message_id: "near-two",
                    vector: &[127, 0],
                    norm: 127.0,
                },
                EmbeddingWrite {
                    message_id: "near-three",
                    vector: &[127, 0],
                    norm: 127.0,
                },
            ],
        )
        .unwrap();
}

fn ids(storage: &Storage, scope: Option<&Scope>) -> Vec<i64> {
    storage
        .vec1_candidates(GENERATION, &[127, 0], None, None, scope, 1)
        .unwrap()
        .0
        .into_iter()
        .map(|candidate| candidate.id)
        .collect()
}

#[test]
fn vec1_static_registration_and_signed_vector_conversion() {
    let connection = Connection::open_in_memory().unwrap();
    let info: String = connection
        .query_row("SELECT vec1_info()", [], |row| row.get(0))
        .unwrap();
    assert!(info.contains("0.8"), "{info}");
    assert_eq!(
        float_blob(&[127]),
        [127.0_f32, 0.0]
            .into_iter()
            .flat_map(f32::to_ne_bytes)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        float_blob(&[127, 128, 255, 0]),
        [127.0_f32, -128.0, -1.0, 0.0]
            .into_iter()
            .flat_map(f32::to_ne_bytes)
            .collect::<Vec<_>>()
    );
}

#[test]
fn vec1_filters_stream_before_candidate_limit_and_empty_is_empty() {
    let (_root, path) = fixture();
    let reader = Storage::open_catalog(&path).unwrap();
    let scope = Scope {
        node: "local".into(),
        match_mode: crate::views::matching::Mode::Strict,
        filter: Filter::All(vec![
            Filter::Role(vec![crate::role::Role::Assistant]),
            Filter::Not(Box::new(Filter::Chat(vec!["absent".into()]))),
        ]),
    };
    let expected: i64 = reader
        .connection
        .query_row("SELECT storage_id FROM objects WHERE id='far'", [], |r| {
            r.get(0)
        })
        .unwrap();
    // The sole eligible result is outside the unfiltered four-candidate pool.
    assert!(!ids(&reader, None).contains(&expected));
    assert_eq!(ids(&reader, Some(&scope)), [expected]);
    let empty = Scope {
        filter: Filter::Chat(vec!["absent".into()]),
        ..scope
    };
    let (hits, count) = reader
        .vec1_candidates(GENERATION, &[127, 0], None, None, Some(&empty), 1)
        .unwrap();
    assert!(hits.is_empty());
    assert_eq!(count, 0);
}

#[test]
fn vec1_readers_see_commits_and_rollback_restores_projection() {
    let (_root, path) = fixture();
    let reader = Storage::open_catalog(&path).unwrap();
    let before = ids(&reader, None);
    {
        let mut writer = Storage::open_writer(&path).unwrap();
        writer
            .replace_embeddings(
                GENERATION,
                &[EmbeddingWrite {
                    message_id: "near",
                    vector: &[0, 127],
                    norm: 127.0,
                }],
            )
            .unwrap();
        assert!(!writer.vec1_ready(GENERATION).unwrap());
        assert!(
            writer
                .vec1_candidates(GENERATION, &[127, 0], None, None, None, 1)
                .is_err()
        );
        writer.sync_vec1(GENERATION).unwrap();
        // Drop the uncommitted writer after the entire Vec1 rebuild.
    }
    assert_eq!(ids(&reader, None), before);
    let mut writer = Storage::open_writer(&path).unwrap();
    writer
        .connection
        .execute("DELETE FROM message_embeddings WHERE message_id='near'", [])
        .unwrap();
    writer.sync_vec1(GENERATION).unwrap();
    writer.commit_writer().unwrap();
    assert_ne!(ids(&reader, None), before);
    drop(reader);
    assert!(
        Storage::open_catalog(&path)
            .unwrap()
            .vec1_ready(GENERATION)
            .unwrap()
    );
}

#[test]
fn vec1_failed_build_stays_unready_even_if_caller_commits_then_retries() {
    let (_root, path) = fixture();
    let mut writer = Storage::open_writer(&path).unwrap();
    writer
        .connection
        .execute("DELETE FROM message_embeddings WHERE message_id='near'", [])
        .unwrap();
    writer.connection.execute_batch("CREATE TEMP TRIGGER stop_vec1_cert BEFORE UPDATE ON views.vec1_state WHEN NEW.dirty=0 BEGIN SELECT RAISE(ABORT,'injected before Vec1 certificate'); END;").unwrap();
    assert!(writer.sync_vec1(GENERATION).is_err());
    writer.commit_writer().unwrap();
    drop(writer);
    let reader = Storage::open_catalog(&path).unwrap();
    assert!(!reader.vec1_ready(GENERATION).unwrap());
    assert!(
        reader
            .vec1_candidates(GENERATION, &[127, 0], None, None, None, 1)
            .is_err()
    );
    let writer = Storage::open_writer(&path).unwrap();
    writer.sync_vec1(GENERATION).unwrap();
    assert!(writer.vec1_ready(GENERATION).unwrap());
}

#[test]
fn vec1_rebuild_accepts_new_generation_dimensions_and_empty_views() {
    let (_root, path) = fixture();
    let mut writer = Storage::open_writer(&path).unwrap();
    writer
        .connection
        .execute("DELETE FROM message_embeddings", [])
        .unwrap();
    writer.sync_vec1("empty").unwrap();
    assert_eq!(
        writer
            .connection
            .query_row("SELECT count(*) FROM vec1_index", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0,
    );
    assert!(
        writer
            .vec1_candidates("empty", &[127, 0, 0], None, None, None, 1)
            .unwrap()
            .0
            .is_empty()
    );
    for message_id in ["near", "middle", "far"] {
        writer
            .replace_embeddings(
                "new",
                &[EmbeddingWrite {
                    message_id,
                    vector: &[127, 0, 0],
                    norm: 127.0,
                }],
            )
            .unwrap();
    }
    writer.sync_vec1("new").unwrap();
    assert_eq!(
        writer
            .vec1_candidates("new", &[127, 0, 0], None, None, None, 1)
            .unwrap()
            .1,
        3
    );
    assert!(!writer.vec1_ready(GENERATION).unwrap());
}

#[test]
#[ignore = "requires explicit frozen source and NEW private MASH_VEC1_TRIAL_DB"]
fn vec1_prepare_corpus() {
    let source =
        std::path::PathBuf::from(std::env::var_os("MASH_TEST_SEARCH_DB").expect("frozen source"));
    let destination = std::path::PathBuf::from(
        std::env::var_os("MASH_VEC1_TRIAL_DB").expect("new private database"),
    );
    assert!(!destination.exists(), "destination must be new");
    let source_connection =
        Connection::open_with_flags(&source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    crate::storage::packed::register(&source_connection).unwrap();
    source_connection
        .execute("VACUUM INTO ?1", [destination.to_str().unwrap()])
        .unwrap();
    drop(source_connection);
    let mut writer = Storage::open_writer(&destination).unwrap();
    let source_views = crate::storage::schema::views_path(&source);
    writer
        .connection
        .execute(
            "ATTACH DATABASE ?1 AS baseline_views",
            [source_views.to_str().unwrap()],
        )
        .unwrap();
    for table in [
        "message_embeddings",
        "pending_embeddings",
        "semantic_chunks",
        "connector_units",
        "connector_embeddings",
        "derived_state",
        "views_manifest",
        "lexical_manifest",
    ] {
        writer.connection.execute_batch(&format!("DELETE FROM views.{table}; INSERT INTO views.{table} SELECT * FROM baseline_views.{table};")).unwrap();
    }
    writer
        .connection
        .execute_batch(
            "INSERT INTO views.message_fts(rowid,content,message_id,conversation_id)
           SELECT rowid,content,message_id,conversation_id FROM baseline_views.message_fts;
         INSERT INTO views.connector_fts(rowid,content)
           SELECT rowid,content FROM baseline_views.connector_fts;",
        )
        .unwrap();
    let generation: String = writer
        .connection
        .query_row(
            "SELECT semantic_ready_generation FROM derived_state WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let started = std::time::Instant::now();
    writer.sync_vec1(&generation).unwrap();
    let build_ms = started.elapsed().as_secs_f64() * 1000.0;
    writer.commit_writer().unwrap();
    let points: i64 = writer
        .connection
        .query_row("SELECT count(*) FROM vec1_points", [], |r| r.get(0))
        .unwrap();
    drop(writer);
    assert!(Storage::open_search(&destination, &generation).is_ok());
    println!(
        "{}",
        serde_json::json!({"build_ms":build_ms,"points":points,"generation":generation})
    );
}
