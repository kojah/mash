use super::*;

fn record(id: &str, content: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "type":"user", "sessionId":"retained-session", "uuid":id,
        "message":{"role":"user","content":content}
    }))
    .unwrap()
}

fn options(root: &Path) -> IndexOptions {
    IndexOptions {
        providers: [(Provider::ClaudeCode, vec![root.into()])].into(),
        roots_are_authoritative: true,
        since_days: None,
    }
}

fn seed_vectors(storage: &mut Storage, message_ids: &[&str]) {
    let vectors: Vec<_> = message_ids
        .iter()
        .map(|message_id| crate::storage::EmbeddingWrite {
            message_id,
            vector: &[127],
            norm: 127.0,
        })
        .collect();
    storage.replace_embeddings("fixture", &vectors).unwrap();
    storage.mark_semantic_index_ready("fixture").unwrap();
    storage.commit_and_continue_writer().unwrap();
}

fn index_with_slow_progress_and_safe_traces(
    storage: &mut Storage,
    options: &IndexOptions,
) -> IndexSummary {
    let (traces, dispatcher) = crate::observability::test_support::capture();
    let (sink, _slow_client) = tokio::sync::mpsc::channel(1);
    sink.try_send(crate::progress::Event::default()).unwrap();
    let summary = tracing::dispatcher::with_default(&dispatcher, || {
        crate::progress::on_blocking_thread(Some(sink), || index(storage, options).unwrap())
    });
    let traces = String::from_utf8(traces.lock().unwrap().clone()).unwrap();
    assert!(traces.contains("source snapshot rejected"));
    assert!(!traces.contains("PRIVATE-TRACE-RECORD-CANARY"));
    assert_eq!(
        summary.record_diagnostics.samples.len(),
        1,
        "final diagnostics survive a full progress queue"
    );
    summary
}

#[test]
fn malformed_refresh_retains_canonical_messages_vectors_and_checkpoint_while_other_sources_advance()
{
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("session.jsonl");
    let original = format!(
        "{}\n{}\n",
        record("first", "retained first sentinel"),
        record("second", "retained second sentinel")
    );
    fs::write(&source, &original).unwrap();
    let parsed = parse_claude(&source).unwrap().conversation.unwrap();
    let first = parsed.messages.inline()[0].id.clone();
    let second = parsed.messages.inline()[1].id.clone();
    let checkpoint = source_stamp(&source).unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    index(&mut storage, &options(directory.path())).unwrap();
    seed_vectors(&mut storage, &[&first, &second]);

    fs::write(
        &source,
        format!(
            "{}\n{{\"sessionId\":[\"PRIVATE-TRACE-RECORD-CANARY\"]}}\n{}\n",
            record("first", "damaged fragment sentinel"),
            record("third", "uncommitted third sentinel")
        ),
    )
    .unwrap();
    let healthy = directory.path().join("healthy.jsonl");
    write_claude_session(&healthy, "healthy-session", "healthy source sentinel");
    let partial =
        index_with_slow_progress_and_safe_traces(&mut storage, &options(directory.path()));
    storage.commit_and_continue_writer().unwrap();
    assert_eq!(partial.malformed_records, 1);
    assert_record_location(&partial, &source);
    assert_eq!(
        partial.changed_messages, 1,
        "only the healthy source changes"
    );
    assert_eq!(partial.removed_messages, 0);
    assert_eq!(
        storage.view(&first, 0).unwrap()[0].content,
        "retained first sentinel"
    );
    assert_eq!(
        storage.view(&second, 0).unwrap()[0].content,
        "retained second sentinel"
    );
    assert_eq!(storage.embedding_count("fixture").unwrap(), 2);
    for query in ["damaged fragment", "uncommitted third"] {
        assert_eq!(storage.search(query, 10, None, None).unwrap().len(), 0);
    }
    assert_eq!(
        storage
            .search("healthy source", 10, None, None)
            .unwrap()
            .len(),
        1
    );
    let identity = SourceIdentity::file(&source);
    assert!(
        storage
            .source_checkpoint_matches(Provider::ClaudeCode, &identity, &checkpoint)
            .unwrap()
    );
    assert!(
        !storage
            .source_checkpoint_matches(
                Provider::ClaudeCode,
                &identity,
                &source_stamp(&source).unwrap()
            )
            .unwrap()
    );

    fs::write(
        &source,
        format!(
            "{}{}\n",
            original,
            record("third", "repaired third sentinel")
        ),
    )
    .unwrap();
    let repaired = index(&mut storage, &options(directory.path())).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(repaired.malformed_records, 0);
    assert_eq!(repaired.changed_messages, 1);
    assert_eq!(storage.counts().unwrap().messages, 4);
    assert_eq!(
        storage
            .search("repaired third", 10, None, None)
            .unwrap()
            .len(),
        1
    );
    assert!(
        storage
            .source_checkpoint_matches(
                Provider::ClaudeCode,
                &identity,
                &source_stamp(&source).unwrap()
            )
            .unwrap()
    );
}

fn assert_record_location(summary: &IndexSummary, source: &Path) {
    let issue = &summary.record_diagnostics.samples[0];
    assert_eq!(issue.source, source);
    assert_eq!(issue.line, 2);
    assert_eq!(issue.category, diagnostics::Category::Data);
}

#[test]
fn malformed_initial_source_admits_no_fragments_and_a_clean_retry_indexes_it() {
    for provider in [Provider::ClaudeCode, Provider::Codex] {
        check_initial_source_retry(provider);
    }
}

fn source_records(provider: Provider, text: &str) -> String {
    match provider {
        Provider::ClaudeCode => record("first", text),
        Provider::Codex => format!(
            "{}\n{}",
            serde_json::json!({"type":"session_meta","payload":{"id":"retained-session"}}),
            serde_json::json!({"type":"response_item","payload":{
                "type":"message","id":"first","role":"user",
                "content":[{"type":"input_text","text":text}]
            }})
        ),
        _ => unreachable!("fixture only supports the two tolerant JSONL adapters"),
    }
}

fn check_initial_source_retry(provider: Provider) {
    let directory = tempfile::tempdir().unwrap();
    let options = IndexOptions {
        providers: [(provider, vec![directory.path().into()])].into(),
        roots_are_authoritative: true,
        since_days: None,
    };
    let source = directory.path().join("session.jsonl");
    fs::write(
        &source,
        format!(
            "{}\nnot-json\n",
            source_records(provider, "initial partial sentinel")
        ),
    )
    .unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    let partial = index(&mut storage, &options).unwrap();
    storage.commit_and_continue_writer().unwrap();
    assert_eq!(partial.malformed_records, 1);
    assert_eq!(partial.changed_messages, 0);
    assert_eq!(storage.counts().unwrap().messages, 0);
    assert!(
        !storage
            .source_checkpoint_matches(
                provider,
                &SourceIdentity::file(&source),
                &source_stamp(&source).unwrap()
            )
            .unwrap()
    );
    fs::write(
        &source,
        format!(
            "{}\n",
            source_records(provider, "initial repaired sentinel")
        ),
    )
    .unwrap();
    let repaired = index(&mut storage, &options).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(repaired.malformed_records, 0);
    assert_eq!(repaired.changed_messages, 1);
    assert_eq!(
        storage
            .search("initial repaired", 10, None, None)
            .unwrap()
            .len(),
        1
    );
}
