use super::*;

fn fixture() -> rusqlite::Connection {
    let connection = rusqlite::Connection::open_in_memory().unwrap();
    crate::storage::packed::register(&connection).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE claude_sources (
                source_key TEXT PRIMARY KEY, source_path TEXT, chat_id TEXT,
                title TEXT, project TEXT, repository TEXT);
             CREATE TABLE claude_source_messages (
                source_key TEXT, message_id TEXT, parent_id TEXT, role TEXT,
                content_packed, content TEXT GENERATED ALWAYS AS (silver_text(content_packed)) VIRTUAL,
                projection TEXT, created_at INTEGER, attachments TEXT NOT NULL DEFAULT '[]',
                metadata TEXT);
             INSERT INTO claude_sources VALUES
                ('a','/a','thread','title a','project a','repository a'),
                ('b','/b','thread','title b','project b','repository b');
             INSERT INTO claude_source_messages(source_key,message_id,parent_id,role,content_packed,projection,created_at) VALUES
                ('a','root',NULL,'user','root text',NULL,10),
                ('a','left','root','assistant','left text','left projection',20),
                ('b','root',NULL,'user','root text',NULL,10),
                ('b','right','root','assistant','right text','',30);",
        )
        .unwrap();
    connection
}

#[test]
fn streamed_contributions_preserve_copies_branches_and_selected_sources() {
    let connection = fixture();
    let all = claude_chat(&connection, "thread", None).unwrap().unwrap();
    assert_eq!(all.messages.len(), 3);
    assert_eq!(all.messages.inline()[0].id, "root");
    for message in &all.messages.inline()[1..] {
        assert_eq!(
            message.context,
            crate::message_context::MessageContext::Parent { id: "root".into() }
        );
    }
    assert_eq!(all.created_at, Some(10));
    assert_eq!(all.updated_at, Some(30));
    assert_eq!(
        all.messages.inline()[1].dialogue_text.as_deref(),
        Some("left projection")
    );
    assert_eq!(all.messages.inline()[2].dialogue_text.as_deref(), Some(""));
    let selected = BTreeSet::from(["b".to_owned()]);
    let selected = claude_chat(&connection, "thread", Some(&selected))
        .unwrap()
        .unwrap();
    assert_eq!(
        selected
            .messages
            .inline()
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        ["root", "right"]
    );
    assert_eq!(selected.source.path, PathBuf::from("/b"));
    assert_eq!(selected.title.as_deref(), Some("title b"));
    assert_eq!(selected.project.as_deref(), Some("project b"));
    assert_eq!(selected.repository.as_deref(), Some("repository b"));
    assert!(
        claude_chat(&connection, "thread", Some(&BTreeSet::new()))
            .unwrap()
            .is_none()
    );
}

#[test]
fn streamed_contributions_reject_conflicts_and_late_decode_failures() {
    let connection = fixture();
    connection.execute("UPDATE claude_source_messages SET content_packed='conflict' WHERE source_key='b' AND message_id='root'", []).unwrap();
    assert!(claude_chat(&connection, "thread", None).is_err());
    // A conflicting copy outside the routed subset is not a canonical conflict.
    let selected = BTreeSet::from(["a".to_owned()]);
    assert_eq!(
        claude_chat(&connection, "thread", Some(&selected))
            .unwrap()
            .unwrap()
            .messages
            .len(),
        2
    );
    connection.execute("UPDATE claude_source_messages SET content_packed='root text' WHERE source_key='b' AND message_id='root'", []).unwrap();
    connection
        .execute(
            "INSERT INTO claude_source_messages(source_key,message_id,parent_id,role,content_packed,projection,created_at) VALUES ('b','z-late',NULL,'user',x'ff',NULL,40)",
            [],
        )
        .expect_err("a corrupt packed frame never reaches silver");
    connection
        .execute(
            "INSERT INTO claude_source_messages(source_key,message_id,parent_id,role,content_packed,projection,created_at) VALUES ('b','z-late',NULL,'user','late text',x'ff',40)",
            [],
        )
        .unwrap();
    // Never return a partial thread after earlier rows were successfully decoded.
    assert!(claude_chat(&connection, "thread", None).is_err());
    // Keep the prior typed-query contract: malformed rows are errors even when
    // their source would otherwise be excluded by the selected-source filter.
    assert!(claude_chat(&connection, "thread", Some(&selected)).is_err());
}

#[test]
fn late_decode_failure_rolls_back_source_replacement_and_allows_retry() {
    let directory = tempfile::tempdir().unwrap();
    let mut storage = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    let mut source = Chat {
        id: "thread".to_owned(),
        provider: Provider::ClaudeCode,
        source: SourceIdentity::file(directory.path().join("a.jsonl")),
        title_origin: None,
        timestamp_origins: None,
        claude_lineage: Some(claude::Lineage::from([
            ("root".to_owned(), None),
            ("z-late".to_owned(), Some("root".to_owned())),
        ])),
        title: None,
        project: None,
        repository: None,
        created_at: Some(10),
        updated_at: Some(20),
        messages: ["root", "z-late"]
            .into_iter()
            .enumerate()
            .map(|(ordinal, id)| NormalizedMessage {
                attachments: Vec::new(),
                id: id.to_owned(),
                ordinal: i64::try_from(ordinal).unwrap(),
                context: crate::message_context::MessageContext::Linear,
                role: crate::role::Role::User,
                content: format!("{id} text"),
                dialogue_text: None,
                created_at: Some(10 + i64::try_from(ordinal).unwrap() * 10),
                tools: crate::tools::Activity::default(),
            })
            .collect(),
    };
    storage.replace_chat(&source).unwrap();
    storage.commit_and_continue_writer().unwrap();
    storage
        .connection
        .execute(
            "UPDATE claude_source_messages SET projection=x'ff' WHERE message_id='z-late'",
            [],
        )
        .unwrap();
    source.source = SourceIdentity::file(directory.path().join("b.jsonl"));
    source.messages.inline_mut()[1].id = "branch".to_owned();
    source.messages.inline_mut()[1].content = "new branch text".to_owned();
    source.claude_lineage = Some(claude::Lineage::from([
        ("root".to_owned(), None),
        ("branch".to_owned(), Some("root".to_owned())),
    ]));
    let error = storage
        .replace_chat(&source)
        .err()
        .expect("corrupt contribution must fail reconciliation");
    assert_eq!(error.error.kind, crate::ErrorKind::Database);
    assert_eq!(storage.counts().unwrap().messages, 2);
    assert_eq!(storage.view("z-late", 0).unwrap()[0].content, "z-late text");
    assert!(storage.view("branch", 0).unwrap().is_empty());
    let sources: i64 = storage
        .connection
        .query_row("SELECT count(*) FROM claude_sources", [], |row| row.get(0))
        .unwrap();
    assert_eq!(sources, 1);
    // The savepoint unwinds only this replacement; repairing the bad persisted
    // contribution permits a clean retry within the same writer transaction.
    storage
        .connection
        .execute(
            "UPDATE claude_source_messages SET projection=NULL WHERE message_id='z-late'",
            [],
        )
        .unwrap();
    storage.replace_chat(&source).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().messages, 3);
    let sources: i64 = storage
        .connection
        .query_row("SELECT count(*) FROM claude_sources", [], |row| row.get(0))
        .unwrap();
    assert_eq!(sources, 2);
}

mod memory;
