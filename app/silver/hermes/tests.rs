use super::*;
use crate::silver::{IndexOptions, index, source_stamp_for};
use crate::storage::Storage;

// Columns consumed from upstream hermes_state_common.SCHEMA_SQL; extra native
// columns are intentionally irrelevant to this adapter's read-only queries.
const SCHEMA: &str = "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
CREATE TABLE sessions(id TEXT PRIMARY KEY,title TEXT,cwd TEXT,git_repo_root TEXT,started_at REAL,ended_at REAL,last_activity_at REAL);
CREATE TABLE messages(id INTEGER PRIMARY KEY,session_id TEXT,role TEXT,content TEXT,tool_calls TEXT,timestamp REAL,
active INTEGER DEFAULT 1,compacted INTEGER DEFAULT 0,display_metadata TEXT,display_order INTEGER,tool_call_id TEXT,tool_name TEXT);
INSERT INTO sessions VALUES('alpha','Importing mutations','/work/cbioportal','/work/repository',1790726400,NULL,1790726401);
INSERT INTO sessions VALUES('beta','Other work',NULL,NULL,1790726400,NULL,NULL);
INSERT INTO messages(id,session_id,role,content,timestamp,display_order) VALUES(1,'alpha','user','Fix the cBioPortal importer',1790726400.25,1);
INSERT INTO messages(id,session_id,role,content,timestamp,display_order) VALUES(2,'alpha','assistant','I will repair mutation parsing.',1790726401,2);
INSERT INTO messages(id,session_id,role,content,timestamp,display_order) VALUES(3,'beta','user','independent history',1790726401,3);";

fn fixture(path: &Path) -> Connection {
    let connection = Connection::open(path).unwrap();
    connection.execute_batch(SCHEMA).unwrap();
    connection
}

fn options(path: &Path) -> IndexOptions {
    IndexOptions {
        providers: [(Provider::Hermes, vec![path.to_path_buf()])].into(),
        roots_are_authoritative: true,
        since_days: None,
    }
}

#[test]
fn native_display_history_is_read_only_and_excludes_compaction_copies_and_hidden_rows() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.db");
    let upstream = fixture(&path);
    upstream.execute_batch("UPDATE messages SET active=0,compacted=1 WHERE id=1;
        INSERT INTO messages(id,session_id,role,content,timestamp,display_order) VALUES(4,'alpha','user','Fix the cBioPortal importer',1790726400.25,1);
        INSERT INTO messages(id,session_id,role,content,timestamp,display_order,display_metadata) VALUES(5,'alpha','user','hidden model context',1790726402,5,'{\"model_only\":true}');
        INSERT INTO messages(id,session_id,role,content,timestamp,display_order,active) VALUES(6,'alpha','user','rewound evidence',1790726402,6,0);
        INSERT INTO messages(id,session_id,role,content,tool_calls,timestamp,display_order) VALUES(7,'alpha','assistant','', '[{\"function\":{\"name\":\"read_file\",\"arguments\":\"toolargumentmarker\"}}]',1790726402,7);
        INSERT INTO messages(id,session_id,role,content,timestamp,display_order) VALUES(8,'alpha','tool','tooloutputmarker',1790726403,8);").unwrap();
    let sources = discover(&path).unwrap();
    let source = &sources[0].identity;
    let before = source_stamp_for(source).unwrap();
    let parsed = parse(source).unwrap().conversation.unwrap();
    assert_eq!(parsed.messages.len(), 4);
    assert_eq!(
        parsed.messages.inline()[0].created_at,
        Some(1_790_726_400_250)
    );
    assert_eq!(parsed.project.as_deref(), Some("/work/cbioportal"));
    assert_eq!(parsed.repository.as_deref(), Some("/work/repository"));
    assert!(
        parsed.messages.inline()[2]
            .content
            .contains("toolargumentmarker")
    );
    assert_eq!(
        parsed.messages.inline()[2].dialogue_text.as_deref(),
        Some("")
    );
    assert_eq!(
        parsed.messages.inline()[3].dialogue_text.as_deref(),
        Some("")
    );
    assert!(
        parsed
            .messages
            .inline()
            .iter()
            .all(|m| !m.content.contains("hidden") && !m.content.contains("rewound"))
    );
    assert_eq!(source_stamp_for(source).unwrap(), before);
    assert!(
        open(&path)
            .unwrap()
            .execute("DELETE FROM sessions", [])
            .is_err()
    );
    // A later compaction representative retains the same normalized identity.
    let id = parsed.messages.inline()[0].id.clone();
    upstream.execute_batch("UPDATE messages SET active=0,compacted=1 WHERE id=4;
        INSERT INTO messages(id,session_id,role,content,timestamp,display_order) VALUES(9,'alpha','user','Fix the cBioPortal importer',1790726400.25,1);").unwrap();
    assert_eq!(
        parse(source)
            .unwrap()
            .conversation
            .unwrap()
            .messages
            .inline()[0]
            .id,
        id
    );
}

#[test]
fn wal_updates_invalidate_checkpoints_and_session_deletions_reconcile_independently() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.db");
    let upstream = fixture(&path);
    let local = directory.path().join("index.sqlite3");
    let sources = discover(&path).unwrap();
    let first_stamp = source_stamp_for(&sources[0].identity).unwrap();
    let main_before = std::fs::read(&path).unwrap();
    let mut writer = Storage::open_writer(&local).unwrap();
    index(&mut writer, &options(&path)).unwrap();
    writer.commit_writer().unwrap();
    drop(writer);
    let mut writer = Storage::open_writer(&local).unwrap();
    assert_eq!(
        index(&mut writer, &options(&path))
            .unwrap()
            .checkpoint_skipped_sources,
        2
    );
    writer.commit_writer().unwrap();
    drop(writer);
    upstream
        .execute_batch("UPDATE messages SET content='WAL-only updated evidence' WHERE id=3;")
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), main_before);
    assert_ne!(source_stamp_for(&sources[0].identity).unwrap(), first_stamp);
    assert!(first_stamp.verify(&sources[0].identity).is_err());
    let mut writer = Storage::open_writer(&local).unwrap();
    let changed = index(&mut writer, &options(&path)).unwrap();
    assert_eq!(changed.changed_messages, 1);
    writer.commit_writer().unwrap();
    assert_eq!(
        writer
            .search("updated", 10, Some(Provider::Hermes), None)
            .unwrap()
            .len(),
        1
    );
    drop(writer);
    upstream
        .execute_batch(
            "DELETE FROM messages WHERE session_id='beta'; DELETE FROM sessions WHERE id='beta';",
        )
        .unwrap();
    let mut writer = Storage::open_writer(&local).unwrap();
    let removed = index(&mut writer, &options(&path)).unwrap();
    assert_eq!(removed.purged_conversations, 1);
    writer.commit_writer().unwrap();
    assert_eq!(writer.counts().unwrap().conversations, 1);
    assert_eq!(
        writer.search("cbioportal", 10, None, None).unwrap().len(),
        1
    );
}

#[test]
fn structured_content_is_projected_and_malformed_content_fails_without_replacing_history() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.db");
    let upstream = fixture(&path);
    let structured = concat!(
        "\0json:",
        r#"[{"type":"text","text":"structured useful request"},{"type":"thinking","text":"context reasoning"}]"#
    );
    upstream
        .execute("UPDATE messages SET content=?1 WHERE id=1", [structured])
        .unwrap();
    let local = directory.path().join("index.sqlite3");
    let mut writer = Storage::open_writer(&local).unwrap();
    index(&mut writer, &options(&path)).unwrap();
    writer.commit_writer().unwrap();
    assert_eq!(
        writer.search("structured", 10, None, None).unwrap().len(),
        1
    );
    assert!(
        writer
            .search("reasoning", 10, None, None)
            .unwrap()
            .is_empty()
    );
    drop(writer);
    upstream
        .execute(
            "UPDATE messages SET content=?1 WHERE id=1",
            ["\0json:{broken"],
        )
        .unwrap();
    let mut writer = Storage::open_writer(&local).unwrap();
    assert!(index(&mut writer, &options(&path)).is_err());
    writer.commit_writer().unwrap();
    assert_eq!(
        writer.search("structured", 10, None, None).unwrap().len(),
        1
    );
}

#[test]
fn database_horizon_uses_session_activity_and_progress_counts_session_payloads() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.db");
    let upstream = fixture(&path);
    upstream.execute_batch("UPDATE sessions SET started_at=1,last_activity_at=1 WHERE id='beta'; UPDATE messages SET timestamp=1 WHERE session_id='beta';").unwrap();
    let local = directory.path().join("index.sqlite3");
    let mut options = options(&path);
    options.since_days = Some(90);
    let scans = crate::silver::direct_scans(&options).unwrap();
    let cutoff = scans[0].cutoff.unwrap();
    let expected_bytes: u64 = scans[0]
        .sources
        .iter()
        .filter(|s| s.activity_ns.unwrap() >= cutoff)
        .map(|s| s.estimated_bytes.unwrap())
        .sum();
    let mut writer = Storage::open_writer(&local).unwrap();
    let result = index(&mut writer, &options).unwrap();
    writer.commit_writer().unwrap();
    assert_eq!(writer.counts().unwrap().conversations, 1);
    assert_eq!(result.discovered_bytes, expected_bytes);
    assert_eq!(result.processed_bytes, expected_bytes);
    assert!(
        writer
            .search("independent", 10, None, None)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn compaction_without_a_backfilled_display_index_does_not_duplicate_evidence() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.db");
    let upstream = fixture(&path);
    upstream.execute_batch("UPDATE messages SET display_order=NULL; UPDATE messages SET active=0,compacted=1 WHERE id=1;
        INSERT INTO messages(id,session_id,role,content,timestamp) VALUES(4,'alpha','user','Fix the cBioPortal importer',1790726400.25);").unwrap();
    let sources = discover(&path).unwrap();
    let parsed = parse(&sources[0].identity).unwrap().conversation.unwrap();
    assert_eq!(parsed.messages.len(), 2);
    assert_eq!(
        parsed.messages.inline()[0].content,
        "Fix the cBioPortal importer"
    );
}

#[test]
fn an_empty_provider_database_is_still_protected_as_an_input_source() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.db");
    let upstream = fixture(&path);
    upstream
        .execute_batch("DELETE FROM messages; DELETE FROM sessions;")
        .unwrap();
    assert!(discover(&path).unwrap().is_empty());
    let error = crate::silver::validate_output_path(&path, &options(&path)).unwrap_err();
    assert_eq!(error.error.kind, crate::ErrorKind::Configuration);
}

#[tokio::test]
#[ignore = "requires MASH_TEST_MODELS_DIR containing an explicit model installation"]
async fn core_indexes_and_searches_native_database_sessions_with_real_models() {
    use crate::core::{IndexRequest, Operation, Paths, Request, Response};
    let directory = tempfile::tempdir().unwrap();
    let history = directory.path().join("state.db");
    let _upstream = fixture(&history);
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        toml::to_string(&toml::toml! {
            version=1
            providers={hermes={roots=[(history.to_str().unwrap())]}}
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
    let source_bytes = std::fs::read(&history).unwrap();
    let error = crate::core::execute(Request {
        paths: Paths {
            db: Some(history.clone()),
            ..paths.clone()
        },
        operation: Operation::Index(IndexRequest::default()),
    })
    .await
    .expect_err("provider database must not become the index output");
    assert_eq!(error.error.kind, crate::ErrorKind::Configuration);
    assert_eq!(std::fs::read(&history).unwrap(), source_bytes);
    crate::core::execute(Request {
        paths: paths.clone(),
        operation: Operation::Index(IndexRequest::default()),
    })
    .await
    .unwrap();
    let Response::Search(response) = crate::core::execute(Request {
        paths: paths.clone(),
        operation: Operation::Search {
            fusion: crate::gold::config::FusionOverrides::default(),
            diagnostics: false,
            query: "cbioportal importer".into(),
            intent: None,
            limit: Some(5),
            unit: Some(crate::search_results::SearchUnit::Threads),
            filters: Box::new(crate::search_filter::Selection {
                provider: vec![Provider::Hermes],
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
    let crate::search_results::Results::Threads(threads) = response.results else {
        panic!("thread results")
    };
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].provider, Provider::Hermes);
    assert_eq!(
        threads[0].metadata.project.as_deref(),
        Some("/work/cbioportal")
    );
    let Response::View(view) = crate::core::execute(Request {
        paths,
        operation: Operation::View {
            id: threads[0].matches[0].message_id.clone(),
            context: 5,
            shared: None,
        },
    })
    .await
    .unwrap() else {
        panic!("view response")
    };
    assert_eq!(view.messages.len(), 2);
}
