use super::*;

mod malformed;

#[test]
fn event_only_claude_sources_remove_old_contributions_and_keep_restart_checkpoints() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.jsonl");
    let database = directory.path().join("index.sqlite3");
    let options = IndexOptions {
        providers: [(Provider::ClaudeCode, vec![directory.path().to_path_buf()])].into(),
        roots_are_authoritative: true,
        since_days: None,
    };
    write_claude_session(&path, "session", "old dialogue sentinel");
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 1);
    drop(storage);
    fs::write(&path, "{\"type\":\"started\",\"agentId\":\"alpha\"}\n{\"type\":\"result\",\"agentId\":\"beta\"}\n").unwrap();
    for restart in [false, true] {
        let mut storage = Storage::open_writer(&database).unwrap();
        let summary = index(&mut storage, &options).unwrap();
        assert_eq!(summary.malformed_records, 0);
        assert_eq!(summary.checkpoint_skipped_sources, u64::from(restart));
        storage.commit_writer().unwrap();
        assert_eq!(storage.counts().unwrap().messages, 0);
        assert_eq!(storage.counts().unwrap().conversations, 0);
        assert!(
            storage
                .source_checkpoint_matches(
                    Provider::ClaudeCode,
                    &SourceIdentity::file(&path),
                    &source_stamp(&path).unwrap()
                )
                .unwrap()
        );
    }
}

#[test]
fn claude_parent_and_agents_keep_independent_histories_and_checkpoints() {
    for reverse in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let parent_path = directory.path().join("parent.jsonl");
        write_claude_session(&parent_path, "session", "parent sentinel");
        let parent = parse_claude(&parent_path).unwrap().conversation.unwrap();
        let mut agents = Vec::new();
        for agent in ["alpha", "beta"] {
            let path = directory.path().join(format!("agent-{agent}.jsonl"));
            fs::write(&path, format!(
                "{{\"type\":\"user\",\"sessionId\":\"session\",\"agentId\":\"{agent}\",\"isSidechain\":true,\"uuid\":\"{agent}-message\",\"message\":{{\"role\":\"user\",\"content\":\"{agent} sentinel\"}}}}\n"
            )).unwrap();
            agents.push(parse_claude(&path).unwrap().conversation.unwrap());
        }
        assert_ne!(parent.id, agents[0].id);
        assert_ne!(agents[0].id, agents[1].id);
        let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
        // Emulate the old parser: the agent occupied its parent's session ID.
        let mut legacy = parse_claude(&agents[0].source.path)
            .unwrap()
            .conversation
            .unwrap();
        legacy.id = parent.id.clone();
        storage.replace_conversation(&legacy).unwrap();
        let mut conversations = vec![parent, agents.remove(0), agents.remove(0)];
        if reverse {
            conversations.reverse();
        }
        for conversation in &conversations {
            storage.replace_conversation(conversation).unwrap();
        }
        assert_eq!(storage.counts().unwrap().conversations, 3);
        assert_eq!(storage.counts().unwrap().messages, 3);
        for conversation in &conversations {
            let message = &conversation.messages.inline()[0];
            assert_eq!(
                storage.view(&message.id, 0).unwrap()[0].content,
                message.content
            );
        }
        let options = IndexOptions {
            providers: [(Provider::ClaudeCode, vec![directory.path().to_path_buf()])].into(),
            roots_are_authoritative: true,
            since_days: None,
        };
        index(&mut storage, &options).unwrap();
        let repeat = index(&mut storage, &options).unwrap();
        assert_eq!(repeat.checkpoint_skipped_sources, 3);
        assert_eq!(storage.counts().unwrap().messages, 3);
        write_claude_session(&parent_path, "session", "parent edited sentinel");
        index(&mut storage, &options).unwrap();
        assert_eq!(storage.counts().unwrap().messages, 3);
        fs::remove_file(directory.path().join("agent-alpha.jsonl")).unwrap();
        index(&mut storage, &options).unwrap();
        assert_eq!(storage.counts().unwrap().conversations, 2);
        assert_eq!(storage.counts().unwrap().messages, 2);
    }
}

#[test]
fn progress_exposes_parser_admission_without_history_content() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("session.jsonl");
    write_claude_session(&source, "session", "private-history-canary");
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    let options = IndexOptions {
        providers: [(Provider::ClaudeCode, vec![source])].into(),
        roots_are_authoritative: true,
        since_days: None,
    };
    let (sender, mut receiver) = tokio::sync::mpsc::channel(32);
    crate::progress::on_blocking_thread(Some(sender), || index(&mut storage, &options)).unwrap();
    let mut completed = None;
    while let Ok(event) = receiver.try_recv() {
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("private-history-canary"));
        let _: crate::progress::Event = serde_json::from_str(&json).unwrap();
        if event.phase == crate::progress::Phase::IngestionComplete {
            completed = event.parser_admission;
        }
    }
    let stats = completed.expect("completed provider admission statistics");
    assert_eq!(stats.active_sources, 0);
    assert_eq!(stats.waiting_sources, 0);
    assert_eq!(stats.estimated_reserved_bytes, 0);
    assert_eq!(stats.peak_active_sources, 1);
    assert!(stats.peak_reserved_bytes > 0);
    assert!(stats.peak_reserved_bytes <= stats.budget_bytes);
}

#[tokio::test]
#[ignore = "requires explicitly installed MASH_TEST_MODELS_DIR"]
async fn core_retrieves_native_codex_completed_and_mixed_dialogue() {
    use crate::core::{IndexRequest, Operation, Paths, Request, Response};
    use crate::search_results::{Results, SearchUnit};
    for fixture in [
        include_str!("../../../fixtures/codex-native-lifecycle-only-0.159.2.jsonl"),
        include_str!("../../../fixtures/codex-native-migration-0.159.2.jsonl"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let history = directory.path().join("session.jsonl");
        fs::write(&history, fixture).unwrap();
        let config = directory.path().join("config.toml");
        fs::write(
            &config,
            toml::to_string(&toml::toml! {
                version=1
                providers={codex={roots=[(history.to_str().unwrap())]}}
                index={all_history=true}
            })
            .unwrap(),
        )
        .unwrap();
        let paths = Paths {
            config: Some(config),
            db: Some(directory.path().join("index.sqlite3")),
            models_dir: Some(std::env::var_os("MASH_TEST_MODELS_DIR").unwrap().into()),
        };
        crate::core::execute(Request {
            paths: paths.clone(),
            operation: Operation::Index(IndexRequest::default()),
        })
        .await
        .unwrap();
        for unit in [SearchUnit::Messages, SearchUnit::Chats] {
            let Response::Search(response) = crate::core::execute(Request {
                paths: paths.clone(),
                operation: Operation::Search {
                    shared_vector_search: None,
                    fusion: crate::views::config::FusionOverrides::default(),
                    ranking: None,
                    diagnostics: false,
                    query: "cbioportal conversations".into(),
                    intent: None,
                    limit: Some(5),
                    unit: Some(unit),
                    filters: Box::new(crate::search_filter::Selection {
                        provider: vec![Provider::Codex],
                        project: vec!["/synthetic/mash".into()],
                        role: vec![crate::role::Role::User],
                        ..Default::default()
                    }),
                    shared: false,
                    local: true,
                },
            })
            .await
            .unwrap() else {
                panic!("search response")
            };
            let id = match response.results {
                Results::Messages(messages) => {
                    assert_eq!(messages.len(), 2);
                    assert_ne!(messages[0].id, messages[1].id);
                    messages[0].id.clone()
                }
                Results::Chats(threads) => {
                    assert_eq!(threads.len(), 1);
                    assert_eq!(
                        threads[0].source,
                        crate::source::Source::Provider(Provider::Codex)
                    );
                    assert_eq!(
                        threads[0].metadata.project.as_deref(),
                        Some("/synthetic/mash")
                    );
                    threads[0].matches[0].message_id.clone()
                }
            };
            let Response::View(view) = crate::core::execute(Request {
                paths: paths.clone(),
                operation: Operation::View {
                    id,
                    context: 4,
                    shared: None,
                },
            })
            .await
            .unwrap() else {
                panic!("view response")
            };
            assert_eq!(view.messages.len(), 4);
        }
    }
}

#[test]
fn oversized_jsonl_rolls_back_messages_and_checkpoint_then_recovers() {
    for provider in [Provider::ClaudeCode, Provider::Codex] {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("session.jsonl");
        let database = directory.path().join("index.sqlite3");
        let write = |text: &str| {
            let record = match provider {
                Provider::ClaudeCode => serde_json::json!({
                    "type": "user", "sessionId": "session", "uuid": "message",
                    "message": { "role": "user", "content": text }
                }),
                Provider::Codex => serde_json::json!({
                    "type": "response_item",
                    "payload": { "type": "message", "role": "user", "id": "message",
                        "content": [{ "type": "input_text", "text": text }] }
                }),
                _ => unreachable!(),
            };
            fs::write(&source, format!("{record}\n")).unwrap();
        };
        write("durable sentinel");
        let options = IndexOptions {
            providers: [(provider, vec![source.clone()])].into(),
            roots_are_authoritative: true,
            since_days: None,
        };
        let mut storage = Storage::open_writer(&database).unwrap();
        index(&mut storage, &options).unwrap();
        storage.commit_writer().unwrap();
        drop(storage);
        let stamp = source_stamp(&source).unwrap();
        write("uncommitted sentinel");
        let mut file = fs::OpenOptions::new().append(true).open(&source).unwrap();
        file.write_all(&vec![b'x'; MAX_RECORD_BYTES + 1]).unwrap();
        file.write_all(b"\n").unwrap();
        let mut storage = Storage::open_writer(&database).unwrap();
        let error = index(&mut storage, &options)
            .err()
            .expect("oversized record");
        assert!(
            error.error.message.contains("8 MiB ingestion limit"),
            "{error:?}"
        );
        storage.commit_writer().unwrap();
        assert_eq!(storage.search("durable", 10, None, None).unwrap().len(), 1);
        assert!(
            storage
                .search("uncommitted", 10, None, None)
                .unwrap()
                .is_empty()
        );
        assert!(
            storage
                .source_checkpoint_matches(provider, &SourceIdentity::file(&source), &stamp)
                .unwrap()
        );

        drop(storage);
        write("recovered sentinel");
        let mut storage = Storage::open_writer(&database).unwrap();
        assert_eq!(index(&mut storage, &options).unwrap().changed_messages, 1);
        storage.commit_writer().unwrap();
        assert_eq!(
            storage.search("recovered", 10, None, None).unwrap().len(),
            1
        );
        assert!(
            storage
                .search("durable", 10, None, None)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn changing_history_rolls_back_content_and_checkpoints_then_retries() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path().join("claude");
    fs::create_dir(&root).unwrap();
    let source = root.join("session.jsonl");
    let database = directory.path().join("mash.sqlite3");
    write_claude_session(&source, "session", "durable sentinel");
    let options = IndexOptions {
        providers: [(Provider::ClaudeCode, vec![root.clone()])].into(),
        roots_are_authoritative: true,
        since_days: None,
    };
    let mut writer = Storage::open_writer(&database).unwrap();
    index(&mut writer, &options).unwrap();
    writer.commit_writer().unwrap();
    drop(writer);
    let old_stamp = source_stamp(&source).unwrap();

    write_claude_session(&source, "session", "uncommitted changed sentinel");
    let discovery =
        discover_history_files(Provider::ClaudeCode, std::slice::from_ref(&root), true).unwrap();
    let mut writer = Storage::open_writer(&database).unwrap();
    let error = index_provider(
        &mut writer,
        ProviderScan {
            provider: Provider::ClaudeCode,
            roots: std::slice::from_ref(&root),
            cutoff_modified_ns: None,
            parse: Parser::Custom(parse_then_append),
            authoritative: true,
        },
        discovery,
        &mut empty_summary(),
        &mut IndexProgress::new(),
    )
    .expect_err("mutation must reject the scan");
    assert_eq!(error.error.kind, crate::ErrorKind::SourceChanged);
    assert!(error.error.retryable);
    assert!(matches!(
        error.error.recommended_action,
        Some(crate::Action::Index)
    ));
    assert!(error.error.message.contains("history changed"));
    writer.commit_writer().unwrap();
    assert_eq!(writer.search("durable", 10, None, None).unwrap().len(), 1);
    assert!(
        writer
            .search("uncommitted", 10, None, None)
            .unwrap()
            .is_empty()
    );
    assert!(
        writer
            .source_checkpoint_matches(
                Provider::ClaudeCode,
                &SourceIdentity::file(&source),
                &crate::silver::SourceStamp::file(old_stamp.size_bytes, old_stamp.modified_ns)
            )
            .unwrap()
    );
    drop(writer);

    let mut writer = Storage::open_writer(&database).unwrap();
    let retry = index(&mut writer, &options).expect("retry reads the complete new source");
    assert_eq!(retry.checkpoint_skipped_sources, 0);
    assert_eq!(retry.changed_messages, 1);
    writer.commit_writer().unwrap();
    assert_eq!(
        writer.search("uncommitted", 10, None, None).unwrap().len(),
        1
    );
    assert!(writer.search("durable", 10, None, None).unwrap().is_empty());
}

#[test]
fn queued_jsonl_prefix_applies_once_and_later_appends_remain_pending() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    write_claude_session(&path, "session", "original sentinel");
    let history = HistorySource::file(&path);
    let mut stamp = history.stamp().unwrap();
    let parsed = parse_stable_source(&history, &mut stamp, Provider::ClaudeCode.parser()).unwrap();
    let original_id = parsed.conversation.as_ref().unwrap().messages.inline()[0]
        .id
        .clone();
    std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(b"{\"type\":\"user\",\"sessionId\":\"session\",\"uuid\":\"second\",\"message\":{\"role\":\"user\",\"content\":\"later sentinel\"}}\n").unwrap();
    let mut writer = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    assert!(
        apply_parsed_source(
            &mut writer,
            Provider::ClaudeCode,
            ParsedSource {
                source: SourceFile {
                    history: history.clone(),
                    stamp: stamp.clone(),
                    estimated_bytes: u64::try_from(stamp.size_bytes).unwrap()
                },
                parsed: Ok(parsed),
                _reservation: None,
            },
            &mut empty_summary()
        )
        .unwrap()
    );
    writer.commit_writer().unwrap();
    assert_eq!(writer.counts().unwrap().messages, 1);
    assert_eq!(
        writer.search("original", 10, None, None).unwrap()[0].id,
        original_id
    );
    assert!(writer.search("later", 10, None, None).unwrap().is_empty());
    assert!(
        !writer
            .source_checkpoint_matches(
                Provider::ClaudeCode,
                &history.identity,
                &history.stamp().unwrap()
            )
            .unwrap()
    );
    let options = IndexOptions {
        providers: [(Provider::ClaudeCode, vec![directory.path().to_path_buf()])].into(),
        roots_are_authoritative: true,
        since_days: None,
    };
    drop(writer);
    let mut writer = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    index(&mut writer, &options).unwrap();
    writer.commit_writer().unwrap();
    assert_eq!(writer.counts().unwrap().messages, 2);
    assert_eq!(
        writer.search("original", 10, None, None).unwrap()[0].id,
        original_id
    );
    assert_eq!(writer.search("later", 10, None, None).unwrap().len(), 1);
    assert!(
        writer
            .source_checkpoint_matches(
                Provider::ClaudeCode,
                &history.identity,
                &history.stamp().unwrap()
            )
            .unwrap()
    );
}

#[test]
fn queued_results_refresh_changed_sources_and_reject_removed_sources_before_writing() {
    for remove in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("session.jsonl");
        write_claude_session(&source, "session", "queued sentinel");
        let mut stamp = source_stamp(&source).unwrap();
        let parsed = parse_stable_source(
            &HistorySource::file(&source),
            &mut stamp,
            Provider::ClaudeCode.parser(),
        )
        .unwrap();
        if remove {
            fs::remove_file(&source).unwrap();
        } else {
            write_claude_session(&source, "session", "replacement has different length");
        }
        let mut writer = Storage::open_writer(&directory.path().join("mash.sqlite3")).unwrap();
        let applied = apply_parsed_source(
            &mut writer,
            Provider::ClaudeCode,
            ParsedSource {
                source: SourceFile {
                    history: HistorySource::file(&source),
                    stamp: stamp.clone(),
                    estimated_bytes: u64::try_from(stamp.size_bytes).unwrap(),
                },
                parsed: Ok(parsed),
                _reservation: None,
            },
            &mut empty_summary(),
        );
        if remove {
            let error = applied.expect_err("removed source must not be applied");
            assert_eq!(error.error.kind, crate::ErrorKind::SourceChanged);
            assert!(error.error.retryable);
        } else {
            assert!(applied.unwrap());
        }
        writer.commit_writer().unwrap();
        assert_eq!(writer.counts().unwrap().messages, u64::from(!remove));
        assert!(
            writer
                .search("sentinel", 10, None, None)
                .unwrap()
                .is_empty()
        );
        if !remove {
            assert_eq!(
                writer.search("replacement", 10, None, None).unwrap().len(),
                1
            );
            assert!(
                writer
                    .source_checkpoint_matches(
                        Provider::ClaudeCode,
                        &SourceIdentity::file(&source),
                        &source_stamp(&source).unwrap()
                    )
                    .unwrap()
            );
        }
        assert!(
            !writer
                .source_checkpoint_matches(
                    Provider::ClaudeCode,
                    &SourceIdentity::file(&source),
                    &crate::silver::SourceStamp::file(stamp.size_bytes, stamp.modified_ns)
                )
                .unwrap()
        );
    }
}

#[test]
fn a_changed_discovered_stamp_is_rejected_before_parsing() {
    fn must_not_parse(_: &HistorySource) -> Result<ParsedFile, AppError> {
        panic!("changed source must be rejected before the parser runs")
    }
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("session.jsonl");
    write_claude_session(&source, "session", "initial");
    let mut stamp = source_stamp(&source).unwrap();
    write_claude_session(&source, "session", "changed after discovery");
    let error = parse_stable_source(
        &HistorySource::file(&source),
        &mut stamp,
        Parser::Custom(must_not_parse),
    )
    .err()
    .expect("discovery stamp is stale");
    assert!(error.error.message.contains("history changed"));
}

#[test]
fn stable_retry_refreshes_discovery_and_recovers_a_single_append() {
    fn append_once(source: &HistorySource) -> Result<ParsedFile, AppError> {
        let marker = source.identity.path.with_extension("appended");
        if !marker.exists() {
            fs::write(marker, b"done").map_err(AppError::io)?;
            return parse_then_append(source);
        }
        parse_claude(&source.identity.path)
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    write_claude_session(&path, "session", "old content");
    let mut stamp = source_stamp(&path).unwrap();
    write_claude_session(&path, "session", "current content after discovery");
    let source = HistorySource::file(&path);
    let parsed =
        parse_source_with_retries(&source, &mut stamp, Parser::Custom(append_once)).unwrap();
    assert_eq!(
        parsed.conversation.unwrap().messages.inline()[0].content,
        "current content after discovery"
    );
    stamp.verify(&source.identity).unwrap();
    assert_eq!(stamp, source.stamp().unwrap());
}

#[test]
fn unstable_source_retries_are_bounded_and_actionable() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    write_claude_session(&path, "session", "content");
    let initial_size = fs::metadata(&path).unwrap().len();
    let source = HistorySource::file(&path);
    let mut stamp = source.stamp().unwrap();
    let error = parse_source_with_retries(&source, &mut stamp, Parser::Custom(parse_then_append))
        .err()
        .unwrap();
    assert_eq!(fs::metadata(&path).unwrap().len(), initial_size + 3);
    assert_eq!(error.error.kind, crate::ErrorKind::SourceChanged);
    assert!(error.error.retryable);
    assert!(matches!(
        error.error.recommended_action,
        Some(crate::Action::Index)
    ));
}

#[test]
fn stable_retry_reads_atomic_replacement_instead_of_retired_content() {
    fn replace_once(source: &HistorySource) -> Result<ParsedFile, AppError> {
        let path = &source.identity.path;
        let marker = path.with_extension("replaced");
        let parsed = parse_claude(path)?;
        if !marker.exists() {
            let replacement = path.with_extension("new");
            write_claude_session(&replacement, "session", "replacement dialogue");
            fs::rename(replacement, path).map_err(AppError::io)?;
            fs::write(marker, b"done").map_err(AppError::io)?;
        }
        Ok(parsed)
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    write_claude_session(&path, "session", "retired content");
    let source = HistorySource::file(&path);
    let mut stamp = source.stamp().unwrap();
    let parsed =
        parse_source_with_retries(&source, &mut stamp, Parser::Custom(replace_once)).unwrap();
    assert_eq!(
        parsed.conversation.unwrap().messages.inline()[0].content,
        "replacement dialogue"
    );
    stamp.verify(&source.identity).unwrap();
}

#[test]
fn missing_source_exhausts_retries_without_parsing_or_claiming_success() {
    fn must_not_parse(_: &HistorySource) -> Result<ParsedFile, AppError> {
        panic!("missing source must not reach the parser")
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    write_claude_session(&path, "session", "content");
    let source = HistorySource::file(&path);
    let mut stamp = source.stamp().unwrap();
    fs::remove_file(path).unwrap();
    let error = parse_source_with_retries(&source, &mut stamp, Parser::Custom(must_not_parse))
        .err()
        .unwrap();
    assert_eq!(error.error.kind, crate::ErrorKind::SourceChanged);
    assert!(error.error.retryable);
    assert!(matches!(
        error.error.recommended_action,
        Some(crate::Action::Index)
    ));
}

#[test]
fn retries_distinguish_mutation_during_parse_errors_from_stable_bad_input() {
    fn interrupted_once(source: &HistorySource) -> Result<ParsedFile, AppError> {
        let marker = source.identity.path.with_extension("interrupted");
        if !marker.exists() {
            fs::write(marker, b"done").map_err(AppError::io)?;
            parse_then_append(source)?;
            return Err(AppError::configuration("transient partial record"));
        }
        parse_claude(&source.identity.path)
    }
    fn invalid(source: &HistorySource) -> Result<ParsedFile, AppError> {
        let marker = source.identity.path.with_extension("invalid");
        assert!(!marker.exists(), "stable invalid input must not be retried");
        fs::write(marker, b"done").map_err(AppError::io)?;
        Err(AppError::configuration("invalid history"))
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.jsonl");
    write_claude_session(&path, "session", "content");
    let source = HistorySource::file(&path);
    let mut stamp = source.stamp().unwrap();
    parse_source_with_retries(&source, &mut stamp, Parser::Custom(interrupted_once)).unwrap();
    let error = parse_source_with_retries(&source, &mut stamp, Parser::Custom(invalid))
        .err()
        .unwrap();
    assert_eq!(error.error.kind, crate::ErrorKind::Configuration);
    assert!(!error.error.retryable);
}

#[test]
fn purge_requires_a_complete_provider_scan() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database = directory.path().join("mash.sqlite3");
    let claude_root = directory.path().join("claude");
    fs::create_dir_all(&claude_root).expect("Claude root");
    let missing_source = claude_root.join("missing.jsonl");

    let mut writer = Storage::open_writer(&database).expect("writer");
    writer
        .replace_conversation(&Conversation {
            id: "old-session".to_owned(),
            provider: Provider::ClaudeCode,
            title_origin: None,
            timestamp_origins: None,
            claude_lineage: None,
            source: SourceIdentity::file(missing_source),
            project: None,
            repository: None,
            title: None,
            created_at: None,
            updated_at: None,
            messages: vec![NormalizedMessage {
                attachments: Vec::new(),
                id: "old-message".to_owned(),
                ordinal: 0,
                context: crate::message_context::MessageContext::Linear,
                role: crate::role::Role::User,
                content: "preserve until complete".to_owned(),
                dialogue_text: None,
                created_at: None,
            }]
            .into(),
        })
        .expect("seed source");
    writer.commit_writer().expect("commit seed");
    drop(writer);

    fs::write(claude_root.join("malformed.jsonl"), "not-json\n").expect("malformed source");
    let options = IndexOptions {
        providers: [(Provider::ClaudeCode, vec![claude_root.clone()])].into(),
        roots_are_authoritative: true,
        since_days: None,
    };
    let mut writer = Storage::open_writer(&database).expect("incomplete writer");
    let incomplete = index(&mut writer, &options).expect("bounded malformed scan");
    assert_eq!(incomplete.malformed_records, 1);
    writer.commit_writer().expect("commit incomplete scan");
    assert_eq!(writer.counts().expect("counts").conversations, 1);
    drop(writer);

    fs::remove_file(claude_root.join("malformed.jsonl")).expect("remove malformed source");
    let mut writer = Storage::open_writer(&database).expect("complete writer");
    let complete = index(&mut writer, &options).expect("complete scan");
    assert_eq!(complete.purged_conversations, 1);
    writer.commit_writer().expect("commit complete scan");
    assert_eq!(writer.counts().expect("counts").conversations, 0);
}

#[test]
fn authoritative_roots_index_only_the_two_concrete_formats() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let claude = directory.path().join("claude");
    let codex = directory.path().join("codex");
    fs::create_dir(&claude).expect("Claude root");
    fs::create_dir(&codex).expect("Codex root");
    write_claude_session(
        &claude.join("claude.jsonl"),
        "claude-session",
        "claude sentinel",
    );
    fs::write(
        claude.join("unsupported.jsonl"),
        "{\"info\":{\"id\":\"opencode-session\"},\"messages\":[{\"role\":\"user\",\"content\":\"unsupported sentinel\"}]}\n",
    )
    .expect("unsupported fixture");
    fs::write(
        codex.join("codex.jsonl"),
        concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"codex-session\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"id\":\"codex-message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"codex sentinel\"}]}}\n"
        ),
    )
    .expect("Codex fixture");
    let mut writer = Storage::open_writer(&directory.path().join("mash.sqlite3")).expect("writer");
    let summary = index(
        &mut writer,
        &IndexOptions {
            providers: [
                (Provider::ClaudeCode, vec![claude]),
                (Provider::Codex, vec![codex]),
            ]
            .into(),
            roots_are_authoritative: true,
            since_days: None,
        },
    )
    .expect("configured index");

    assert_eq!(summary.scanned_files, 3);
    assert_eq!(writer.counts().expect("counts").conversations, 2);
    writer.commit_writer().expect("commit configured index");
    assert_eq!(
        writer
            .search(
                "claude",
                10,
                Some(crate::provider::Provider::ClaudeCode),
                None
            )
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        writer
            .search("codex", 10, Some(crate::provider::Provider::Codex), None)
            .unwrap()
            .len(),
        1
    );
    assert!(
        writer
            .search("unsupported", 10, None, None)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn every_authoritative_root_is_preflighted_before_ingestion_writes() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let claude_root = directory.path().join("claude");
    fs::create_dir(&claude_root).expect("Claude root");
    write_claude_session(&claude_root.join("session.jsonl"), "session", "sentinel");
    let options = IndexOptions {
        providers: [
            (Provider::ClaudeCode, vec![claude_root]),
            (
                Provider::Codex,
                vec![directory.path().join("missing-codex")],
            ),
        ]
        .into(),
        roots_are_authoritative: true,
        since_days: None,
    };
    let mut writer = Storage::open_writer(&directory.path().join("mash.sqlite3")).expect("writer");

    let error = index(&mut writer, &options).err().expect("missing root");

    assert_eq!(error.error.kind, crate::ErrorKind::Configuration);
    assert_eq!(writer.counts().expect("counts").conversations, 0);
    assert_eq!(writer.counts().expect("counts").messages, 0);
}

#[test]
fn missing_builtin_root_is_nonfatal_but_never_authorizes_purge() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path().join("claude");
    fs::create_dir(&root).expect("Claude root");
    write_claude_session(&root.join("session.jsonl"), "session", "durable sentinel");
    let database = directory.path().join("mash.sqlite3");
    let options = IndexOptions {
        providers: [(Provider::ClaudeCode, vec![root.clone()])].into(),
        roots_are_authoritative: false,
        since_days: None,
    };
    let mut writer = Storage::open_writer(&database).expect("initial writer");
    index(&mut writer, &options).expect("initial index");
    writer.commit_writer().expect("commit initial index");
    drop(writer);
    fs::rename(&root, directory.path().join("offline")).expect("take root offline");

    let mut writer = Storage::open_writer(&database).expect("offline writer");
    let refresh = index(&mut writer, &options).expect("nonfatal refresh");

    assert_eq!(refresh.purged_conversations, 0);
    assert_eq!(writer.counts().expect("counts").conversations, 1);
    assert_eq!(
        writer
            .search("durable", 10, None, None)
            .expect("preserved search row")
            .len(),
        1
    );
    drop(writer);

    fs::write(&root, "not a JSONL history").expect("replace root with unusable file");
    let mut writer = Storage::open_writer(&database).expect("wrong-type writer");
    let refresh = index(&mut writer, &options).expect("nonfatal wrong-type refresh");
    assert_eq!(refresh.purged_conversations, 0);
    assert_eq!(writer.counts().expect("counts").conversations, 1);
}

#[test]
fn configured_root_disappearing_after_discovery_rolls_back_without_purge() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path().join("configured");
    fs::create_dir(&root).expect("configured root");
    let discovery = discover_history_files(Provider::ClaudeCode, std::slice::from_ref(&root), true)
        .expect("initial empty discovery");
    let database = directory.path().join("mash.sqlite3");
    let mut writer = Storage::open_writer(&database).expect("seed writer");
    writer
        .replace_conversation(&test_conversation(
            "existing",
            Provider::ClaudeCode,
            root.join("missing.jsonl"),
            "durable sentinel",
        ))
        .expect("seed conversation");
    writer.commit_writer().expect("commit seed");
    drop(writer);
    fs::remove_dir(&root).expect("remove configured root");

    let mut writer = Storage::open_writer(&database).expect("configured writer");
    let error = index_provider(
        &mut writer,
        ProviderScan {
            provider: Provider::ClaudeCode,
            roots: std::slice::from_ref(&root),
            cutoff_modified_ns: None,
            parse: Provider::ClaudeCode.parser(),
            authoritative: true,
        },
        discovery,
        &mut empty_summary(),
        &mut IndexProgress::new(),
    )
    .expect_err("disappearing configured root");

    assert_eq!(error.error.kind, crate::ErrorKind::Configuration);
    assert_eq!(writer.counts().expect("counts").conversations, 1);
    writer.commit_writer().expect("commit unchanged state");
    assert_eq!(writer.search("durable", 10, None, None).unwrap().len(), 1);
}

#[test]
fn late_configured_source_failure_rolls_back_provider_changes() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database = directory.path().join("mash.sqlite3");
    let old_source = directory.path().join("old.jsonl");
    let mut writer = Storage::open_writer(&database).expect("seed writer");
    for (id, content) in [("existing", "durable sentinel"), ("forgotten", "forgotten")] {
        writer
            .replace_conversation(&test_conversation(
                id,
                Provider::ClaudeCode,
                if id == "existing" {
                    old_source.clone()
                } else {
                    directory.path().join("forgotten.jsonl")
                },
                content,
            ))
            .expect("seed conversation");
    }
    writer
        .record_source_checkpoint(
            crate::provider::Provider::ClaudeCode,
            &SourceIdentity::file(&old_source),
            &crate::silver::SourceStamp::file(10, 20),
        )
        .expect("seed checkpoint");
    writer
        .replace_embeddings(
            "test-generation",
            &[crate::storage::EmbeddingWrite {
                message_id: "existing-message",
                vector: &[127],
                norm: 127.0,
            }],
        )
        .expect("seed embedding");
    writer.commit_writer().expect("commit seed");
    drop(writer);
    let mut storage = Storage::open(&database).expect("forget storage");
    assert!(storage.forget("forgotten").expect("seed tombstone"));
    drop(storage);

    let root = directory.path().join("configured");
    fs::create_dir(&root).expect("configured root");
    let valid = root.join("valid.jsonl");
    let failure = root.join("failure.jsonl");
    write_claude_session(&valid, "new-session", "uncommitted sentinel");
    fs::write(&failure, "placeholder\n").expect("failure fixture");
    let discovery = discover_history_files(Provider::ClaudeCode, std::slice::from_ref(&root), true)
        .expect("initial configured discovery");
    let mut writer = Storage::open_writer(&database).expect("configured writer");
    let mut summary = empty_summary();
    let error = index_provider(
        &mut writer,
        ProviderScan {
            provider: Provider::ClaudeCode,
            roots: std::slice::from_ref(&root),
            cutoff_modified_ns: None,
            parse: Parser::Custom(parse_with_late_failure),
            authoritative: true,
        },
        discovery,
        &mut summary,
        &mut IndexProgress::new(),
    )
    .expect_err("late source failure");

    assert_eq!(error.error.kind, crate::ErrorKind::Configuration);
    assert_eq!(summary.committed_batches, 0);
    assert_eq!(writer.counts().expect("counts").conversations, 1);
    assert_eq!(writer.embedding_count("test-generation").unwrap(), 1);
    assert!(
        writer
            .source_checkpoint_matches(
                crate::provider::Provider::ClaudeCode,
                &SourceIdentity::file(&old_source),
                &crate::silver::SourceStamp::file(10, 20)
            )
            .unwrap()
    );
    assert_eq!(writer.search("durable", 10, None, None).unwrap().len(), 1);
    assert!(
        writer
            .search("uncommitted", 10, None, None)
            .unwrap()
            .is_empty()
    );
    let tombstone = writer
        .replace_conversation(&test_conversation(
            "forgotten",
            Provider::ClaudeCode,
            directory.path().join("forgotten.jsonl"),
            "forgotten",
        ))
        .expect("tombstone remains");
    assert!(tombstone.tombstoned);
}

#[test]
fn horizon_admits_the_boundary_and_preserves_deleted_old_sources() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path().join("claude");
    fs::create_dir(&root).expect("Claude root");
    let old_path = root.join("old.jsonl");
    let recent_path = root.join("recent.jsonl");
    write_claude_session(&old_path, "old-session", "old sentinel");
    fs::write(
        &recent_path,
        concat!(
            "{\"type\":\"user\",\"sessionId\":\"recent-session\",\"uuid\":\"recent-1\",\"timestamp\":\"2020-01-01T00:00:00Z\",\"message\":{\"role\":\"user\",\"content\":\"recent sentinel\"}}\n",
            "{\"type\":\"assistant\",\"sessionId\":\"recent-session\",\"uuid\":\"recent-2\",\"timestamp\":\"2020-01-02T00:00:00Z\",\"message\":{\"role\":\"assistant\",\"content\":\"complete conversation sentinel\"}}\n"
        ),
    )
    .expect("boundary fixture");
    let run_started = UNIX_EPOCH + Duration::from_hours(200 * 24);
    let boundary = run_started - Duration::from_hours(90 * 24);
    set_modified(&old_path, boundary - Duration::from_nanos(1));
    set_modified(&recent_path, boundary);
    let database = directory.path().join("mash.sqlite3");
    let bounded = IndexOptions {
        providers: [(Provider::ClaudeCode, vec![root.clone()])].into(),
        roots_are_authoritative: true,
        since_days: Some(90),
    };
    let mut writer = Storage::open_writer(&database).expect("bounded writer");
    let admitted = index_at(&mut writer, &bounded, run_started).expect("bounded index");
    assert_eq!(admitted.scanned_files, 1);
    assert_eq!(writer.counts().expect("bounded counts").conversations, 1);
    assert_eq!(writer.counts().expect("bounded counts").messages, 2);
    writer.commit_writer().expect("commit bounded index");
    assert_eq!(writer.search("complete", 10, None, None).unwrap().len(), 1);
    assert!(writer.search("old", 10, None, None).unwrap().is_empty());
    drop(writer);

    let all_history = IndexOptions {
        since_days: None,
        ..bounded
    };
    let mut writer = Storage::open_writer(&database).expect("all-history writer");
    let archival = index_at(&mut writer, &all_history, run_started).expect("all-history index");
    assert_eq!(archival.scanned_files, 2);
    assert_eq!(
        writer.counts().expect("all-history counts").conversations,
        2
    );
    let old_message_id = stable_id("claude-message", &["old-session", "old-session-m1"]);
    writer
        .replace_embeddings(
            "old-generation",
            &[crate::storage::EmbeddingWrite {
                message_id: &old_message_id,
                vector: &[127],
                norm: 127.0,
            }],
        )
        .expect("old embedding");
    let old_stamp = source_stamp(&old_path).expect("old source stamp");
    writer.commit_writer().expect("commit all-history index");
    drop(writer);

    fs::remove_file(&old_path).expect("remove old source");
    fs::remove_file(&recent_path).expect("remove recent source");
    let bounded = IndexOptions {
        since_days: Some(90),
        ..all_history
    };
    let mut writer = Storage::open_writer(&database).expect("bounded writer");
    let refresh = index_at(&mut writer, &bounded, run_started).expect("bounded refresh");

    assert_eq!(refresh.purged_conversations, 1);
    assert_eq!(writer.counts().expect("counts").conversations, 1);
    assert_eq!(writer.embedding_count("old-generation").unwrap(), 1);
    assert!(
        writer
            .source_checkpoint_matches(
                crate::provider::Provider::ClaudeCode,
                &SourceIdentity::file(&old_path),
                &crate::silver::SourceStamp::file(old_stamp.size_bytes, old_stamp.modified_ns)
            )
            .expect("old checkpoint")
    );
    writer.commit_writer().expect("commit bounded refresh");
    assert_eq!(writer.search("old", 10, None, None).unwrap().len(), 1);
    assert!(writer.search("recent", 10, None, None).unwrap().is_empty());
}

#[test]
fn selecting_one_provider_never_reconciles_an_unselected_provider() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path().join("codex");
    fs::create_dir(&root).expect("Codex root");
    let database = directory.path().join("mash.sqlite3");
    let mut writer = Storage::open_writer(&database).expect("writer");
    for (id, provider, source_path) in [
        (
            "claude-session",
            Provider::ClaudeCode,
            directory.path().join("claude.jsonl"),
        ),
        ("codex-session", Provider::Codex, root.join("missing.jsonl")),
        (
            "claude-forgotten",
            Provider::ClaudeCode,
            directory.path().join("claude-forgotten.jsonl"),
        ),
    ] {
        writer
            .replace_conversation(&test_conversation(
                id,
                provider,
                source_path,
                &format!("{provider} sentinel"),
            ))
            .expect("seed conversation");
    }
    writer
        .record_source_checkpoint(
            crate::provider::Provider::ClaudeCode,
            &SourceIdentity::file(directory.path().join("claude.jsonl")),
            &crate::silver::SourceStamp::file(10, 20),
        )
        .expect("Claude checkpoint");
    writer
        .replace_embeddings(
            "claude-generation",
            &[crate::storage::EmbeddingWrite {
                message_id: "claude-session-message",
                vector: &[127],
                norm: 127.0,
            }],
        )
        .expect("Claude embedding");
    writer.commit_writer().expect("commit seed");
    drop(writer);
    let mut storage = Storage::open(&database).expect("tombstone storage");
    assert!(
        storage
            .forget("claude-forgotten")
            .expect("Claude tombstone")
    );
    drop(storage);

    let mut writer = Storage::open_writer(&database).expect("refresh writer");
    let summary = index(
        &mut writer,
        &IndexOptions {
            providers: [(Provider::Codex, vec![root])].into(),
            roots_are_authoritative: true,
            since_days: None,
        },
    )
    .expect("Codex-only refresh");

    assert_eq!(summary.purged_conversations, 1);
    assert_eq!(writer.counts().expect("counts").conversations, 1);
    assert_eq!(writer.embedding_count("claude-generation").unwrap(), 1);
    assert!(
        writer
            .source_checkpoint_matches(
                crate::provider::Provider::ClaudeCode,
                &SourceIdentity::file(directory.path().join("claude.jsonl")),
                &crate::silver::SourceStamp::file(10, 20)
            )
            .unwrap()
    );
    assert_eq!(
        writer
            .search("claude", 10, None, None)
            .expect("Claude remains")
            .len(),
        1
    );
    let tombstone = writer
        .replace_conversation(&test_conversation(
            "claude-forgotten",
            Provider::ClaudeCode,
            directory.path().join("claude-forgotten.jsonl"),
            "forgotten",
        ))
        .expect("Claude tombstone remains");
    assert!(tombstone.tombstoned);
}
