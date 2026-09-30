use super::*;
use crate::{
    ingestion::{IndexOptions, direct_scans, index},
    provider::Provider,
    storage::Storage,
};
use serde_json::{Value, json};
use std::io::Write;

fn header() -> Value {
    use sha2::{Digest, Sha256};
    json!({"sessionId":"gemini-session","projectHash":hex::encode(Sha256::digest(b"/work/cbioportal")),"startTime":"2026-09-30T10:00:00Z","lastUpdated":"2026-09-30T10:00:01Z","kind":"main"})
}

fn message(id: &str, kind: &str, content: Value) -> Value {
    let mut message = json!({"id":id,"type":kind,"timestamp":"2026-09-30T10:00:01Z"});
    message["content"] = content;
    message
}

fn fixture() -> Vec<Value> {
    vec![
        header(),
        {
            let mut m = message(
                "request",
                "user",
                json!([{"text":"privatemodelpromptmarker"}]),
            );
            m["displayContent"] = json!([{"text":"Fix the cBioPortal importer"}]);
            m
        },
        message("answer", "gemini", json!("Incomplete response.")),
        {
            let mut m = message(
                "answer",
                "gemini",
                json!([
                    {"text":"Validate mutation metadata before importing."},
                    {"text":"privatethoughtmarker","thought":true},
                    {"functionCall":{"name":"read_file","args":{"path":"privateargumentmarker"}}},
                    {"inlineData":{"mimeType":"image/png","data":"privatebinarymarker"}}
                ]),
            );
            m["thoughts"] = json!([{"subject":"Plan","description":"privatethoughtmetadatamarker","timestamp":"2026-09-30T10:00:01Z"}]);
            m["toolCalls"] = json!([{"id":"call","name":"read_file","args":{"path":"privatetoolmetadata"},"result":"privateresultmarker","status":"success","timestamp":"2026-09-30T10:00:01Z"}]);
            m
        },
        message(
            "tool",
            "user",
            json!([{"functionResponse":{"name":"read_file","response":{"output":"privatefunctionresponsemarker"}}}]),
        ),
        message("info", "info", json!("privateinfomarker")),
        message(
            "hook",
            "user",
            json!("<hook_context>privatehookmarker</hook_context>"),
        ),
        message("slash", "user", json!("/model privatecommandmarker")),
        message(
            "followup",
            "user",
            json!(
                "Also check structural variants.<hook_context>privateinlinehookmarker</hook_context>"
            ),
        ),
        json!({"$set":{"summary":"Importer repair","lastUpdated":"2026-09-30T10:00:02Z","memoryScratchpad":{"version":1,"workflowSummary":"privatememorymarker"}}}),
    ]
}

fn write(path: &Path, values: &[Value]) {
    let mut file = File::create(path).unwrap();
    for value in values {
        writeln!(file, "{value}").unwrap();
    }
}

fn options(path: &Path) -> IndexOptions {
    IndexOptions {
        providers: [(Provider::GeminiCli, vec![path.into()])].into(),
        roots_are_authoritative: true,
        since_days: None,
    }
}

fn history(dir: &Path) -> PathBuf {
    let chats = dir.join("project/chats");
    std::fs::create_dir_all(&chats).unwrap();
    std::fs::write(
        chats.parent().unwrap().join(".project_root"),
        "/work/cbioportal",
    )
    .unwrap();
    let path = chats.join("session-native.jsonl");
    write(&path, &fixture());
    path
}

#[test]
fn native_updates_keep_one_message_and_only_displayed_prose_is_searchable() {
    let dir = tempfile::tempdir().unwrap();
    let path = history(dir.path());
    let original = std::fs::read(&path).unwrap();
    let conversation = parse(&path).unwrap().conversation.unwrap();
    assert_eq!(conversation.messages.len(), 7);
    assert_eq!(conversation.project.as_deref(), Some("/work/cbioportal"));
    assert_eq!(conversation.title.as_deref(), Some("Importer repair"));
    assert!(conversation.repository.is_none());
    let canonical = conversation
        .messages
        .iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let projection = conversation
        .messages
        .iter()
        .map(|m| m.search_projection.as_deref().unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!canonical.contains("Incomplete response"));
    for marker in [
        "privatemodelpromptmarker",
        "privatethoughtmarker",
        "privateargumentmarker",
        "privatethoughtmetadatamarker",
        "privatetoolmetadata",
        "privateresultmarker",
        "privatefunctionresponsemarker",
        "privateinfomarker",
        "privatehookmarker",
        "privatecommandmarker",
        "privateinlinehookmarker",
    ] {
        assert!(canonical.contains(marker), "{marker}");
        assert!(!projection.contains(marker), "{marker}");
    }
    assert!(!canonical.contains("privatebinarymarker"));
    assert!(!canonical.contains("privatememorymarker"));
    assert!(projection.contains("Fix the cBioPortal importer"));
    assert!(projection.contains("Validate mutation metadata"));
    assert!(projection.contains("structural variants"));
    let repeated = parse(&path).unwrap().conversation.unwrap();
    assert_eq!(
        conversation
            .messages
            .iter()
            .map(|m| &m.id)
            .collect::<Vec<_>>(),
        repeated.messages.iter().map(|m| &m.id).collect::<Vec<_>>()
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn checkpoints_and_inclusive_rewinds_follow_the_upstream_loaded_history() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let first = message("first", "user", json!("First saved request"));
    let second = message("second", "gemini", json!("Second saved answer"));
    write(
        file.path(),
        &[
            header(),
            first.clone(),
            second.clone(),
            json!({"$rewindTo":"second"}),
            message("new", "gemini", json!("Replacement answer")),
        ],
    );
    let parsed = parse(file.path()).unwrap().conversation.unwrap();
    assert_eq!(parsed.messages.len(), 2);
    assert_eq!(parsed.messages[1].content, "Replacement answer");
    write(
        file.path(),
        &[
            header(),
            first,
            second.clone(),
            json!({"$set":{"messages":[second],"lastUpdated":"2026-09-30T11:00:00Z"}}),
        ],
    );
    let checkpoint = parse(file.path()).unwrap().conversation.unwrap();
    assert_eq!(checkpoint.messages.len(), 1);
    assert_eq!(checkpoint.messages[0].content, "Second saved answer");
    assert_eq!(
        checkpoint.messages[0].search_projection.as_deref(),
        Some("Second saved answer")
    );
    let h = message(
        "context",
        "user",
        json!("<session_context>privatecompactmarker</session_context>"),
    );
    write(file.path(), &[header(), json!({"$set":{"messages":[h]}})]);
    let compact = parse(file.path()).unwrap().conversation.unwrap();
    assert_eq!(compact.messages[0].search_projection.as_deref(), Some(""));
    write(
        file.path(),
        &[
            header(),
            message("first", "user", json!("Old message")),
            json!({"$rewindTo":"absent"}),
        ],
    );
    assert!(
        parse(file.path())
            .err()
            .unwrap()
            .error
            .message
            .contains("missing message")
    );
}

#[test]
fn model_only_checkpoint_turns_do_not_become_human_requests_or_retrievable_answers() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let request = message(
        "request",
        "user",
        json!("Actual cBioPortal importer request"),
    );
    let summary = message(
        "summary",
        "user",
        json!("Plain synthetic summary with no reliable XML tags."),
    );
    let acknowledgement = message(
        "ack",
        "gemini",
        json!("Got it. Thanks for the additional context!"),
    );
    write(
        file.path(),
        &[
            header(),
            request.clone(),
            json!({"$set":{"messages":[summary.clone(),acknowledgement,request]}}),
        ],
    );
    let state = parse(file.path()).unwrap().conversation.unwrap();
    assert_eq!(state.messages.len(), 3);
    assert_eq!(state.messages[0].search_projection.as_deref(), Some(""));
    assert_eq!(state.messages[1].search_projection.as_deref(), Some(""));
    assert_eq!(
        state.messages[2].search_projection.as_deref(),
        Some("Actual cBioPortal importer request")
    );
    // A subsequent direct message is recorded evidence, even with a reused ID.
    let mut writer = std::fs::OpenOptions::new()
        .append(true)
        .open(file.path())
        .unwrap();
    writeln!(
        writer,
        "{}",
        message(
            "summary",
            "user",
            json!("New directly recorded user request")
        )
    )
    .unwrap();
    assert_eq!(
        parse(file.path()).unwrap().conversation.unwrap().messages[0]
            .search_projection
            .as_deref(),
        Some("New directly recorded user request")
    );
    let mut snapshot = header();
    snapshot["messages"] = json!([summary]);
    write(file.path(), &[snapshot]);
    assert_eq!(
        parse(file.path()).unwrap().conversation.unwrap().messages[0]
            .search_projection
            .as_deref(),
        Some("Plain synthetic summary with no reliable XML tags."),
        "complete native snapshots have no event provenance to infer"
    );
}

#[test]
fn native_json_and_jsonl_migration_have_the_same_identity_and_do_not_duplicate_sources() {
    let dir = tempfile::tempdir().unwrap();
    let path = history(dir.path());
    let mut json_session = header();
    json_session["messages"] = json!([
        message("request", "user", json!("Fix the cBioPortal importer")),
        message(
            "answer",
            "gemini",
            json!("Validate mutation metadata before importing.")
        )
    ]);
    let old = path.with_extension("json");
    std::fs::write(&old, serde_json::to_vec_pretty(&json_session).unwrap()).unwrap();
    let old_parsed = parse(&old).unwrap().conversation.unwrap();
    write(
        &path,
        &[
            header(),
            message("request", "user", json!("Fix the cBioPortal importer")),
            message(
                "answer",
                "gemini",
                json!("Validate mutation metadata before importing."),
            ),
        ],
    );
    let new = parse(&path).unwrap().conversation.unwrap();
    assert_eq!(new.id, old_parsed.id);
    assert_eq!(new.messages[0].id, old_parsed.messages[0].id);
    std::fs::write(dir.path().join("settings.json"), "unrelated configuration").unwrap();
    let scan = direct_scans(&options(dir.path())).unwrap();
    assert_eq!(scan[0].sources.len(), 1);
    assert_eq!(scan[0].sources[0].identity.path, path);
    assert_eq!(
        direct_scans(&options(&old)).unwrap()[0].sources.len(),
        1,
        "an explicit old file is still readable"
    );
    std::fs::remove_file(path).unwrap();
    assert_eq!(
        direct_scans(&options(dir.path())).unwrap()[0].sources[0]
            .identity
            .path,
        old
    );
}

#[test]
fn optional_project_markers_invalidate_stamps_without_changing_history_age() {
    let dir = tempfile::tempdir().unwrap();
    let path = history(dir.path());
    let marker = project_marker(&path).unwrap();
    std::fs::remove_file(&marker).unwrap();
    let scan = direct_scans(&options(&path)).unwrap();
    let source = &scan[0].sources[0];
    let before = source.stamp().unwrap();
    assert_eq!(
        before.activity_ns,
        Some(super::super::source_stamp(&path).unwrap().modified_ns)
    );
    assert_eq!(
        parse(&path).unwrap().conversation.unwrap().project.unwrap(),
        header()["projectHash"].as_str().unwrap()
    );
    std::fs::write(&marker, "/work/cbioportal").unwrap();
    assert!(before.verify(&source.identity).is_err());
    let present = source.stamp().unwrap();
    assert_ne!(before.fingerprint, present.fingerprint);
    assert_eq!(present.activity_ns, before.activity_ns);
    assert_eq!(
        parse(&path)
            .unwrap()
            .conversation
            .unwrap()
            .project
            .as_deref(),
        Some("/work/cbioportal")
    );
    std::fs::remove_file(&marker).unwrap();
    assert!(present.verify(&source.identity).is_err());
    assert!(crate::ingestion::watch_roots(&options(&path)).contains(&marker));
    assert!(crate::ingestion::watch_roots(&options(path.parent().unwrap())).contains(&marker));
}

#[test]
fn subagents_commands_and_unknown_message_roles_remain_context_only() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let mut records = fixture();
    records[0]["kind"] = "subagent".into();
    write(file.path(), &records);
    assert!(
        parse(file.path())
            .unwrap()
            .conversation
            .unwrap()
            .messages
            .iter()
            .all(|m| m.search_projection.as_deref() == Some(""))
    );
    write(
        file.path(),
        &[
            header(),
            message("unknown", "future-role", json!("futuremarker")),
        ],
    );
    let parsed = parse(file.path()).unwrap().conversation.unwrap();
    assert_eq!(parsed.messages[0].role, crate::role::Role::System);
    assert_eq!(parsed.messages[0].search_projection.as_deref(), Some(""));
}

#[test]
fn incomplete_refresh_preserves_canonical_history_and_retries_updated_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = history(dir.path());
    let database = dir.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options(&path)).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let scan = direct_scans(&options(&path)).unwrap();
    let source = &scan[0].sources[0];
    let checkpoint = source.stamp().unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"{\"$set\":")
        .unwrap();
    let mut storage = Storage::open_writer(&database).unwrap();
    assert!(index(&mut storage, &options(&path)).is_err());
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 7);
    assert!(
        storage
            .source_checkpoint_matches(Provider::GeminiCli, &source.identity, &checkpoint)
            .unwrap()
    );
    drop(storage);
    let mut valid = fixture();
    valid.push(message(
        "answer",
        "gemini",
        json!("Updated mutation metadata validation."),
    ));
    write(&path, &valid);
    let mut storage = Storage::open_writer(&database).unwrap();
    assert_eq!(
        index(&mut storage, &options(&path))
            .unwrap()
            .changed_messages,
        1
    );
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 7);
    assert_eq!(storage.counts().unwrap().searchable_messages, 3);
    assert_eq!(
        storage
            .search("Updated", 10, Some(Provider::GeminiCli), None)
            .unwrap()
            .len(),
        1
    );
    assert!(
        storage
            .search("privateinlinehookmarker", 10, None, None)
            .unwrap()
            .is_empty()
    );
    let request = storage
        .search("cbioportal", 10, None, None)
        .unwrap()
        .remove(0);
    assert!(
        storage.view(&request.id, 1).unwrap()[0]
            .content
            .contains("privatemodelpromptmarker")
    );
}

#[test]
fn malformed_envelopes_and_oversized_native_documents_fail_closed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    for invalid in [
        message("bad", "user", json!(42)),
        message("", "user", json!("empty id")),
        json!({"id":"bad","type":"user","timestamp":"bad","content":"bad time"}),
        json!({"$set":{"messages":{}}}),
        json!({"other":"unknown-record"}),
    ] {
        write(&path, &[header(), invalid]);
        assert!(parse(&path).is_err());
    }
    write(&path, &[message("bad", "user", json!("no header"))]);
    assert!(parse(&path).is_err());
    let too_large = dir.path().join("session.json");
    std::fs::write(&too_large, vec![b' '; super::super::MAX_RECORD_BYTES + 1]).unwrap();
    assert!(
        parse(&too_large)
            .err()
            .unwrap()
            .error
            .message
            .contains("8 MiB")
    );
    let large_line = format!(
        "{}\n{}\n",
        header(),
        " ".repeat(super::super::MAX_RECORD_BYTES + 1)
    );
    std::fs::write(&path, large_line).unwrap();
    assert!(parse(&path).is_err());
}

#[tokio::test]
#[ignore = "requires MASH_TEST_MODELS_DIR containing an explicit model installation"]
async fn core_retrieves_native_gemini_threads_with_real_models() {
    use crate::core::{IndexRequest, Operation, Paths, Request, Response};
    let dir = tempfile::tempdir().unwrap();
    let history = history(dir.path());
    let config = dir.path().join("config.toml");
    std::fs::write(&config,toml::to_string(&toml::toml!{version=1 providers={"gemini-cli"={roots=[(history.to_str().unwrap())]}} index={all_history=true}}).unwrap()).unwrap();
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
        let Response::Search(response) = crate::core::execute(Request {
            paths: paths.clone(),
            operation: Operation::Search {
                query: query.into(),
                limit: 5,
                unit: crate::search_results::SearchUnit::Threads,
                filters: Box::new(crate::search_filter::Selection {
                    provider: vec![Provider::GeminiCli],
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
        let crate::search_results::Results::Threads(threads) = response.results else {
            panic!("threads")
        };
        assert_eq!(threads.len(), 1, "{query}");
        assert_eq!(threads[0].provider, Provider::GeminiCli);
        assert_eq!(
            threads[0].metadata.project.as_deref(),
            Some("/work/cbioportal")
        );
    }
}
