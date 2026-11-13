use super::*;
use crate::{
    silver::{IndexOptions, index, source_stamp_for},
    storage::Storage,
};
use serde_json::{Value, json};
use std::io::Write;

mod identities;
mod staging;

fn record(id: &str, kind: &str, data: Value) -> Value {
    let mut result =
        json!({"id":id,"parentId":"previous-event","timestamp":"2026-09-30T10:00:01Z","type":kind});
    result["data"] = data;
    result
}

fn header(session: &str) -> Value {
    let mut value = record(
        "start",
        "session.start",
        json!({
            "sessionId":session,"version":1,"producer":"copilot-agent","copilotVersion":"1.0.89",
            "startTime":"2026-09-30T10:00:00Z",
            "context":{"cwd":"/work/cbioportal","gitRoot":"/work/cbioportal","repository":"team/cbioportal"}
        }),
    );
    value["parentId"] = Value::Null;
    value
}

fn fixture() -> Vec<Value> {
    vec![
        header("copilot-session"),
        record(
            "request",
            "user.message",
            json!({"content":"Fix the cBioPortal importer","source":"user","attachments":[],"transformedContent":"privatetransformedmarker"}),
        ),
        record(
            "delta",
            "assistant.message_delta",
            json!({"messageId":"answer","deltaContent":"deltaduplicatemarker"}),
        ),
        record(
            "answer",
            "assistant.message",
            json!({"messageId":"answer","content":"Validate mutation metadata before importing.","reasoningText":"privatereasoningmarker","toolRequests":[{"toolCallId":"call","name":"read_file","arguments":{"path":"privatetoolmarker"}}]}),
        ),
        record(
            "execution",
            "tool.execution_start",
            json!({"toolCallId":"call","toolName":"read_file","arguments":{"path":"privatetoolmarker"}}),
        ),
        record(
            "result",
            "tool.execution_complete",
            json!({"success":true,"toolCallId":"call","result":{"content":"shortresultmarker","detailedContent":"privateoutputmarker"}}),
        ),
        record(
            "injected",
            "user.message",
            json!({"content":"privateskillmarker","source":"skill-pdf"}),
        ),
        record(
            "automatic",
            "user.message",
            json!({"content":"privateautopilotmarker","source":"user","isAutopilotContinuation":true}),
        ),
        {
            let mut value = record(
                "subrequest",
                "user.message",
                json!({"content":"privatesubagentrequest","source":"user"}),
            );
            value["agentId"] = "subagent-1".into();
            value
        },
        {
            let mut value = record(
                "subanswer",
                "assistant.message",
                json!({"messageId":"subanswer","content":"privatesubagentanswer"}),
            );
            value["agentId"] = "subagent-1".into();
            value
        },
        record(
            "legacy-sub",
            "assistant.message",
            json!({"messageId":"legacy-sub","content":"privatelegacysubagent","parentToolCallId":"task-1"}),
        ),
        record(
            "compact",
            "session.compaction_complete",
            json!({"success":true,"summaryContent":"privatecompactionmarker"}),
        ),
        record(
            "followup",
            "user.message",
            json!({"content":"Also check structural variants","source":"user"}),
        ),
    ]
}

fn write(path: &Path, records: &[Value]) {
    let mut file = File::create(path).unwrap();
    for record in records {
        writeln!(file, "{record}").unwrap();
    }
}

fn options(path: &Path) -> IndexOptions {
    IndexOptions {
        providers: [(Provider::Copilot, vec![path.to_path_buf()])].into(),
        roots_are_authoritative: true,
        since_days: None,
    }
}

#[test]
fn native_log_separates_prose_from_internal_context_and_agent_lanes() {
    for record in fixture() {
        if let Err(error) = serde_json::from_value::<Event>(record.clone()) {
            panic!("invalid typed fixture {record}: {error}");
        }
    }
    let file = tempfile::NamedTempFile::new().unwrap();
    write(file.path(), &fixture());
    let original = std::fs::read(file.path()).unwrap();
    let chat = parse(file.path()).unwrap().chat.unwrap();
    assert_eq!(chat.title.as_deref(), Some("Fix the cBioPortal importer"));
    assert_eq!(chat.project.as_deref(), Some("/work/cbioportal"));
    assert_eq!(chat.repository.as_deref(), Some("team/cbioportal"));
    assert_eq!(chat.messages.len(), 12);
    let canonical = chat
        .messages
        .inline()
        .iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let projection = chat
        .messages
        .inline()
        .iter()
        .map(|m| m.dialogue_text.as_deref().unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!canonical.contains("deltaduplicatemarker"));
    assert!(!canonical.contains("privatetransformedmarker"));
    assert!(!canonical.contains("shortresultmarker"));
    for marker in [
        "privatereasoningmarker",
        "privatetoolmarker",
        "privateoutputmarker",
        "privateskillmarker",
        "privateautopilotmarker",
        "privatesubagentrequest",
        "privatesubagentanswer",
        "privatelegacysubagent",
        "privatecompactionmarker",
    ] {
        assert!(canonical.contains(marker), "{marker}");
        assert!(!projection.contains(marker), "{marker}");
    }
    assert!(projection.contains("cBioPortal"));
    assert!(projection.contains("Validate mutation metadata"));
    assert_eq!(chat.messages.inline()[7].context, MessageContext::Root);
    assert_eq!(
        chat.messages.inline()[8].context,
        MessageContext::Parent {
            id: chat.messages.inline()[7].id.clone()
        }
    );
    assert_eq!(chat.messages.inline()[9].context, MessageContext::Root);
    assert_eq!(
        chat.messages.inline()[10].context,
        MessageContext::Parent {
            id: chat.messages.inline()[6].id.clone()
        }
    );
    assert_eq!(std::fs::read(file.path()).unwrap(), original);
    let repeated = parse(file.path()).unwrap().chat.unwrap();
    assert_eq!(
        chat.messages
            .inline()
            .iter()
            .map(|m| &m.id)
            .collect::<Vec<_>>(),
        repeated
            .messages
            .inline()
            .iter()
            .map(|m| &m.id)
            .collect::<Vec<_>>()
    );
    let mut fork = fixture();
    fork[0] = header("forked-session");
    write(file.path(), &fork);
    let forked = parse(file.path()).unwrap().chat.unwrap();
    assert_ne!(chat.id, forked.id);
    assert_ne!(chat.messages.inline()[0].id, forked.messages.inline()[0].id);
}

#[test]
fn assistant_events_split_reasoning_replies_and_tool_calls() {
    let file = tempfile::NamedTempFile::new().unwrap();
    write(file.path(), &fixture());
    let chat = parse(file.path()).unwrap().chat.unwrap();
    // Reasoning, the reply and the tool request are separate messages, and
    // the tool call and its output carry the same call identity.
    let inline = chat.messages.inline();
    assert_eq!(inline[1].role, Role::Assistant);
    assert_eq!(inline[1].content, "[Thinking] privatereasoningmarker");
    assert!(inline[1].tools.is_empty());
    assert_eq!(
        inline[2].content,
        "Validate mutation metadata before importing."
    );
    assert!(inline[2].tools.is_empty());
    assert_eq!(inline[3].role, Role::Assistant);
    assert_eq!(inline[3].tools.tool_calls.len(), 1);
    assert_eq!(
        inline[3].tools.tool_calls[0].name.as_deref(),
        Some("read_file")
    );
    assert_eq!(
        inline[3].tools.tool_calls[0].call_id.as_deref(),
        Some("call")
    );
    assert_eq!(inline[4].role, Role::Tool);
    assert_eq!(
        inline[4].tools.tool_results[0].call_id.as_deref(),
        Some("call")
    );
    assert_eq!(inline[4].tools.tool_results[0].is_error, Some(false));
    // The execution start repeats the request, so it adds no message.
    assert_eq!(
        inline
            .iter()
            .filter(|m| m.content.contains("privatetoolmarker"))
            .count(),
        1
    );
}

#[test]
fn local_routing_and_views_exclude_subagents_and_reconcile_native_rewinds() {
    let dir = tempfile::tempdir().unwrap();
    let history = dir.path().join("events.jsonl");
    write(&history, &fixture());
    // Recursive discovery must not mistake other Copilot JSONL artifacts for history.
    std::fs::write(dir.path().join("requests.jsonl"), "not a session").unwrap();
    let sources = super::super::discovery::discover_history_files(
        Provider::Copilot,
        &[dir.path().into()],
        true,
    )
    .unwrap();
    assert_eq!(sources.sources.len(), 1);
    let database = dir.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(dir.path())).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 12);
    assert_eq!(storage.counts().unwrap().searchable_messages, 3);
    let chat = parse(&history).unwrap().chat.unwrap();
    let followup = crate::storage::test_support::projections_with_text(
        &storage,
        "structural",
        Some(Provider::Copilot),
    )
    .remove(0);
    let context = storage.view(&followup.id, 10).unwrap();
    assert!(context.iter().any(|m| m.content.contains("cBioPortal")));
    assert!(!context.iter().any(|m| m.content.contains("subagent")));
    for marker in [
        "privatetoolmarker",
        "privateskillmarker",
        "privateautopilotmarker",
    ] {
        assert!(
            crate::storage::test_support::projections_with_text(&storage, marker, None).is_empty()
        );
    }
    storage
        .with_publication_snapshot(|snapshot| {
            snapshot.visit(crate::shared::protocol::BatchKind::Messages, |batch| {
                let crate::shared::protocol::Batch::Messages { records } = batch else {
                    panic!("message batch")
                };
                assert_eq!(records.len(), 12);
                for record in records {
                    let native = chat
                        .messages
                        .inline()
                        .iter()
                        .find(|m| m.id == record.id)
                        .unwrap();
                    assert_eq!(record.context, native.context);
                    assert_eq!(record.chat_id, chat.id);
                }
                Ok(())
            })
        })
        .unwrap();
    drop(storage);
    write(&history, &fixture()[..4]);
    let mut storage = Storage::open_writer(&database).unwrap();
    let summary = index(&mut storage, &options(&history)).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(summary.removed_messages, 8);
    assert_eq!(storage.counts().unwrap().messages, 4);
    assert!(
        crate::storage::test_support::projections_with_text(&storage, "structural", None)
            .is_empty()
    );
}

#[test]
fn malformed_refresh_preserves_history_and_checkpoint_until_retry() {
    let dir = tempfile::tempdir().unwrap();
    let history = dir.path().join("events.jsonl");
    write(&history, &fixture());
    let database = dir.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&history)).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let source = SourceIdentity::file(&history);
    let stamp = source_stamp_for(&source).unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&history)
        .unwrap()
        .write_all(b"{\"type\":")
        .unwrap();
    let mut storage = Storage::open_writer(&database).unwrap();
    let partial = index(&mut storage, &options(&history)).unwrap();
    assert_eq!(
        (partial.rejected_sources, partial.malformed_records),
        (1, 1)
    );
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 12);
    assert!(
        storage
            .source_checkpoint_matches(Provider::Copilot, &source, &stamp)
            .unwrap()
    );
    drop(storage);
    let mut valid = fixture();
    valid.push(record(
        "new",
        "assistant.message",
        json!({"messageId":"new","content":"Structural variant validation is complete."}),
    ));
    write(&history, &valid);
    let mut storage = Storage::open_writer(&database).unwrap();
    assert_eq!(
        index(&mut storage, &options(&history))
            .unwrap()
            .changed_messages,
        1
    );
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 13);
}

#[test]
fn clear_starts_a_new_context_chain_and_bad_native_envelopes_fail_closed() {
    let file = tempfile::NamedTempFile::new().unwrap();
    for invalid in [
        record("request", "user.message", json!({"content":"duplicate"})),
        record("bad", "user.message", json!({"content":42})),
        {
            let mut r = record("bad", "user.message", json!({"content":"bad time"}));
            r["timestamp"] = "bad".into();
            r
        },
        {
            let mut r = record("bad", "user.message", json!({"content":"self"}));
            r["parentId"] = "bad".into();
            r
        },
        header("second-header"),
    ] {
        let mut records = fixture();
        records.push(invalid);
        write(file.path(), &records);
        assert!(parse(file.path()).is_err());
    }
    for version in [0, 2] {
        let mut h = header("session");
        h["data"]["version"] = version.into();
        write(file.path(), &[h]);
        assert!(parse(file.path()).is_err());
    }
    write(file.path(), &fixture()[1..]);
    assert!(parse(file.path()).is_err());
    let mut valid = fixture();
    valid.push(record(
        "clear",
        "session.context_cleared",
        json!({"messagesCleared":10}),
    ));
    valid.push(record(
        "fresh",
        "user.message",
        json!({"content":"New independent question"}),
    ));
    write(file.path(), &valid);
    let parsed = parse(file.path()).unwrap().chat.unwrap();
    assert_eq!(
        parsed.messages.inline().last().unwrap().context,
        MessageContext::Root
    );
    assert_eq!(
        parsed.messages.inline()[0].dialogue_text.as_deref(),
        Some("Fix the cBioPortal importer")
    );
    write(file.path(), &[header("empty-session")]);
    assert!(
        parse(file.path())
            .unwrap()
            .chat
            .unwrap()
            .messages
            .is_empty()
    );
}

#[test]
fn settled_workspace_and_complete_chunks_ignore_preliminary_and_ephemeral_events() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let mut h = header("settled");
    h["data"]["context"]["pendingGitContext"] = true.into();
    let mut ephemeral = record(
        "ephemeral",
        "assistant.message",
        json!({"messageId":"temporary","content":"temporarymarker"}),
    );
    ephemeral["ephemeral"] = true.into();
    write(
        file.path(),
        &[
            h,
            record(
                "pending",
                "session.context_changed",
                json!({"cwd":"/pending","pendingGitContext":true}),
            ),
            record(
                "settled",
                "session.context_changed",
                json!({"cwd":"/settled","repository":"team/settled"}),
            ),
            record(
                "resumed",
                "session.resume",
                json!({"context":{"cwd":"/other","repository":"team/other"}}),
            ),
            record(
                "request",
                "user.message",
                json!({"content":"Explain the importer"}),
            ),
            record(
                "first",
                "assistant.message",
                json!({"messageId":"first","apiCallId":"same-call","chunkIndex":0,"chunkCount":2,"content":"First complete chunk."}),
            ),
            ephemeral,
            record(
                "second",
                "assistant.message",
                json!({"messageId":"second","apiCallId":"same-call","chunkIndex":1,"chunkCount":2,"content":"Second complete chunk."}),
            ),
        ],
    );
    let parsed = parse(file.path()).unwrap().chat.unwrap();
    assert_eq!(parsed.project.as_deref(), Some("/settled"));
    assert_eq!(parsed.repository.as_deref(), Some("team/settled"));
    assert_eq!(parsed.messages.len(), 3);
    assert_eq!(parsed.messages.inline()[1].content, "First complete chunk.");
    assert_eq!(
        parsed.messages.inline()[2].content,
        "Second complete chunk."
    );
    assert_eq!(
        parsed.messages.inline()[2].context,
        MessageContext::Parent {
            id: parsed.messages.inline()[1].id.clone()
        }
    );
}

#[tokio::test]
#[ignore = "requires MASH_TEST_MODELS_DIR containing an explicit model installation"]
async fn core_retrieves_native_copilot_prose_with_real_models() {
    use crate::core::{IndexRequest, Operation, Paths, Request, Response};
    let dir = tempfile::tempdir().unwrap();
    let history = dir.path().join("events.jsonl");
    write(&history, &fixture());
    let config = dir.path().join("config.toml");
    std::fs::write(&config,toml::to_string(&toml::toml!{version=1 providers={copilot={roots=[(history.to_str().unwrap())]}} index={all_history=true}}).unwrap()).unwrap();
    let paths = Paths {
        config: Some(config),
        db: Some(dir.path().join("index.sqlite3")),
        models_dir: Some(std::env::var_os("MASH_TEST_MODELS_DIR").unwrap().into()),
    };
    crate::core::execute(Request {
        paths: paths.clone(),
        operation: Operation::Index(IndexRequest::default()),
    })
    .await
    .unwrap();
    for query in ["cbioportal importer", "structural variants"] {
        let Response::Search(search) = crate::core::execute(Request {
            paths: paths.clone(),
            operation: Operation::Search {
                shared_vector_search: None,
                fusion: crate::views::config::FusionOverrides::default(),
                ranking: None,
                diagnostics: false,
                query: query.into(),
                intent: None,
                limit: Some(5),
                offset: 0,
                unit: Some(crate::search_results::SearchUnit::Chats),
                filters: Box::new(crate::search_filter::Selection {
                    provider: vec![Provider::Copilot],
                    ..Default::default()
                }),
                shared: false,
                local: true,
            },
        })
        .await
        .unwrap() else {
            panic!("search")
        };
        let crate::search_results::Results::Chats(threads) = search.results else {
            panic!("chats")
        };
        assert_eq!(threads.len(), 1, "{query}");
        assert_eq!(threads[0].provider, Provider::Copilot);
    }
}

#[test]
fn torn_event_lines_reject_the_snapshot_instead_of_failing_the_index() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let complete = fixture();
    assert!(parse(file.path()).is_err(), "an empty file has no header");
    for (records, tail, line) in [
        (&complete[..0], r#"{"type":"session.st"#, 1),
        (
            &complete[..],
            r#"{"id":"late","type":"assistant.mess"#,
            complete.len() + 1,
        ),
    ] {
        write(file.path(), records);
        std::fs::OpenOptions::new()
            .append(true)
            .open(file.path())
            .unwrap()
            .write_all(tail.as_bytes())
            .unwrap();
        let parsed = parse(file.path()).unwrap();
        assert!(parsed.chat.is_none());
        assert_eq!(parsed.malformed_records, 1);
        assert_eq!(parsed.first_record_issue.unwrap().line, line);
    }
    // Damage between records is counted once; later records are not applied.
    let mut damaged = complete[..2].to_vec();
    damaged.push(json!("placeholder"));
    damaged.extend_from_slice(&complete[2..]);
    write(file.path(), &damaged);
    let text =
        std::fs::read_to_string(file.path())
            .unwrap()
            .replacen("\"placeholder\"", "{\"type\":", 1);
    std::fs::write(file.path(), text).unwrap();
    let parsed = parse(file.path()).unwrap();
    assert!(parsed.chat.is_none());
    assert_eq!(parsed.malformed_records, 1);
    assert_eq!(parsed.first_record_issue.unwrap().line, 3);
    write(file.path(), &complete);
    assert!(parse(file.path()).unwrap().chat.is_some());
}
