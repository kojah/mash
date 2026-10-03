use super::*;
use crate::silver::{IndexOptions, discovery::discover_history_files, index};
use crate::storage::Storage;
use serde_json::{Value, json};
use std::path::PathBuf;

mod legacy;

struct Fixture {
    dir: tempfile::TempDir,
    db: PathBuf,
    chats: PathBuf,
    upstream: rusqlite::Connection,
}

impl Fixture {
    fn new(global: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join(if global {
            "globalStorage"
        } else {
            "workspaceStorage/workspace-hash"
        });
        std::fs::create_dir_all(&root).unwrap();
        let db = root.join("state.vscdb");
        let upstream = rusqlite::Connection::open(&db).unwrap();
        upstream.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE ItemTable(key TEXT PRIMARY KEY, value TEXT);").unwrap();
        let chats = discovery::directory(&db).unwrap();
        std::fs::create_dir(&chats).unwrap();
        std::fs::write(
            root.join("workspace.json"),
            r#"{"folder":"file:///work/cbioportal"}"#,
        )
        .unwrap();
        let result = Self {
            dir,
            db,
            chats,
            upstream,
        };
        result.catalog(&json!({"chat-session":{"sessionId":"chat-session","title":"cBioPortal chat","lastMessageDate":1000}}));
        result
    }
    fn catalog(&self, entries: &Value) {
        self.upstream
            .execute(
                "INSERT OR REPLACE INTO ItemTable VALUES ('chat.ChatSessionStore.index',?1)",
                [json!({"version":1,"entries":entries}).to_string()],
            )
            .unwrap();
    }
    fn write(&self, extension: &str, value: &str) {
        std::fs::write(self.chats.join(format!("chat-session.{extension}")), value).unwrap();
    }
    fn sources(&self) -> Result<Vec<HistorySource>, AppError> {
        discover_history_files(Provider::Copilot, std::slice::from_ref(&self.db), true)
            .map(|d| d.sources)
    }
    fn parse(&self) -> Result<Conversation, AppError> {
        parse(&self.sources()?.remove(0)).map(|p| p.conversation.unwrap())
    }
    fn options(&self) -> IndexOptions {
        IndexOptions {
            providers: [(Provider::Copilot, vec![self.db.clone()])].into(),
            roots_are_authoritative: true,
            since_days: None,
        }
    }
}

fn request(id: &str, text: &str) -> Value {
    json!({"requestId":id,"message":{"text":text,"parts":[]},"timestamp":100,
        "responseId":format!("response-{id}"),"responseTimestamp":200,"variableData":{"variables":[]},
        "response":[{"value":"Validate mutation metadata before importing."},
            {"kind":"thinking","value":["privatefirstthought","privatesecondthought"]},
            {"kind":"toolInvocationSerialized","invocationMessage":"privatetoolmarker","pastTenseMessage":{"value":"privatecompletedtoolmarker"},"toolSpecificData":{"output":"privateoutputmarker"},"resultDetails":{"input":"privateinputmarker","output":"privateresultmarker"}},
            {"kind":"futurePart","value":"privatefuturemarker"}]})
}

fn session() -> Value {
    json!({"version":3,"sessionId":"chat-session","creationDate":50,"customTitle":"Native Chat",
        "repoData":{"remoteUrl":"https://github.com/cBioPortal/cbioportal"},
        "requests":[request("request-1","Fix the cBioPortal importer")]})
}

fn log(entries: &[Value]) -> String {
    use std::fmt::Write as _;
    entries.iter().fold(String::new(), |mut output, value| {
        writeln!(output, "{value}").unwrap();
        output
    })
}

#[test]
fn catalog_snapshot_projects_prose_and_keeps_cli_identity_distinct() {
    for global in [false, true] {
        let f = Fixture::new(global);
        f.write("json", &session().to_string());
        // An orphaned transcript and a non-local catalog entry are not sources.
        std::fs::write(f.chats.join("orphan.json"), session().to_string()).unwrap();
        f.catalog(&json!({"chat-session":{"sessionId":"chat-session","title":"Catalog title","lastMessageDate":1000},
            "copilotcli:/external":{"sessionId":"external","title":"CLI bridge","lastMessageDate":1000,"isExternal":true}}));
        let before = std::fs::read(f.chats.join("chat-session.json")).unwrap();
        let conversation = f.parse().unwrap();
        assert_eq!(conversation.project.as_deref(), Some("/work/cbioportal"));
        assert_eq!(
            conversation.repository.as_deref(),
            Some("https://github.com/cBioPortal/cbioportal")
        );
        assert_eq!(conversation.title.as_deref(), Some("Native Chat"));
        let rows = conversation.messages.inline();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[1].dialogue_text.as_deref(),
            Some("Validate mutation metadata before importing.")
        );
        assert!(rows[1].content.contains("privatefirstthought"));
        assert!(rows[1].content.contains("privatefuturemarker"));
        assert!(rows[1].content.contains("privatetoolmarker"));
        assert!(rows[1].content.contains("privateoutputmarker"));
        assert!(rows[1].content.contains("privateresultmarker"));
        assert_ne!(
            conversation.id,
            crate::silver::parsers::stable_id("session", &["copilot", "chat-session"])
        );
        assert_eq!(
            std::fs::read(f.chats.join("chat-session.json")).unwrap(),
            before
        );
        assert_eq!(f.sources().unwrap().len(), 1);
    }
}

#[test]
fn logs_replay_edits_appends_truncation_and_typed_deletions() {
    let f = Fixture::new(false);
    f.write("json", &session().to_string());
    let snapshot_source = f.sources().unwrap().remove(0);
    let original = f.parse().unwrap();
    f.write("jsonl", &log(&[
        json!({"kind":0,"v":session()}),
        json!({"kind":1,"k":["requests",0,"message","text"],"v":"Fix cBioPortal structural variants"}),
        json!({"kind":1,"k":["requests",0,"response",0],"v":{"value":"Updated structural variant answer"}}),
        json!({"kind":2,"k":["requests",0,"response"],"v":[{"value":"Additional answer"}]}),
        json!({"kind":2,"k":["requests"],"v":[request("discarded","discarded marker")]}),
        json!({"kind":2,"k":["requests"],"i":1,"v":[request("surviving","surviving question")]}),
        json!({"kind":1,"k":["requests",1,"hiddenFromTranscript"],"v":true}),
        json!({"kind":3,"k":["requests",1,"hiddenFromTranscript"]}),
        json!({"kind":3,"k":["customTitle"]}),
        json!({"kind":1,"k":["inputState","inputText"],"v":"privateunsentdraft"}),
    ]));
    assert!(
        parse(&snapshot_source).is_err(),
        "changed storage selection must retry"
    );
    assert!(snapshot_source.stamp().is_ok());
    let result = f.parse().unwrap();
    assert_eq!(result.id, original.id);
    assert_eq!(result.source.key(), original.source.key());
    let rows = result.messages.inline();
    assert_eq!(rows.len(), 4);
    assert_eq!(
        rows[0].id,
        original.messages.inline()[0].id,
        "edits retain identity"
    );
    assert_eq!(rows[0].content, "Fix cBioPortal structural variants");
    assert_eq!(
        rows[1].dialogue_text.as_deref(),
        Some("Updated structural variant answer\nAdditional answer")
    );
    assert_eq!(rows[2].content, "surviving question");
    assert_eq!(result.title.as_deref(), Some("cBioPortal chat"));
    assert!(
        !rows
            .iter()
            .any(|r| r.content.contains("discarded") || r.content.contains("privateunsentdraft"))
    );
}

#[test]
fn indexing_refreshes_wal_catalog_and_transcript_with_atomic_retry() {
    let f = Fixture::new(false);
    f.write("json", &session().to_string());
    let database = f.dir.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&database).unwrap();
    let options = f.options();
    index(&mut storage, &options).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().searchable_messages, 2);
    assert!(
        !storage
            .search("importer", 10, Some(Provider::Copilot), None)
            .unwrap()
            .is_empty()
    );
    for text in [
        "privatetoolmarker",
        "privatefirstthought",
        "privatefuturemarker",
    ] {
        assert!(storage.search(text, 10, None, None).unwrap().is_empty());
    }
    let source = f.sources().unwrap().remove(0);
    let stamp = source.stamp().unwrap();
    f.write(
        "jsonl",
        &log(&[
            json!({"kind":0,"v":session()}),
            json!({"kind":1,"k":["requests",0,"message","text"],"v":"structural variants"}),
        ]),
    );
    assert!(stamp.verify(&source.identity).is_err());
    let prior = storage.counts().unwrap().messages;
    std::fs::OpenOptions::new()
        .append(true)
        .open(f.chats.join("chat-session.jsonl"))
        .unwrap()
        .write_all(b"{\"kind\":")
        .unwrap();
    drop(storage);
    let mut storage = Storage::open_writer(&database).unwrap();
    assert!(index(&mut storage, &options).is_err());
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, prior);
    assert!(
        !storage
            .search("importer", 10, None, None)
            .unwrap()
            .is_empty()
    );
    f.write(
        "jsonl",
        &log(&[
            json!({"kind":0,"v":session()}),
            json!({"kind":1,"k":["requests",0,"message","text"],"v":"structural variants"}),
        ]),
    );
    drop(storage);
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options).unwrap();
    storage.commit_writer().unwrap();
    assert!(
        storage
            .search("importer", 10, None, None)
            .unwrap()
            .is_empty()
    );
    assert!(
        !storage
            .search("structural", 10, None, None)
            .unwrap()
            .is_empty()
    );
    storage
        .with_publication_snapshot(|snapshot| {
            snapshot.visit(crate::shared::protocol::BatchKind::Messages, |batch| {
                let crate::shared::protocol::Batch::Messages { records } = batch else {
                    panic!("messages");
                };
                assert_eq!(records.len(), 2);
                assert_eq!(
                    records[1].projection,
                    "Validate mutation metadata before importing."
                );
                Ok(())
            })
        })
        .unwrap();
    f.catalog(&json!({}));
    drop(storage);
    let mut storage = Storage::open_writer(&database).unwrap();
    index(&mut storage, &options).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(
        storage.counts().unwrap().messages,
        0,
        "catalog removal owns deletion despite retained files"
    );
}

#[test]
fn malformed_logs_and_identity_changes_fail_instead_of_partial_replay() {
    let f = Fixture::new(false);
    for operations in [
        vec![json!({"kind":1,"k":["customTitle"],"v":"no initial state"})],
        vec![json!({"kind":0,"v":session()}), json!({"kind":99})],
        vec![
            json!({"kind":0,"v":session()}),
            json!({"kind":1,"k":["requests",90,"message"],"v":"bad"}),
        ],
        vec![
            json!({"kind":0,"v":session()}),
            json!({"kind":2,"k":["requests"],"i":90}),
        ],
        vec![
            json!({"kind":0,"v":session()}),
            json!({"kind":1,"k":["requests",0,"message"],"v":42}),
        ],
        vec![
            json!({"kind":0,"v":session()}),
            json!({"kind":1,"k":["version"],"v":99}),
        ],
        vec![
            json!({"kind":0,"v":session()}),
            json!({"kind":1,"k":["sessionId"],"v":"another-session"}),
        ],
        vec![
            json!({"kind":0,"v":session()}),
            json!({"kind":2,"k":["requests"],"v":[request("request-1","duplicate identity")]}),
        ],
    ] {
        f.write("jsonl", &log(&operations));
        assert!(
            f.parse().is_err(),
            "accepted invalid operations {operations:?}"
        );
    }
    f.catalog(&json!({"../escape":{"sessionId":"../escape","title":"bad","lastMessageDate":1000}}));
    assert!(f.sources().is_err());
    let roots = crate::silver::watch_roots(&f.options());
    assert!(roots.contains(&f.db.parent().unwrap().to_path_buf()));
}

use std::io::Write;

#[test]
fn old_snapshots_and_hidden_or_non_copilot_requests_preserve_context_only() {
    let f = Fixture::new(false);
    let mut snapshot = session();
    snapshot["version"] = json!(2);
    snapshot.as_object_mut().unwrap().remove("customTitle");
    snapshot["computedTitle"] = json!("Older native title");
    snapshot["workingDirectory"] = json!("file:///work/other%20project");
    snapshot["requests"][0]["message"] = json!("Older plain-string prompt");
    let mut hidden = request("hidden", "privatehiddenrequest");
    hidden["hiddenFromTranscript"] = json!(true);
    let mut automatic = request("automatic", "privateautomaticrequest");
    automatic["isSystemInitiated"] = json!(true);
    let mut other_agent = request("other-agent", "privatethirdpartyrequest");
    other_agent["agent"] = json!({"extensionId":{"value":"example.other-chat"}});
    snapshot["requests"]
        .as_array_mut()
        .unwrap()
        .extend([hidden, automatic, other_agent]);
    snapshot["pendingRequests"] = json!([{"request":request("unsent", "privateunsentrequest")}]);
    f.write("json", &snapshot.to_string());
    let conversation = f.parse().unwrap();
    assert_eq!(conversation.title.as_deref(), Some("Older native title"));
    assert_eq!(conversation.project.as_deref(), Some("/work/other project"));
    let rows = conversation.messages.inline();
    assert_eq!(rows.len(), 8);
    assert_eq!(rows[0].content, "Older plain-string prompt");
    assert!(
        rows[2..4]
            .iter()
            .all(|r| r.dialogue_text.as_deref() == Some(""))
    );
    assert_eq!(rows[4].dialogue_text.as_deref(), Some(""));
    assert!(
        rows[6..]
            .iter()
            .all(|r| r.dialogue_text.as_deref() == Some(""))
    );
    assert!(
        !rows
            .iter()
            .any(|r| r.content.contains("privateunsentrequest"))
    );
}

#[test]
fn one_provider_root_discovers_chat_and_cli_without_merging_sessions() {
    let f = Fixture::new(false);
    f.write("json", &session().to_string());
    let cli = f
        .dir
        .path()
        .join(".copilot/session-state/chat-session/events.jsonl");
    std::fs::create_dir_all(cli.parent().unwrap()).unwrap();
    std::fs::write(&cli, log(&[
        json!({"id":"start","parentId":null,"timestamp":"2026-10-01T10:00:00Z","type":"session.start","data":{"sessionId":"chat-session","version":1,"startTime":"2026-10-01T10:00:00Z"}}),
        json!({"id":"user","parentId":"start","timestamp":"2026-10-01T10:00:01Z","type":"user.message","data":{"content":"Copilot CLI importer question"}}),
    ])).unwrap();
    let sources = discover_history_files(Provider::Copilot, &[f.dir.path().into()], true)
        .unwrap()
        .sources;
    assert_eq!(sources.len(), 2);
    let conversations: Vec<_> = sources
        .iter()
        .map(|s| {
            crate::silver::copilot::parse_source(s)
                .unwrap()
                .conversation
                .unwrap()
        })
        .collect();
    assert_ne!(conversations[0].id, conversations[1].id);
    assert_ne!(
        conversations[0].messages.inline()[0].id,
        conversations[1].messages.inline()[0].id
    );
    assert!(sources.iter().any(|s| s.identity.path == cli));
    assert!(sources.iter().any(|s| s.identity.path == f.db));
}
