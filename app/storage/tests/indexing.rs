use super::*;

use crate::ingestion::SourceStamp;
use std::collections::BTreeSet;

#[test]
fn prior_text_fingerprints_preserve_vectors_for_context_changes_and_invalidate_projection_edits() {
    let directory = tempfile::tempdir().unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    let mut value = conversation("canonical context and visible sentinel");
    value.messages[0].search_projection = Some("visible sentinel".into());
    storage.replace_conversation(&value).unwrap();
    storage
        .replace_embeddings(
            "fixture",
            &[EmbeddingWrite {
                message_id: "message-1",
                vector: &[127],
                norm: 127.0,
            }],
        )
        .unwrap();
    value.messages[0].content = "changed canonical context".into();
    value.messages[0].created_at = Some(123);
    storage.replace_conversation(&value).unwrap();
    assert_eq!(storage.embedding_count("fixture").unwrap(), 1);
    assert!(storage.messages_needing_embeddings().unwrap().is_empty());
    assert_eq!(
        storage.view("message-1", 0).unwrap()[0].content,
        "changed canonical context"
    );
    value.messages[0].search_projection = Some("different visible evidence".into());
    storage.replace_conversation(&value).unwrap();
    assert_eq!(storage.embedding_count("fixture").unwrap(), 0);
    assert_eq!(storage.messages_needing_embeddings().unwrap().len(), 1);
    value.messages[0].search_projection = Some(String::new());
    storage.replace_conversation(&value).unwrap();
    assert!(storage.messages_needing_embeddings().unwrap().is_empty());
    value.messages[0].search_projection = None;
    storage.replace_conversation(&value).unwrap();
    assert_eq!(storage.messages_needing_embeddings().unwrap().len(), 1);
}

#[test]
fn sessions_in_one_database_have_independent_checkpoints_and_reconciliation() {
    use crate::ingestion::SourceIdentity;
    use crate::provider::Provider;
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("mash.sqlite3");
    let physical = directory.path().join("history.db");
    let mut storage = Storage::open_writer(&database).unwrap();
    let mut sources = Vec::new();
    for (session, path, modified) in [
        ("keep", physical.clone(), 100),
        ("deleted", physical.clone(), 100),
        ("archived", physical.clone(), 1),
        ("outside", PathBuf::from("/outside/history.db"), 100),
    ] {
        let source = SourceIdentity {
            path,
            session: Some(session.into()),
        };
        let mut value = conversation(&format!("{session} searchable evidence"));
        value.id = session.into();
        value.messages[0].id = format!("{session}-message");
        value.source = source.clone();
        storage.replace_conversation(&value).unwrap();
        storage
            .record_source_checkpoint(Provider::Codex, &source, &SourceStamp::file(10, modified))
            .unwrap();
        sources.push(source);
    }
    storage.commit_writer().unwrap();
    drop(storage);
    let mut storage = Storage::open_writer(&database).unwrap();
    assert_eq!(storage.counts().unwrap().conversations, 4);
    let paths: Vec<(String, String)> = storage.connection.prepare(
        "SELECT source_path, source_session FROM conversations WHERE id IN ('keep','deleted') ORDER BY id"
    ).unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap()
        .collect::<Result<_, _>>().unwrap();
    assert!(
        paths
            .iter()
            .all(|(path, _)| path == &physical.to_string_lossy())
    );
    assert_ne!(paths[0].1, paths[1].1);
    assert!(
        storage
            .source_checkpoint_matches(Provider::Codex, &sources[0], &SourceStamp::file(10, 100))
            .unwrap()
    );
    assert!(
        !storage
            .source_checkpoint_matches(
                Provider::ClaudeCode,
                &sources[0],
                &SourceStamp::file(10, 100)
            )
            .unwrap()
    );

    let observed = [sources[0].key()].into();
    assert_eq!(
        storage
            .purge_missing_sources(Provider::Codex, &observed, &[physical], Some(50))
            .unwrap(),
        1
    );
    storage.commit_writer().unwrap();
    assert!(
        storage
            .search("deleted", 10, None, None)
            .unwrap()
            .is_empty()
    );
    for session in ["keep", "archived", "outside"] {
        assert_eq!(storage.search(session, 10, None, None).unwrap().len(), 1);
    }
    assert!(
        !storage
            .source_checkpoint_matches(Provider::Codex, &sources[1], &SourceStamp::file(10, 100))
            .unwrap()
    );
    assert!(
        storage
            .source_checkpoint_matches(Provider::Codex, &sources[2], &SourceStamp::file(10, 1))
            .unwrap()
    );
    drop(storage);
    let mut storage = Storage::open_writer(&database).unwrap();
    assert!(storage.remove_source(Provider::Codex, &sources[0]).unwrap());
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().conversations, 2);
    assert_eq!(storage.search("archived", 10, None, None).unwrap().len(), 1);
}

#[test]
fn literal_search_handles_urls_hyphens_questions_and_phrases() {
    let directory = tempfile::tempdir().unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("mash.sqlite3")).unwrap();
    storage
        .replace_conversation(&conversation_with_messages(&[
            (
                "url",
                "https://github.com/cBioPortal/cbioportal-core/pull/180",
            ),
            ("model", "use voyage-code-4"),
            ("question", "why is Redis faster than PostgreSQL"),
            ("phrase", "one shared index per node group"),
            ("separated", "one shared separate index per node group"),
        ]))
        .unwrap();
    storage.commit_writer().unwrap();
    for (query, expected) in [
        (
            "https://github.com/cBioPortal/cbioportal-core/pull/180",
            "url",
        ),
        ("voyage-code-4", "model"),
        ("why is Redis faster than PostgreSQL?", "question"),
        ("\"shared index\"", "phrase"),
        ("\"shared index", "phrase"),
    ] {
        let hits = storage.search(query, 10, None, None).unwrap();
        assert_eq!(hits.len(), 1, "query {query}");
        assert_eq!(hits[0].id, expected);
    }
    assert!(storage.search("!!!", 10, None, None).is_err());
}

#[test]
fn full_rebuild_is_idempotent() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let mut storage = Storage::open(&directory.path().join("mash.sqlite3")).expect("open database");
    storage
        .rebuild_derived_search_state()
        .expect("first rebuild");
    storage
        .rebuild_derived_search_state()
        .expect("second rebuild");
    let counts = storage.counts().expect("counts");
    assert_eq!(counts.conversations, 0);
    assert_eq!(counts.messages, 0);
    assert_eq!(counts.embeddings, 0);
}

#[test]
fn writer_checkpoint_survives_a_later_rollback() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut writer = Storage::open_writer(&path).expect("writer");
    writer
        .replace_conversation(&conversation("durable needle"))
        .expect("durable mutation");
    writer
        .record_source_checkpoint(
            crate::provider::Provider::Codex,
            &crate::ingestion::SourceIdentity::file("/tmp/session-1.jsonl"),
            &SourceStamp::file(100, 200),
        )
        .expect("durable source checkpoint");
    writer.checkpoint_writer().expect("checkpoint batch");
    writer
        .replace_conversation(&conversation("rolled back text"))
        .expect("later mutation");
    drop(writer);

    let storage = Storage::open_existing(&path).expect("reopen database");
    assert_eq!(
        storage
            .search("durable", 10, None, None)
            .expect("search committed batch")
            .len(),
        1
    );
    assert!(
        storage
            .source_checkpoint_matches(
                crate::provider::Provider::Codex,
                &crate::ingestion::SourceIdentity::file("/tmp/session-1.jsonl"),
                &SourceStamp::file(100, 200)
            )
            .expect("read checkpoint")
    );
}

#[test]
fn incremental_fts_and_canonical_changes_roll_back_together() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut writer = Storage::open_writer(&path).expect("writer");
    writer
        .replace_conversation(&conversation("durable old term"))
        .expect("seed conversation");
    writer.commit_writer().expect("commit seed");
    drop(writer);

    let mut writer = Storage::open_writer(&path).expect("replacement writer");
    writer
        .replace_conversation(&conversation("uncommitted new term"))
        .expect("replace conversation");
    assert_eq!(
        writer
            .finalize_pending_fts_updates(u64::MAX)
            .expect("incremental FTS finalization"),
        FtsRefreshStrategy::Incremental
    );
    assert_eq!(
        writer.search("uncommitted", 10, None, None).unwrap().len(),
        1
    );
    drop(writer);

    let storage = Storage::open_existing(&path).expect("reopen database");
    assert_eq!(storage.search("durable", 10, None, None).unwrap().len(), 1);
    assert!(
        storage
            .search("uncommitted", 10, None, None)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        storage.view("message-1", 0).unwrap()[0].content,
        "durable old term"
    );
}

#[test]
fn bulk_fts_and_canonical_changes_roll_back_together() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut writer = Storage::open_writer(&path).expect("writer");
    writer
        .replace_conversation(&conversation("durable bulk term"))
        .expect("seed conversation");
    writer.commit_writer().expect("commit seed");
    drop(writer);

    let mut writer = Storage::open_writer(&path).expect("replacement writer");
    writer
        .replace_conversation(&conversation("uncommitted bulk replacement"))
        .expect("replace conversation");
    assert_eq!(
        writer
            .finalize_pending_fts_updates(1)
            .expect("bulk FTS finalization"),
        FtsRefreshStrategy::Bulk
    );
    assert_eq!(
        writer.search("replacement", 10, None, None).unwrap().len(),
        1
    );
    drop(writer);

    let storage = Storage::open_existing(&path).expect("reopen database");
    assert_eq!(storage.search("durable", 10, None, None).unwrap().len(), 1);
    assert!(
        storage
            .search("replacement", 10, None, None)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        storage.view("message-1", 0).unwrap()[0].content,
        "durable bulk term"
    );
}

#[test]
fn fts_strategy_switches_at_the_declared_boundary() {
    let directory = tempfile::tempdir().expect("temporary directory");
    for (changed, expected) in [
        (8_usize, FtsRefreshStrategy::Incremental),
        (9, FtsRefreshStrategy::Bulk),
    ] {
        let path = directory.path().join(format!("changed-{changed}.sqlite3"));
        let original = (0..10)
            .map(|index| (format!("message-{index}"), format!("original {index}")))
            .collect::<Vec<_>>();
        let original_refs = original
            .iter()
            .map(|(id, content)| (id.as_str(), content.as_str()))
            .collect::<Vec<_>>();
        let mut writer = Storage::open_writer(&path).expect("writer");
        writer
            .replace_conversation(&conversation_with_messages(&original_refs))
            .expect("seed messages");
        writer.commit_writer().expect("commit seed");
        drop(writer);

        let replacement = original
            .iter()
            .enumerate()
            .map(|(index, (id, content))| {
                let content = if index < changed {
                    format!("changed {index}")
                } else {
                    content.clone()
                };
                (id.clone(), content)
            })
            .collect::<Vec<_>>();
        let replacement_refs = replacement
            .iter()
            .map(|(id, content)| (id.as_str(), content.as_str()))
            .collect::<Vec<_>>();
        let mut writer = Storage::open_writer(&path).expect("replacement writer");
        writer
            .replace_conversation(&conversation_with_messages(&replacement_refs))
            .expect("replace messages");
        let threshold = writer.measured_fts_bulk_threshold().unwrap();
        assert_eq!(threshold, 9);
        assert_eq!(
            writer.finalize_pending_fts_updates(threshold).unwrap(),
            expected,
            "changed messages: {changed}"
        );
    }

    let deletion_path = directory.path().join("deletion.sqlite3");
    let original = (0..10)
        .map(|index| (format!("message-{index}"), format!("original {index}")))
        .collect::<Vec<_>>();
    let original_refs = original
        .iter()
        .map(|(id, content)| (id.as_str(), content.as_str()))
        .collect::<Vec<_>>();
    let mut writer = Storage::open_writer(&deletion_path).expect("deletion writer");
    writer
        .replace_conversation(&conversation_with_messages(&original_refs))
        .expect("seed deletion corpus");
    writer.commit_writer().expect("commit deletion corpus");
    drop(writer);
    let retained_refs = original_refs[..5].to_vec();
    let mut writer = Storage::open_writer(&deletion_path).expect("deletion writer");
    writer
        .replace_conversation(&conversation_with_messages(&retained_refs))
        .expect("delete half the messages");
    let threshold = writer.measured_fts_bulk_threshold().unwrap();
    assert_eq!(threshold, 9, "deletions use the pre-transaction corpus");
    assert_eq!(
        writer.finalize_pending_fts_updates(threshold).unwrap(),
        FtsRefreshStrategy::Incremental
    );
}

#[test]
fn incremental_and_bulk_fts_produce_equivalent_results() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let incremental_path = directory.path().join("incremental.sqlite3");
    let bulk_path = directory.path().join("bulk.sqlite3");
    for (path, cutoff) in [(&incremental_path, u64::MAX), (&bulk_path, 1)] {
        let mut writer = Storage::open_writer(path).expect("writer");
        writer
            .replace_conversation(&conversation_with_messages(&[
                ("message-1", "alpha shared"),
                ("message-2", "beta shared"),
                ("message-3", "removed sentinel"),
            ]))
            .expect("seed messages");
        writer
            .finalize_pending_fts_updates(cutoff)
            .expect("seed FTS");
        writer
            .replace_conversation(&conversation_with_messages(&[
                ("message-1", "gamma shared"),
                ("message-4", "delta shared"),
            ]))
            .expect("replace messages");
        writer
            .finalize_pending_fts_updates(cutoff)
            .expect("refresh FTS");
        writer.commit_writer().expect("commit corpus");
    }

    let incremental = Storage::open_existing(&incremental_path).unwrap();
    let bulk = Storage::open_existing(&bulk_path).unwrap();
    for (query, limit, provider, days) in [
        ("shared", 10, None, None),
        ("shared", 1, Some(crate::provider::Provider::Codex), None),
        (
            "shared",
            10,
            Some(crate::provider::Provider::Codex),
            Some(1),
        ),
        ("gamma", 10, Some(crate::provider::Provider::Codex), None),
        ("removed", 10, None, None),
    ] {
        let ids = |storage: &Storage| {
            storage
                .search(query, limit, provider, days)
                .unwrap()
                .into_iter()
                .map(|hit| hit.id)
                .collect::<Vec<_>>()
        };
        let incremental_ids = ids(&incremental);
        let bulk_ids = ids(&bulk);
        assert_eq!(incremental_ids, bulk_ids, "query {query}");
        if days.is_some() {
            assert!(
                !incremental_ids.is_empty(),
                "recency-filter equivalence must exercise matching rows"
            );
        }
    }
    assert!(
        incremental
            .search("alpha", 10, None, None)
            .unwrap()
            .is_empty()
    );
    assert!(
        incremental
            .search("beta", 10, None, None)
            .unwrap()
            .is_empty()
    );
    assert!(
        incremental
            .search("removed", 10, None, None)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn search_projection_controls_fts_embeddings_and_rerank_text_not_view() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut corpus = conversation_with_messages(&[
        ("tool-only", "private tool payload"),
        ("mixed", "visible request\nprivate mixed payload"),
        ("ordinary", "ordinary searchable text"),
    ]);
    corpus.messages[0].search_projection = Some(String::new());
    corpus.messages[1].search_projection = Some("visible request".to_owned());

    let mut writer = Storage::open_writer(&path).expect("writer");
    writer
        .replace_conversation(&corpus)
        .expect("store projected messages");
    let pending = writer
        .messages_needing_embeddings()
        .expect("embedding selection");
    assert_eq!(
        pending
            .iter()
            .map(|message| (message.id.as_str(), message.content.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("mixed", "visible request"),
            ("ordinary", "ordinary searchable text")
        ]
    );
    writer
        .replace_embeddings(
            "generation",
            &[
                EmbeddingWrite {
                    message_id: "tool-only",
                    vector: &[127],
                    norm: 127.0,
                },
                EmbeddingWrite {
                    message_id: "mixed",
                    vector: &[126],
                    norm: 126.0,
                },
                EmbeddingWrite {
                    message_id: "ordinary",
                    vector: &[125],
                    norm: 125.0,
                },
            ],
        )
        .expect("seed semantic vectors");
    writer.commit_writer().expect("commit projected messages");

    let storage = Storage::open_existing(&path).expect("open projected corpus");
    let counts = storage.counts().expect("counts");
    assert_eq!(counts.messages, 3);
    assert_eq!(counts.searchable_messages, 2);
    assert!(
        storage
            .search("private", 10, None, None)
            .unwrap()
            .is_empty()
    );
    assert_eq!(storage.search("visible", 10, None, None).unwrap().len(), 1);
    assert_eq!(storage.search("ordinary", 10, None, None).unwrap().len(), 1);

    let vectors = storage
        .semantic_chunks("generation", None, None)
        .expect("semantic vectors");
    assert_eq!(vectors.chunks.len(), 1);
    assert_eq!(vectors.chunks[0].message_rowids, [2, 3]);
    storage
        .connection
        .execute(
            "DELETE FROM message_embeddings WHERE message_id = 'ordinary'",
            [],
        )
        .expect("remove one searchable vector");
    assert_eq!(storage.embedding_count("generation").unwrap(), 2);
    assert_eq!(counts.searchable_messages, 2);
    assert!(
        !storage
            .semantic_coverage_is_complete("generation")
            .expect("exact semantic coverage")
    );
    assert_eq!(
        storage
            .search_documents(&["ordinary", "mixed"])
            .expect("rerank documents"),
        vec![
            "ordinary searchable text".to_owned(),
            "visible request".to_owned()
        ]
    );
    let context = storage.view("mixed", 1).expect("canonical view");
    assert_eq!(context[0].content, "private tool payload");
    assert_eq!(context[1].content, "visible request\nprivate mixed payload");
}

#[test]
fn bulk_fts_refresh_preserves_unchanged_embeddings() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut writer = Storage::open_writer(&path).expect("writer");
    writer
        .replace_conversation(&conversation_with_messages(&[
            ("message-1", "first"),
            ("message-2", "second"),
        ]))
        .expect("seed messages");
    writer
        .replace_embeddings(
            "generation-a",
            &[
                EmbeddingWrite {
                    message_id: "message-1",
                    vector: &[127],
                    norm: 127.0,
                },
                EmbeddingWrite {
                    message_id: "message-2",
                    vector: &[127],
                    norm: 127.0,
                },
            ],
        )
        .expect("seed embeddings");
    writer
        .finalize_pending_fts_updates(1)
        .expect("seed bulk FTS");

    writer
        .replace_conversation(&conversation_with_messages(&[
            ("message-1", "changed first"),
            ("message-2", "second"),
        ]))
        .expect("change one message");
    assert_eq!(
        writer.finalize_pending_fts_updates(1).unwrap(),
        FtsRefreshStrategy::Bulk
    );
    assert_eq!(writer.embedding_count("generation-a").unwrap(), 1);
    assert_eq!(
        writer
            .messages_needing_embeddings()
            .unwrap()
            .into_iter()
            .map(|message| message.id)
            .collect::<Vec<_>>(),
        ["message-1"]
    );
}

#[ignore = "manual FTS crossover benchmark"]
#[test]
fn benchmark_fts_refresh_crossover() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let benchmark_path = directory.path().join("benchmark.sqlite3");
    if let Some(source) = std::env::var_os("MASH_FTS_BENCH_DB") {
        let source = Connection::open(PathBuf::from(source)).expect("open benchmark source");
        source
            .execute(
                "VACUUM INTO ?1",
                [benchmark_path.to_string_lossy().as_ref()],
            )
            .expect("copy benchmark database");
    } else {
        let mut writer = Storage::open_writer(&benchmark_path).expect("writer");
        let messages = (0..25_000)
            .map(|index| crate::ingestion::NormalizedMessage {
                id: format!("message-{index:05}"),
                ordinal: i64::from(index),
                context: crate::message_context::MessageContext::Linear,
                role: crate::role::Role::User,
                content: format!("representative searchable text number {index}"),
                search_projection: None,
                created_at: Some(1),
            })
            .collect();
        let mut corpus = conversation("");
        corpus.messages = messages;
        writer
            .replace_conversation(&corpus)
            .expect("seed benchmark corpus");
        writer.commit_writer().expect("commit benchmark corpus");
    }

    let reader = Storage::open_existing(&benchmark_path).expect("benchmark reader");
    let total = reader.counts().expect("benchmark counts").messages;
    drop(reader);
    let mut deltas = vec![
        1,
        10,
        100,
        1_000,
        10_000,
        total / 10,
        total / 2,
        total.saturating_mul(75) / 100,
        total.saturating_mul(85) / 100,
        total.saturating_mul(90) / 100,
        total.saturating_mul(95) / 100,
        total,
    ];
    deltas.retain(|delta| *delta > 0 && *delta <= total);
    deltas.sort_unstable();
    deltas.dedup();

    eprintln!("fts benchmark corpus_messages={total}");
    for delta in deltas {
        let mut incremental = Vec::new();
        let mut bulk = Vec::new();
        for repetition in 0..3 {
            let order = if repetition % 2 == 0 {
                [(u64::MAX, &mut incremental), (1, &mut bulk)]
            } else {
                [(1, &mut bulk), (u64::MAX, &mut incremental)]
            };
            for (cutoff, timings) in order {
                let mut writer = Storage::open_writer(&benchmark_path).expect("benchmark writer");
                writer
                    .connection
                    .execute(
                        "INSERT INTO pending_fts_messages(message_id)
                         SELECT id FROM messages ORDER BY id LIMIT ?1",
                        [i64::try_from(delta).expect("delta")],
                    )
                    .expect("stage benchmark messages");
                writer
                    .connection
                    .execute(
                        "UPDATE messages SET content = content || ' changed'
                          WHERE id IN (SELECT message_id FROM pending_fts_messages)",
                        [],
                    )
                    .expect("mutate benchmark messages");
                let started = std::time::Instant::now();
                writer
                    .finalize_pending_fts_updates(cutoff)
                    .expect("benchmark FTS finalization");
                timings.push(started.elapsed());
                drop(writer);
            }
        }
        incremental.sort_unstable();
        bulk.sort_unstable();
        eprintln!(
            "delta={delta} incremental_ms={:.3} bulk_ms={:.3}",
            incremental[1].as_secs_f64() * 1_000.0,
            bulk[1].as_secs_f64() * 1_000.0
        );
    }
}

#[test]
fn full_rebuild_defers_per_message_search_writes() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut storage = Storage::open_writer(&path).expect("writer");
    storage
        .defer_search_updates()
        .expect("defer derived writes");
    storage
        .replace_conversation(&conversation("deferred needle"))
        .expect("insert conversation");
    assert_eq!(
        storage
            .connection
            .query_row("SELECT count(*) FROM message_fts", [], |row| row
                .get::<_, i64>(0))
            .expect("FTS count before rebuild"),
        0
    );

    storage
        .rebuild_derived_search_state()
        .expect("bulk rebuild");
    assert_eq!(
        storage
            .search("needle", 10, None, None)
            .expect("search rebuilt FTS")
            .len(),
        1
    );
}

#[test]
fn concurrent_writer_is_rejected_and_first_writer_can_commit() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut first = Storage::open_writer(&path).expect("first writer");
    let error = Storage::open_writer(&path)
        .err()
        .expect("second writer rejected");
    assert_eq!(error.error.kind, crate::ErrorKind::IndexBusy);
    first.commit_writer().expect("first writer commits");
    Storage::open_writer(&path).expect("writer available after commit");
}

#[test]
fn conversation_reconciliation_writes_only_changed_messages() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut storage = Storage::open_writer(&path).expect("writer");
    let first = storage
        .replace_conversation(&conversation("first"))
        .expect("initial insert");
    assert_eq!(first.changed_message_ids, ["message-1"]);
    storage
        .replace_embeddings(
            "generation-a",
            &[EmbeddingWrite {
                message_id: "message-1",
                vector: &[127, 0],
                norm: 127.0,
            }],
        )
        .expect("seed embedding");
    let unchanged = storage
        .replace_conversation(&conversation("first"))
        .expect("unchanged refresh");
    assert!(unchanged.unchanged);
    assert_eq!(unchanged.changed_message_ids, Vec::<String>::new());
    assert!(
        storage
            .messages_needing_embeddings()
            .expect("embedding selection")
            .is_empty()
    );
    let changed = storage
        .replace_conversation(&conversation("second"))
        .expect("changed refresh");
    assert_eq!(changed.changed_message_ids, ["message-1"]);
    assert_eq!(
        storage
            .messages_needing_embeddings()
            .expect("embedding selection")
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        ["message-1"]
    );
}

#[test]
fn conversation_reconciliation_replaces_a_changed_message_id_at_the_same_ordinal() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut storage = Storage::open_writer(&path).expect("writer");
    storage
        .replace_conversation(&conversation("first"))
        .expect("initial insert");

    let mut replacement = conversation("second");
    replacement.messages[0].id = "message-2".to_owned();
    let change = storage
        .replace_conversation(&replacement)
        .expect("replace message identity");

    assert_eq!(change.changed_message_ids, ["message-2"]);
    assert_eq!(change.removed_messages, 1);
    assert_eq!(storage.counts().expect("counts").messages, 1);
    storage.commit_writer().expect("commit replacement");
    assert!(storage.search("first", 10, None, None).unwrap().is_empty());
    assert_eq!(
        storage.search("second", 10, None, None).unwrap()[0].id,
        "message-2"
    );
}

#[test]
fn conversation_reconciliation_inserts_and_reorders_stable_ids_with_tree_context() {
    fn history(insert: bool) -> crate::ingestion::Conversation {
        let mut value = conversation("original request");
        let shift = i64::from(insert);
        value.messages[0].context = crate::message_context::MessageContext::Root;
        value.messages[0].ordinal = shift;
        value.messages.push(crate::ingestion::NormalizedMessage {
            id: "reply".into(),
            ordinal: shift + 1,
            content: "original reply".into(),
            role: crate::role::Role::Assistant,
            context: crate::message_context::MessageContext::Parent { ordinal: shift },
            search_projection: None,
            created_at: None,
        });
        if insert {
            value.messages.insert(
                0,
                crate::ingestion::NormalizedMessage {
                    id: "inserted".into(),
                    ordinal: 0,
                    content: "recovered dialogue".into(),
                    role: crate::role::Role::User,
                    context: crate::message_context::MessageContext::Root,
                    search_projection: None,
                    created_at: None,
                },
            );
        }
        value
    }
    let directory = tempfile::tempdir().unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    let original = history(false);
    storage.replace_conversation(&original).unwrap();
    let shifted = history(true);
    storage.replace_conversation(&shifted).unwrap();
    storage.commit_writer().unwrap();
    let viewed = storage.view("reply", 4).unwrap();
    assert_eq!(
        viewed.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        ["message-1", "reply"]
    );
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    // Removing the inserted item shifts the same IDs down into occupied slots.
    storage.replace_conversation(&original).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 2);
    assert!(
        storage
            .search("recovered", 5, None, None)
            .unwrap()
            .is_empty()
    );
    assert_eq!(storage.view("reply", 0).unwrap()[0].ordinal, 1);

    // A later failure must not commit temporary ordinals or partial context.
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    storage.begin_provider_scan().unwrap();
    let mut invalid = history(true);
    invalid.messages[2].context = crate::message_context::MessageContext::Parent { ordinal: 9 };
    assert!(storage.replace_conversation(&invalid).is_err());
    storage.rollback_provider_scan().unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 2);
    assert_eq!(storage.view("message-1", 0).unwrap()[0].ordinal, 0);
    assert_eq!(storage.view("reply", 0).unwrap()[0].ordinal, 1);
}

#[test]
fn purging_a_conversation_removes_its_staged_fts_rows() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut writer = Storage::open_writer(&path).expect("writer");
    writer
        .replace_conversation(&conversation("disappearing sentinel"))
        .expect("seed conversation");
    writer.commit_writer().expect("commit seed");
    drop(writer);

    let mut writer = Storage::open_writer(&path).expect("purge writer");
    assert_eq!(
        writer
            .purge_missing_sources(
                crate::provider::Provider::Codex,
                &BTreeSet::new(),
                &[std::path::PathBuf::from("/tmp")],
                None,
            )
            .expect("purge missing source"),
        1
    );
    writer.commit_writer().expect("commit purge");
    assert!(
        writer
            .search("disappearing", 10, None, None)
            .unwrap()
            .is_empty()
    );
    assert_eq!(writer.counts().unwrap().messages, 0);
}

#[test]
fn tombstone_prevents_reinsertion() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let mut writer = Storage::open_writer(&path).expect("writer");
    writer
        .replace_conversation(&conversation("remember me"))
        .expect("insert");
    writer.commit_writer().expect("commit");
    drop(writer);

    let mut storage = Storage::open(&path).expect("open database");
    assert!(storage.forget("session-1").expect("forget"));
    drop(storage);

    let mut writer = Storage::open_writer(&path).expect("second writer");
    let change = writer
        .replace_conversation(&conversation("remember me"))
        .expect("tombstone check");
    assert!(change.tombstoned);
    writer.commit_writer().expect("commit");
    assert_eq!(writer.counts().expect("counts").conversations, 0);
}

#[test]
fn ordinary_writer_commits_leave_wal_checkpointing_to_sqlite() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let wal_path = std::path::PathBuf::from(format!("{}-wal", path.display()));
    let mut writer = Storage::open_writer(&path).expect("writer");
    writer
        .replace_conversation(&conversation("small incremental write"))
        .expect("insert conversation");
    writer.commit_writer().expect("commit writer");

    assert!(
        std::fs::metadata(wal_path).is_ok_and(|metadata| metadata.len() > 0),
        "a small commit must not force a blocking WAL truncation"
    );
}
