//! Control the real parser/consumer boundary without relying on sleeps or CPU count.
use super::*;
use std::sync::mpsc::sync_channel;

fn handoff(
    paths: [&Path; 2],
    order: [usize; 2],
    mut consume: impl FnMut(usize, ParsedSource<'static>),
) {
    std::thread::scope(|scope| {
        let (ready_tx, ready_rx) = sync_channel(2);
        let (result_tx, result_rx) = sync_channel(1);
        let mut releases = Vec::new();
        for (index, path) in paths.into_iter().enumerate() {
            let (release_tx, release_rx) = sync_channel(1);
            releases.push(release_tx);
            let ready = ready_tx.clone();
            let results = result_tx.clone();
            scope.spawn(move || {
                let history = HistorySource::file(path);
                let mut stamp = history.stamp().unwrap();
                let parsed =
                    parse_source_with_retries(&history, &mut stamp, Provider::ClaudeCode.parser());
                let source = ParsedSource {
                    source: SourceFile {
                        history,
                        estimated_bytes: u64::try_from(stamp.size_bytes).unwrap(),
                        stamp,
                    },
                    parsed,
                    _reservation: None,
                };
                // Both parsers finish before either result reaches the consumer.
                ready.send(index).unwrap();
                if release_rx.recv_timeout(Duration::from_secs(30)).is_ok() {
                    let _ = results.send((index, source));
                }
            });
        }
        drop(ready_tx);
        drop(result_tx);
        let mut ready = [
            ready_rx.recv_timeout(Duration::from_secs(30)).unwrap(),
            ready_rx.recv_timeout(Duration::from_secs(30)).unwrap(),
        ];
        ready.sort_unstable();
        assert_eq!(ready, [0, 1]);
        for next in order {
            releases[next].send(()).unwrap();
            let (producer, source) = result_rx.recv_timeout(Duration::from_secs(30)).unwrap();
            assert_eq!(producer, next);
            // Applying the result is the acknowledgement before the next handoff.
            consume(producer, source);
        }
    });
}

fn canonical(path: &Path) -> Vec<(String, i64, String)> {
    let connection = rusqlite::Connection::open(path).unwrap();
    connection
        .prepare("SELECT id, ordinal, content FROM objects WHERE origin='provider' AND object_role='entry' ORDER BY ordinal")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn checkpoint_matches(storage: &Storage, path: &Path, stamp: &SourceStamp) -> bool {
    storage
        .source_checkpoint_matches(Provider::ClaudeCode, &SourceIdentity::file(path), stamp)
        .unwrap()
}

#[test]
fn claude_parser_handoff_orders_preserve_copies_prefixes_branches_and_restart_checkpoints() {
    for tails in [(false, false), (false, true), (true, true)] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("sources");
        fs::create_dir(&root).unwrap();
        let paths = [root.join("a.jsonl"), root.join("b.jsonl")];
        for (index, path) in paths.iter().enumerate() {
            let mut rows = vec![("common", None, "common sentinel")];
            if [tails.0, tails.1][index] {
                rows.push((
                    if index == 0 { "left" } else { "right" },
                    Some("common"),
                    if index == 0 {
                        "left sentinel"
                    } else {
                        "right sentinel"
                    },
                ));
            }
            transcript(path, &rows);
        }
        let mut baseline = None;
        for order in [[0, 1], [1, 0]] {
            let database = directory.path().join(format!("order-{}.sqlite3", order[0]));
            let mut storage = Storage::open_writer(&database).unwrap();
            let mut summary = empty_summary();
            storage.begin_provider_scan().unwrap();
            handoff([&paths[0], &paths[1]], order, |_, parsed| {
                assert!(
                    apply_parsed_source(&mut storage, Provider::ClaudeCode, parsed, &mut summary)
                        .unwrap()
                );
            });
            storage.finish_provider_scan().unwrap();
            storage.commit_writer().unwrap();
            assert_eq!(storage.counts().unwrap().conversations, 1);
            let expected = 1 + u64::from(tails.0) + u64::from(tails.1);
            assert_eq!(storage.counts().unwrap().messages, expected);
            for path in &paths {
                assert!(checkpoint_matches(
                    &storage,
                    path,
                    &source_stamp(path).unwrap()
                ));
            }
            let messages = canonical(&database);
            assert_eq!(
                messages[0].0,
                stable_id("claude-message", &["copied-thread", "common"])
            );
            for (ordinal, (id, stored_ordinal, _)) in messages.iter().enumerate() {
                assert_eq!(*stored_ordinal, i64::try_from(ordinal).unwrap());
                if ordinal != 0 {
                    let context = storage.view(id, 10).unwrap();
                    assert_eq!(context.len(), 2, "sibling must not enter branch context");
                    assert_eq!(context[0].id, messages[0].0);
                    assert_eq!(context[1].id, *id);
                }
            }
            if let Some(baseline) = &baseline {
                assert_eq!(
                    &messages, baseline,
                    "handoff order changed canonical identities or ordinals"
                );
            } else {
                baseline = Some(messages.clone());
            }
            drop(storage);
            for _ in 0..2 {
                let mut storage = Storage::open_writer(&database).unwrap();
                let summary = index(&mut storage, &options(&root)).unwrap();
                assert_eq!(summary.checkpoint_skipped_sources, 2);
                assert_eq!(summary.changed_messages, 0);
                assert_eq!(summary.removed_messages, 0);
                storage.commit_writer().unwrap();
                assert_eq!(canonical(&database), messages);
            }
        }
    }
}

#[test]
fn claude_parser_handoff_late_identity_conflict_rolls_back_contributions_and_all_checkpoints() {
    for order in [[0, 1], [1, 0]] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("sources");
        fs::create_dir(&root).unwrap();
        let paths = [root.join("a.jsonl"), root.join("b.jsonl")];
        for path in &paths {
            transcript(path, &[("common", None, "common sentinel")]);
        }
        let database = directory.path().join("index.sqlite3");
        let mut storage = Storage::open_writer(&database).unwrap();
        index(&mut storage, &options(&root)).unwrap();
        storage.commit_writer().unwrap();
        drop(storage);
        let before = canonical(&database);
        let old = paths.each_ref().map(|path| source_stamp(path).unwrap());
        transcript(
            &paths[order[0]],
            &[
                ("common", None, "common sentinel"),
                ("left", Some("common"), "left sentinel"),
            ],
        );
        transcript(
            &paths[order[1]],
            &[
                ("common", None, "conflicting sentinel"),
                ("right", Some("common"), "right sentinel"),
            ],
        );
        let new = paths.each_ref().map(|path| source_stamp(path).unwrap());
        let mut storage = Storage::open_writer(&database).unwrap();
        let mut summary = empty_summary();
        storage.begin_provider_scan().unwrap();
        handoff([&paths[0], &paths[1]], order, |producer, parsed| {
            let result =
                apply_parsed_source(&mut storage, Provider::ClaudeCode, parsed, &mut summary);
            if producer == order[0] {
                assert!(result.unwrap());
                assert_eq!(
                    summary.changed_messages, 1,
                    "conflict must follow an applied contribution"
                );
                assert!(checkpoint_matches(
                    &storage,
                    &paths[producer],
                    &new[producer]
                ));
            } else {
                assert_eq!(
                    result.unwrap_err().error.kind,
                    crate::ErrorKind::Configuration
                );
                storage.rollback_provider_scan().unwrap();
            }
        });
        storage.commit_writer().unwrap();
        assert_eq!(canonical(&database), before);
        for (index, path) in paths.iter().enumerate() {
            assert!(checkpoint_matches(&storage, path, &old[index]));
            assert!(!checkpoint_matches(&storage, path, &new[index]));
        }
        drop(storage);
        // Re-deriving from the surviving source exposes accidentally retained
        // contribution rows even when the canonical messages were rolled back.
        let mut storage = Storage::open_writer(&database).unwrap();
        storage
            .remove_source(
                Provider::ClaudeCode,
                &SourceIdentity::file(&paths[order[1]]),
            )
            .unwrap();
        storage.commit_writer().unwrap();
        assert_eq!(canonical(&database), before);
        drop(storage);
        transcript(
            &paths[order[1]],
            &[
                ("common", None, "common sentinel"),
                ("right", Some("common"), "right sentinel"),
            ],
        );
        for expected_skips in [0, 2] {
            let mut storage = Storage::open_writer(&database).unwrap();
            let summary = index(&mut storage, &options(&root)).unwrap();
            assert_eq!(summary.checkpoint_skipped_sources, expected_skips);
            storage.commit_writer().unwrap();
            assert_eq!(storage.counts().unwrap().messages, 3);
            for uuid in ["left", "right"] {
                let id = stable_id("claude-message", &["copied-thread", uuid]);
                assert_eq!(storage.view(&id, 10).unwrap().len(), 2);
            }
        }
    }
}
