use super::*;

fn decoded(value: Value) -> Message {
    serde_json::from_value(value).unwrap()
}

#[test]
fn replacements_rewinds_and_checkpoints_release_retained_memory() {
    let mut replay = Replay::start(serde_json::from_value(header()).unwrap()).unwrap();
    replay
        .message(
            decoded(message("a", "user", json!("Request"))),
            Origin::Recorded,
        )
        .unwrap();
    for size in [1000, 1, 2000, 1] {
        replay
            .message(
                decoded(message("b", "gemini", json!("x".repeat(size)))),
                Origin::Recorded,
            )
            .unwrap();
        assert_eq!(replay.messages.len(), 2);
        assert_eq!(
            replay.retained_bytes,
            replay.messages.iter().map(retained_bytes).sum::<usize>()
        );
    }
    replay
        .record(Record::Rewind { target: "b".into() })
        .unwrap();
    assert_eq!(replay.retained_bytes, retained_bytes(&replay.messages[0]));
    replay
        .update(
            serde_json::from_value(
                json!({"messages":[message("b","gemini",json!("Checkpoint answer"))]}),
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(replay.retained_bytes, retained_bytes(&replay.messages[0]));
    assert!(matches!(replay.positions["b"].origin, Origin::ModelContext));
    replay
        .update(serde_json::from_value(json!({"messages":[]})).unwrap())
        .unwrap();
    assert_eq!(replay.retained_bytes, 0);
    assert!(replay.positions.is_empty());
}

#[test]
fn message_count_limit_accepts_updates_and_reuses_rewound_positions() {
    let mut replay = Replay::start(serde_json::from_value(header()).unwrap()).unwrap();
    for i in 0..MAX_MESSAGES {
        replay
            .message(
                decoded(message(&i.to_string(), "user", json!(""))),
                Origin::Recorded,
            )
            .unwrap();
    }
    assert_eq!(replay.messages.len(), MAX_MESSAGES);
    assert!(replay.retained_bytes <= MAX_REPLAY_BYTES);
    assert!(
        replay
            .message(decoded(message("new", "user", json!(""))), Origin::Recorded)
            .is_err()
    );
    replay
        .message(decoded(message("0", "user", json!(""))), Origin::Recorded)
        .unwrap();
    replay
        .record(Record::Rewind {
            target: (MAX_MESSAGES - 1).to_string(),
        })
        .unwrap();
    replay
        .message(decoded(message("new", "user", json!(""))), Origin::Recorded)
        .unwrap();
    assert_eq!(replay.messages.len(), MAX_MESSAGES);
}

#[test]
fn late_session_kind_updates_apply_to_already_normalized_records() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let request = message("request", "user", json!("Original user request"));
    for kind in ["subagent", "main"] {
        let mut metadata = header();
        metadata["kind"] = json!("subagent");
        write(
            file.path(),
            &[metadata, request.clone(), json!({"$set":{"kind":kind}})],
        );
        let conversation = parse(file.path()).unwrap().conversation.unwrap();
        let expected = if kind == "main" {
            "Original user request"
        } else {
            ""
        };
        assert_eq!(
            conversation.messages.inline()[0]
                .search_projection
                .as_deref(),
            Some(expected)
        );
        assert_eq!(
            conversation.messages.inline()[0].content,
            "Original user request"
        );
    }
}

#[test]
fn nested_native_parts_keep_prose_without_repeated_untagged_tree_copies() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let text = "Validate mutation metadata. ".repeat(4000);
    let mut content = json!({"text":text});
    for _ in 0..80 {
        content = json!([content]);
    }
    write(
        file.path(),
        &[header(), message("request", "user", content)],
    );
    let conversation = parse(file.path()).unwrap().conversation.unwrap();
    assert_eq!(conversation.messages.inline()[0].content, text);
    assert_eq!(
        conversation.messages.inline()[0]
            .search_projection
            .as_deref(),
        Some(text.as_str())
    );
}

#[test]
fn structural_and_replay_overflow_preserve_the_previous_index_and_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let path = history(dir.path());
    let database = dir.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&path)).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let source = direct_scans(&options(&path))
        .unwrap()
        .remove(0)
        .sources
        .remove(0);
    let checkpoint = source.stamp().unwrap();

    for structural in [true, false] {
        write(&path, &fixture());
        let mut writer = std::io::BufWriter::new(
            std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap(),
        );
        if structural {
            // A small serialized record with many empty SDK parts previously
            // allocated an enormous typed/untagged container tree.
            let hostile = message("oversized", "user", json!(vec![json!({}); 65_536]));
            serde_json::to_writer(&mut writer, &hostile).unwrap();
            writeln!(writer).unwrap();
        } else {
            // Each record is below the 8 MiB cap; retained canonical text and
            // projections across the log must still have an aggregate bound.
            let text = "x".repeat(1024 * 1024);
            for i in 0..17 {
                serde_json::to_writer(
                    &mut writer,
                    &message(&format!("large-{i}"), "user", json!(text)),
                )
                .unwrap();
                writeln!(writer).unwrap();
            }
        }
        writer.flush().unwrap();
        let scan = direct_scans(&options(&path)).unwrap().remove(0);
        let current = &scan.sources[0];
        let error = crate::ingestion::DirectScan::parse_source(
            Provider::GeminiCli,
            current,
            &current.stamp().unwrap(),
        )
        .err()
        .unwrap();
        assert!(error.error.message.contains(if structural {
            "structural"
        } else {
            "retained replay"
        }));
        let mut storage = Storage::open_writer(&database).unwrap();
        assert!(index(&mut storage, &options(&path)).is_err());
        storage.commit_writer().unwrap();
        assert_eq!(storage.counts().unwrap().messages, 7);
        assert!(
            storage
                .source_checkpoint_matches(Provider::GeminiCli, &source.identity, &checkpoint)
                .unwrap()
        );
        assert!(
            !storage
                .search("cbioportal", 10, None, None)
                .unwrap()
                .is_empty()
        );
    }
    write(&path, &fixture());
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&path)).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 7);
}

#[test]
#[ignore = "manual native parser throughput measurement"]
fn benchmark_gemini_replay_unique_and_repeated_updates() {
    for repeated in [false, true] {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut writer = std::io::BufWriter::new(file.reopen().unwrap());
        serde_json::to_writer(&mut writer, &header()).unwrap();
        writeln!(writer).unwrap();
        let text = "Validate cBioPortal mutation metadata. ".repeat(50);
        for i in 0..5000 {
            let id = if repeated {
                "request".into()
            } else {
                format!("request-{i}")
            };
            serde_json::to_writer(&mut writer, &message(&id, "user", json!(text))).unwrap();
            writeln!(writer).unwrap();
        }
        writer.flush().unwrap();
        let mut times = Vec::new();
        for _ in 0..3 {
            let start = std::time::Instant::now();
            let conversation =
                std::hint::black_box(parse(file.path()).unwrap().conversation.unwrap());
            times.push(start.elapsed());
            assert_eq!(conversation.messages.len(), if repeated { 1 } else { 5000 });
        }
        times.sort();
        eprintln!(
            "Gemini replay: repeated={repeated}, records=5000, bytes={}, median_ms={:.1}",
            file.as_file().metadata().unwrap().len(),
            times[1].as_secs_f64() * 1000.0
        );
    }
}
