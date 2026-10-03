use super::*;
mod migration;
mod native_capture;
use crate::{
    silver::{IndexOptions, direct_scans, index},
    storage::Storage,
};
use rusqlite::Connection;
use serde_json::{Value, json};

fn write(path: &Path, value: &Value) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}
fn ui_path(root: &Path) -> PathBuf {
    root.join("tasks/task-1/ui_messages.json")
}
fn sdk_path(root: &Path) -> PathBuf {
    root.join("task-1/task-1.messages.json")
}
fn ui_fixture() -> Value {
    json!([
        {"ts":1,"type":"say","say":"task","text":"Fix the cBioPortal importer"},
        {"ts":2,"type":"say","say":"text","text":"partial","partial":true,"seq":1},
        {"ts":2,"type":"say","say":"text","text":"Validate mutation metadata.","partial":false,"seq":3},
        {"ts":2,"type":"say","say":"text","text":"stale","seq":2},
        {"ts":3,"type":"ask","ask":"followup","text":"{\"question\":\"Should we include structural variants?\",\"options\":[\"Yes\",\"No\"]}"},
        {"ts":4,"type":"say","say":"user_feedback","text":"Yes, check structural variants too."},
        {"ts":5,"type":"say","say":"reasoning","text":"privatereasoningmarker"},
        {"ts":6,"type":"say","say":"tool","text":"privatetoolmarker"},
        {"ts":7,"type":"say","say":"future_status","text":"privatefuturemarker"},
        {"ts":8,"type":"ask","ask":"plan_mode_respond","text":"{\"response\":\"Test the importer before deploying.\"}"}
    ])
}
fn sdk_fixture() -> Value {
    json!({
        "version":1,"sessionId":"task-1","updated_at":"2026-09-30T10:00:01Z","agent":"lead",
        "origin":{"source":"cli","mode":"user","sessionId":"task-1"},
        "system_prompt":"privatesystemmarker",
        "messages":[
            {"id":"original","role":"user","ts":100,"content":"<mode_notice>privatemodemarker</mode_notice><user_input mode=\"act\">Fix the cBioPortal importer</user_input>"},
            {"role":"assistant","ts":101,"content":[{"type":"text","text":"Validate mutation metadata."},{"type":"thinking","thinking":"privatethoughtmarker"},{"type":"tool_use","name":"read_file","input":{"path":"privateargumentmarker"}}]},
            {"role":"user","content":[{"type":"tool_result","content":"privateresultmarker"},{"type":"text","text":"Also check structural variants."},{"type":"file","path":"/tmp/context","content":"privatefilebodymarker"},{"type":"image","data":"privatebinarymarker","mediaType":"image/png"}]},
            {"role":"user","content":"Run validation again.","metadata":{"displayOnly":true}},
            {"role":"user","content":"Run validation again."},
            {"role":"user","content":"privateautomaticmarker","metadata":{"userRunSpan":0}},
            {"role":"future-role","content":"privateunknownrolemarker"}
        ]
    })
}
fn options(path: &Path) -> IndexOptions {
    IndexOptions {
        providers: [(Provider::Cline, vec![path.into()])].into(),
        roots_are_authoritative: true,
        since_days: None,
    }
}
fn parsed(path: &Path) -> Conversation {
    parse(&SourceIdentity::file(path))
        .unwrap()
        .conversation
        .unwrap()
}

#[test]
fn native_ui_snapshot_preserves_visible_roles_and_replaces_stale_copies() {
    let dir = tempfile::tempdir().unwrap();
    let path = ui_path(dir.path());
    let fixture = ui_fixture();
    write(&path, &fixture);
    let before = std::fs::read(&path).unwrap();
    let conversation = parsed(&path);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(conversation.messages.len(), 8);
    assert_eq!(conversation.messages.inline()[0].role, Role::User);
    assert_eq!(
        conversation.messages.inline()[1].content,
        "Validate mutation metadata."
    );
    assert_eq!(
        conversation.messages.inline()[2].content,
        "Should we include structural variants?"
    );
    assert_eq!(conversation.messages.inline()[2].role, Role::Assistant);
    assert_eq!(conversation.messages.inline()[3].role, Role::User);
    assert_eq!(
        conversation.messages.inline()[7].content,
        "Test the importer before deploying."
    );
    assert!(
        conversation.messages.inline()[4..7]
            .iter()
            .all(|m| m.dialogue_text.as_deref() == Some(""))
    );
    assert_eq!(
        conversation.title.as_deref(),
        Some("Fix the cBioPortal importer")
    );
    let ids = conversation
        .messages
        .inline()
        .iter()
        .map(|m| m.id.clone())
        .collect::<Vec<_>>();
    write(&path, &fixture);
    assert_eq!(
        parsed(&path)
            .messages
            .inline()
            .iter()
            .map(|m| m.id.clone())
            .collect::<Vec<_>>(),
        ids
    );
}

#[test]
fn sdk_projection_retains_real_repeated_turns_and_excludes_injected_content() {
    let dir = tempfile::tempdir().unwrap();
    let path = sdk_path(dir.path());
    write(&path, &sdk_fixture());
    let conversation = parsed(&path);
    assert_eq!(
        conversation.messages.inline()[0].dialogue_text.as_deref(),
        Some("Fix the cBioPortal importer")
    );
    assert_eq!(
        conversation.messages.inline()[1].dialogue_text.as_deref(),
        Some("Validate mutation metadata.")
    );
    assert!(
        conversation.messages.inline()[1]
            .content
            .contains("privatethoughtmarker")
    );
    assert!(
        conversation.messages.inline()[1]
            .content
            .contains("privateargumentmarker")
    );
    assert_eq!(
        conversation.messages.inline()[2].dialogue_text.as_deref(),
        Some("Also check structural variants.")
    );
    assert!(
        conversation.messages.inline()[2]
            .content
            .contains("privateresultmarker")
    );
    assert!(
        !conversation.messages.inline()[2]
            .content
            .contains("privatefilebodymarker")
    );
    assert!(
        !conversation.messages.inline()[2]
            .content
            .contains("privatebinarymarker")
    );
    assert_ne!(
        conversation.messages.inline()[3].id,
        conversation.messages.inline()[4].id
    );
    assert_eq!(
        conversation.messages.inline()[3].dialogue_text,
        conversation.messages.inline()[4].dialogue_text
    );
    assert!(
        conversation.messages.inline()[5..]
            .iter()
            .all(|m| m.dialogue_text.as_deref() == Some(""))
    );
    let ids = conversation
        .messages
        .inline()
        .iter()
        .map(|m| m.id.clone())
        .collect::<Vec<_>>();
    let mut resumed = sdk_fixture();
    for (index, message) in resumed["messages"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        message["id"] = json!(format!("regenerated-{index}"));
        message["ts"] = json!(1000 + index);
    }
    write(&path, &resumed);
    assert_eq!(
        parsed(&path)
            .messages
            .inline()
            .iter()
            .map(|m| m.id.clone())
            .collect::<Vec<_>>(),
        ids
    );
    for agent in ["subagent", "teammate", "future-agent"] {
        resumed["agent"] = json!(agent);
        write(&path, &resumed);
        assert!(
            parsed(&path)
                .messages
                .inline()
                .iter()
                .all(|m| m.dialogue_text.as_deref() == Some(""))
        );
    }
}

#[test]
fn metadata_dependencies_invalidate_snapshots_and_identity_disagreement_fails() {
    let dir = tempfile::tempdir().unwrap();
    let path = ui_path(dir.path());
    write(&path, &ui_fixture());
    let source = direct_scans(&options(&path))
        .unwrap()
        .remove(0)
        .sources
        .remove(0);
    let stamp = source.stamp().unwrap();
    let metadata = metadata_path(&path).unwrap();
    write(
        &metadata,
        &json!([{"id":"task-1","task":"Importer repair","cwdOnTaskInitialization":"/work/cbioportal"}]),
    );
    assert!(stamp.verify(&source.identity).is_err());
    assert_eq!(parsed(&path).project.as_deref(), Some("/work/cbioportal"));
    assert_eq!(parsed(&path).title.as_deref(), Some("Importer repair"));
    let stamp = source.stamp().unwrap();
    std::fs::remove_file(metadata).unwrap();
    assert!(stamp.verify(&source.identity).is_err());
    assert!(parsed(&path).project.is_none());
    let path = sdk_path(dir.path());
    write(&path, &sdk_fixture());
    let manifest = metadata_path(&path).unwrap();
    write(
        &manifest,
        &json!({"version":1,"session_id":"task-1","workspace_root":"/work/cbioportal","started_at":"2026-09-30T10:00:00Z","metadata":{"title":"SDK importer"}}),
    );
    assert_eq!(parsed(&path).project.as_deref(), Some("/work/cbioportal"));
    assert_eq!(parsed(&path).created_at, Some(1_790_762_400_000));
    write(
        &manifest,
        &json!({"version":1,"session_id":"different","workspace_root":"/work/other","started_at":"2026-09-30T10:00:00Z"}),
    );
    assert!(parse(&SourceIdentity::file(path)).is_err());
}

#[test]
fn sdk_native_question_replies_and_completion_summaries_are_visible_prose() {
    let dir = tempfile::tempdir().unwrap();
    let path = sdk_path(dir.path());
    let mut fixture = sdk_fixture();
    fixture["messages"] = json!([
        {"role":"assistant","content":[{"type":"tool_use","id":"question-1","name":"ask_question","input":{"question":"Should structural variants be imported?","options":["Yes","No"]}}]},
        {"role":"user","metadata":{"userRunSpan":0},"content":[{"type":"tool_result","tool_use_id":"question-1","name":"ask_question","content":"Yes, include structural variants."}]},
        {"role":"assistant","content":[{"type":"tool_use","name":"submit_and_exit","input":{"summary":"The cBioPortal importer now validates mutation metadata."}}]},
        {"role":"assistant","content":[{"type":"tool_use","name":"attempt_completion","input":{"result":"The import is complete."}}]},
        {"role":"user","metadata":{"displayRole":"system"},"content":"privatemodelcontextmarker"},
        {"role":"user","content":"<user_input mode=\"act\">[TASK RESUMPTION] privateresumemarker</user_input>"},
        {"role":"user","content":"<hook_context source=\"test\">privatehookmarker</hook_context>"},
        {"role":"user","content":"The user approved switching to act mode. Continue with the approved plan now."}
    ]);
    write(&path, &fixture);
    let conversation = parsed(&path);
    assert_eq!(
        conversation.messages.inline()[0].dialogue_text.as_deref(),
        Some("Should structural variants be imported?")
    );
    assert_eq!(
        conversation.messages.inline()[1].dialogue_text.as_deref(),
        Some("Yes, include structural variants.")
    );
    assert_eq!(
        conversation.messages.inline()[2].dialogue_text.as_deref(),
        Some("The cBioPortal importer now validates mutation metadata.")
    );
    assert_eq!(
        conversation.messages.inline()[3].dialogue_text.as_deref(),
        Some("The import is complete.")
    );
    assert!(
        conversation.messages.inline()[4..]
            .iter()
            .all(|m| m.dialogue_text.as_deref() == Some(""))
    );
    fixture["messages"][1]["content"][0]["is_error"] = json!(true);
    write(&path, &fixture);
    assert_eq!(
        parsed(&path).messages.inline()[1].dialogue_text.as_deref(),
        Some("")
    );
}

fn catalog(root: &Path, artifact: &Path) -> (PathBuf, Connection) {
    let path = root.join("sessions.db");
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE sessions(session_id TEXT PRIMARY KEY,workspace_root TEXT,prompt TEXT,started_at TEXT,updated_at TEXT,messages_path TEXT,is_subagent INTEGER,metadata_json TEXT);").unwrap();
    connection.execute("INSERT INTO sessions VALUES('task-1','/work/cbioportal','Importer repair','2026-09-30T10:00:00Z','2026-09-30T10:00:01Z',?1,0,'{\"title\":\"Canonical SDK history\"}')",[artifact.to_str().unwrap()]).unwrap();
    (path, connection)
}

#[test]
fn native_catalog_reads_wal_and_custom_artifacts_without_duplicate_discovery() {
    let dir = tempfile::tempdir().unwrap();
    let path = sdk_path(dir.path());
    write(&path, &sdk_fixture());
    let (database, connection) = catalog(dir.path(), &path);
    let sources = direct_scans(&options(dir.path()))
        .unwrap()
        .remove(0)
        .sources;
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].identity.path, database);
    let before = std::fs::read(&database).unwrap();
    let parsed = parse(&sources[0].identity).unwrap().conversation.unwrap();
    assert_eq!(parsed.title.as_deref(), Some("Canonical SDK history"));
    assert_eq!(parsed.project.as_deref(), Some("/work/cbioportal"));
    assert_eq!(std::fs::read(&database).unwrap(), before);
    let stamp = sources[0].stamp().unwrap();
    connection
        .execute("UPDATE sessions SET workspace_root='/work/changed'", [])
        .unwrap();
    assert!(stamp.verify(&sources[0].identity).is_err());
    assert_eq!(
        parse(&sources[0].identity)
            .unwrap()
            .conversation
            .unwrap()
            .project
            .as_deref(),
        Some("/work/changed")
    );
    let stamp = sources[0].stamp().unwrap();
    let mut changed = sdk_fixture();
    changed["messages"][0]["content"] = json!("Updated importer request");
    write(&path, &changed);
    assert!(stamp.verify(&sources[0].identity).is_err());
    let roots = crate::silver::watch_roots(&options(&database));
    assert!(roots.contains(&path));
    changed["sessionId"] = json!("other");
    write(&path, &changed);
    assert!(parse(&sources[0].identity).is_err());
}

#[test]
fn malformed_refresh_preserves_revision_and_valid_retry_updates_search() {
    let dir = tempfile::tempdir().unwrap();
    let path = ui_path(dir.path());
    write(&path, &ui_fixture());
    let db = dir.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&db).unwrap();
    index(&mut storage, &options(&path)).unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let source = direct_scans(&options(&path))
        .unwrap()
        .remove(0)
        .sources
        .remove(0);
    let checkpoint = source.stamp().unwrap();
    std::fs::write(&path, b"[{\"ts\":").unwrap();
    let mut storage = Storage::open_writer(&db).unwrap();
    assert!(index(&mut storage, &options(&path)).is_err());
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 8);
    assert!(
        storage
            .source_checkpoint_matches(Provider::Cline, &source.identity, &checkpoint)
            .unwrap()
    );
    drop(storage);
    let mut fixture = ui_fixture();
    fixture[2]["text"] = json!("Updated mutation validation.");
    write(&path, &fixture);
    let mut storage = Storage::open_writer(&db).unwrap();
    index(&mut storage, &options(&path)).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(
        storage
            .search("Updated", 10, Some(Provider::Cline), None)
            .unwrap()
            .len(),
        1
    );
    for marker in [
        "privatereasoningmarker",
        "privatetoolmarker",
        "privatefuturemarker",
    ] {
        assert!(storage.search(marker, 10, None, None).unwrap().is_empty());
    }
    let request = storage
        .search("cbioportal", 10, None, None)
        .unwrap()
        .remove(0);
    assert!(
        storage
            .view(&request.id, 1)
            .unwrap()
            .iter()
            .any(|m| m.content.contains("Updated"))
    );
}

#[test]
fn malformed_and_oversized_snapshots_and_sequences_fail_explicitly() {
    let dir = tempfile::tempdir().unwrap();
    let path = sdk_path(dir.path());
    for value in [
        json!({}),
        json!({"version":2,"sessionId":"x","agent":"lead","updated_at":"2026-09-30T10:00:00Z","messages":[]}),
    ] {
        write(&path, &value);
        assert!(parse(&SourceIdentity::file(&path)).is_err());
    }
    std::fs::write(&path, " ".repeat(super::super::MAX_RECORD_BYTES + 1)).unwrap();
    assert!(parse(&SourceIdentity::file(&path)).is_err());
    let mut value = sdk_fixture();
    value["messages"] = json!(vec![json!({"role":"user","content":"x"}); 16_385]);
    write(&path, &value);
    assert!(parse(&SourceIdentity::file(&path)).is_err());
    value["messages"] =
        json!([{"role":"user","content":vec![json!({"type":"text","text":""});16_385]}]);
    write(&path, &value);
    assert!(parse(&SourceIdentity::file(&path)).is_err());
}

#[tokio::test]
#[ignore = "requires MASH_TEST_MODELS_DIR containing an explicit model installation"]
async fn core_retrieves_native_cline_threads_with_real_models() {
    use crate::core::{IndexRequest, Operation, Paths, Request, Response};
    #[derive(Debug)]
    enum Store {
        Ui,
        Sdk,
        Migrated,
    }
    for store in [Store::Ui, Store::Sdk, Store::Migrated] {
        let sdk = !matches!(store, Store::Ui);
        let dir = tempfile::tempdir().unwrap();
        let path = if sdk {
            sdk_path(dir.path())
        } else {
            ui_path(dir.path())
        };
        let fixture = match store {
            Store::Ui => ui_fixture(),
            Store::Sdk => sdk_fixture(),
            Store::Migrated => {
                write(&ui_path(dir.path()), &ui_fixture());
                migration::resumed()
            }
        };
        write(&path, &fixture);
        write(
            &metadata_path(&path).unwrap(),
            &if sdk {
                json!({"version":1,"session_id":"task-1","workspace_root":"/work/cbioportal","started_at":"2026-09-30T10:00:00Z"})
            } else {
                json!([{"id":"task-1","task":"Fix the cBioPortal importer","cwdOnTaskInitialization":"/work/cbioportal"}])
            },
        );
        let root = if matches!(store, Store::Migrated) {
            dir.path()
        } else {
            &path
        };
        let config = dir.path().join("config.toml");
        std::fs::write(&config,toml::to_string(&toml::toml!{version=1 providers={cline={roots=[(root.to_str().unwrap())]}} index={all_history=true}}).unwrap()).unwrap();
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
            for unit in [
                crate::search_results::SearchUnit::Threads,
                crate::search_results::SearchUnit::Messages,
            ] {
                let Response::Search(response) = crate::core::execute(Request {
                    paths: paths.clone(),
                    operation: Operation::Search {
                        query: query.into(),
                        intent: None,
                        limit: Some(5),
                        unit: Some(unit),
                        filters: Box::new(crate::search_filter::Selection {
                            provider: vec![Provider::Cline],
                            project: vec!["/work/cbioportal".into()],
                            role: vec![crate::role::Role::User],
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
                match response.results {
                    crate::search_results::Results::Threads(threads) => {
                        assert_eq!(threads.len(), 1, "{query} {store:?}");
                        assert_eq!(threads[0].provider, Provider::Cline);
                        assert_eq!(
                            threads[0].metadata.project.as_deref(),
                            Some("/work/cbioportal")
                        );
                    }
                    crate::search_results::Results::Messages(messages) => {
                        assert!(!messages.is_empty(), "{query} {store:?}");
                        assert!(messages.iter().all(|m| m.provider == Provider::Cline));
                    }
                }
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn non_regular_external_artifacts_are_rejected_before_opening() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("task.messages.json");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        parse(&SourceIdentity::file(path))
            .err()
            .unwrap()
            .error
            .message
            .contains("regular file")
    );
}
