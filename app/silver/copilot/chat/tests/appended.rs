//! Current VS Code sessions: an empty initial state followed by appended
//! requests, streamed response parts and result metadata.
use super::*;

fn empty_session() -> Value {
    json!({"version":3,"sessionId":"chat-session","creationDate":1000,"initialLocation":"panel",
        "responderUsername":"","hasPendingEdits":false,"requests":[],"pendingRequests":[],
        "inputState":{"inputText":"","selections":[],"attachments":[],"mode":{"id":"agent"}}})
}

fn turn(id: &str, text: &str, at: i64) -> Value {
    json!({"requestId":id,"timestamp":at,
        "message":{"text":text,"parts":[{"kind":"text","text":text,"range":{"start":0,"endExclusive":1},"editorRange":{}}]},
        "agent":{"extensionId":{"value":"GitHub.copilot-chat","_lower":"github.copilot-chat"},
            "extensionDisplayName":"GitHub Copilot Chat","extensionPublisherId":"GitHub",
            "extensionVersion":"0.40.0","description":"","disambiguation":[]},
        "modelId":"copilot/auto","responseId":format!("response-{id}"),"modelState":{"value":0},
        "contentReferences":[],"codeCitations":[],"timeSpentWaiting":0,"modeInfo":{"kind":"agent"},
        "variableData":{"variables":[]},"response":[]})
}

/// One request as VS Code appends it while the response streams.
fn exchange(index: usize, id: &str, question: &str, answer: &str, at: i64) -> Vec<Value> {
    vec![
        json!({"kind":1,"k":["inputState","inputText"],"v":"privateunsentdraft"}),
        json!({"kind":2,"k":["requests"],"v":[turn(id, question, at)]}),
        json!({"kind":1,"k":["inputState","inputText"],"v":""}),
        json!({"kind":2,"k":["requests",index,"response"],"v":[{"kind":"thinking","value":"privatereasoning"}]}),
        json!({"kind":2,"k":["requests",index,"response"],"i":1,"v":[{"kind":"toolInvocationSerialized",
            "invocationMessage":{"value":"Reading lock.go","isTrusted":false,"supportThemeIcons":false,"supportHtml":false,"supportAlertSyntax":false},
            "toolId":"copilot_readFile","toolCallId":"call-1","isComplete":true}]}),
        json!({"kind":2,"k":["requests",index,"response"],"i":2,"v":[{"value":answer,"supportThemeIcons":false,"supportHtml":false}]}),
        json!({"kind":1,"k":["requests",index,"result"],"v":{"timings":{"totalElapsed":10},"details":"Auto","metadata":{}}}),
        json!({"kind":1,"k":["requests",index,"modelState"],"v":{"value":1,"completedAt":at + 500}}),
        json!({"kind":1,"k":["requests",index,"completionTokens"],"v":12}),
        json!({"kind":1,"k":["requests",index,"followups"],"v":[]}),
        json!({"kind":1,"k":["responderUsername"],"v":"GitHub Copilot"}),
    ]
}

impl Fixture {
    fn remote(folder: &str) -> Self {
        let f = Self::new(false);
        std::fs::write(
            f.db.parent().unwrap().join("workspace.json"),
            json!({ "folder": folder }).to_string(),
        )
        .unwrap();
        f.catalog(
            &json!({"chat-session":{"sessionId":"chat-session","title":"New Chat",
            "lastMessageDate":2000,"timing":{"created":1000},"initialLocation":"panel",
            "hasPendingEdits":false,"isEmpty":false,"isExternal":false,"lastResponseState":1,
            "permissionLevel":"default"}}),
        );
        f
    }

    fn append(&self, text: &str) {
        std::fs::OpenOptions::new()
            .append(true)
            .open(self.chats.join("chat-session.jsonl"))
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }

    fn reindex(&self, database: &Path) -> crate::silver::IndexSummary {
        let mut storage = Storage::open_writer(database).unwrap();
        let summary = index(&mut storage, &self.options()).unwrap();
        storage.commit_writer().unwrap();
        summary
    }
}

/// Indexed rows in display order: identity, role, timestamp and content.
fn stored(database: &Path) -> Vec<(String, String, Option<i64>, String)> {
    crate::storage::test_support::connect(database)
        .prepare("SELECT id, role, created_at, content FROM messages ORDER BY ordinal")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn remote_window_sessions_keep_remote_folders_roles_order_and_completion_times() {
    for (folder, project) in [
        (
            "vscode-remote://ssh-remote%2Bbuild-host/home/ada/scratch/lockfile",
            "/home/ada/scratch/lockfile",
        ),
        (
            "vscode-remote://codespaces%2Bfriendly-space-tribble-1234/workspaces/lockfile",
            "/workspaces/lockfile",
        ),
    ] {
        let f = Fixture::remote(folder);
        let mut operations = vec![json!({"kind":0,"v":empty_session()})];
        operations.extend(exchange(
            0,
            "request-1",
            "hello ?",
            "Hello! How can I help?",
            1100,
        ));
        operations.push(json!({"kind":1,"k":["customTitle"],"v":"Greeting the assistant"}));
        f.write("jsonl", &log(&operations));
        let chat = f.parse().unwrap();
        assert_eq!(chat.project.as_deref(), Some(project), "{folder}");
        assert_eq!(chat.title.as_deref(), Some("Greeting the assistant"));
        assert_eq!(chat.created_at, Some(1000));
        assert_eq!(chat.updated_at, Some(2000));
        let rows = chat.messages.inline();
        assert_eq!(
            rows.iter()
                .map(|r| (r.role.clone(), r.created_at))
                .collect::<Vec<_>>(),
            [(Role::User, Some(1100)), (Role::Assistant, Some(1600))]
        );
        assert_eq!(rows[0].dialogue_text.as_deref(), Some("hello ?"));
        assert_eq!(
            rows[1].dialogue_text.as_deref(),
            Some("Hello! How can I help?")
        );
        assert!(rows[1].content.contains("privatereasoning"));
        assert!(rows[1].content.contains("Reading lock.go"));
        assert!(
            !rows
                .iter()
                .any(|r| r.content.contains("privateunsentdraft"))
        );
    }
}

#[test]
fn appended_sessions_reindex_without_duplicating_or_reordering_messages() {
    let f = Fixture::remote("file:///work/lockfile");
    let database = f.dir.path().join("index.sqlite3");
    let mut operations = vec![json!({"kind":0,"v":empty_session()})];
    operations.extend(exchange(
        0,
        "request-1",
        "Why does the lock leak?",
        "Unlock on every return.",
        1100,
    ));
    f.write("jsonl", &log(&operations));
    f.reindex(&database);
    let before = stored(&database);
    assert_eq!(before.len(), 2);

    f.append(&log(&exchange(
        1,
        "request-2",
        "Show the deferred unlock",
        "Use defer.",
        3000,
    )));
    f.catalog(&json!({"chat-session":{"sessionId":"chat-session","title":"New Chat","lastMessageDate":3000}}));
    f.reindex(&database);
    let after = stored(&database);
    assert_eq!(after.len(), 4);
    assert_eq!(
        after[..2],
        before[..],
        "existing messages keep identity and order"
    );
    assert_eq!(
        after[2..]
            .iter()
            .map(|r| (r.1.as_str(), r.2, r.3.as_str()))
            .collect::<Vec<_>>(),
        [
            ("user", Some(3000), "Show the deferred unlock"),
            (
                "assistant",
                Some(3500),
                "privatereasoning\nReading lock.go\nUse defer."
            ),
        ]
    );
    let unchanged = f.reindex(&database);
    assert_eq!(unchanged.changed_messages, 0);
    assert_eq!(stored(&database), after);
}

#[test]
fn torn_or_damaged_lines_keep_the_previous_contribution_until_readable() {
    let f = Fixture::remote("file:///work/lockfile");
    let database = f.dir.path().join("index.sqlite3");
    let mut operations = vec![json!({"kind":0,"v":empty_session()})];
    operations.extend(exchange(
        0,
        "request-1",
        "Why does the lock leak?",
        "Unlock on every return.",
        1100,
    ));
    f.write("jsonl", &log(&operations));
    f.reindex(&database);
    let indexed = stored(&database);

    // A request still being written, then the same damage between records.
    let next = exchange(
        1,
        "request-2",
        "Show the deferred unlock",
        "Use defer.",
        3000,
    );
    let torn = next[1].to_string();
    f.append(&torn[..torn.len() / 2]);
    for damaged in [false, true] {
        if damaged {
            f.append(&format!("\n{}", log(&next)));
        }
        let summary = f.reindex(&database);
        assert_eq!(
            (summary.rejected_sources, summary.malformed_records),
            (1, 1)
        );
        let issue = &summary.record_diagnostics.samples[0];
        assert_eq!(issue.line, operations.len() + 1);
        assert!(issue.source.ends_with("chat-session.jsonl"));
        assert_eq!(stored(&database), indexed, "previous messages retained");
    }

    let mut repaired = operations;
    repaired.extend(next);
    f.write("jsonl", &log(&repaired));
    let summary = f.reindex(&database);
    assert_eq!(summary.rejected_sources, 0);
    let rows = stored(&database);
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[..2], indexed[..]);
}

#[test]
fn a_session_whose_first_line_is_still_being_written_is_not_an_error() {
    let f = Fixture::remote("file:///work/lockfile");
    f.write("jsonl", r#"{"kind":0,"v":{"version":3,"sessio"#);
    let parsed = parse(&f.sources().unwrap().remove(0)).unwrap();
    assert!(parsed.chat.is_none());
    assert_eq!(parsed.malformed_records, 1);
    assert_eq!(parsed.first_record_issue.unwrap().line, 1);
}
