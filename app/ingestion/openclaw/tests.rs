use super::*;
use rusqlite::Connection;

const SCHEMA:&str="PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
CREATE TABLE session_nodes(session_key TEXT PRIMARY KEY,label TEXT);
CREATE TABLE session_windows(session_id TEXT PRIMARY KEY,session_key TEXT,display_name TEXT,created_at INTEGER,updated_at INTEGER,transcript_updated_at INTEGER);
CREATE TABLE transcript_events(session_id TEXT,seq INTEGER,event_json TEXT,event_zstd BLOB,event_utf8_bytes INTEGER,created_at INTEGER,PRIMARY KEY(session_id,seq));
CREATE TABLE session_transcript_cold_archives(session_id TEXT PRIMARY KEY,generation TEXT,archive_name TEXT,archive_sha256 TEXT,storage TEXT,archive_blob BLOB,archive_bytes INTEGER,raw_bytes INTEGER,event_count INTEGER,last_seq INTEGER);
INSERT INTO session_nodes VALUES('key','Mutation importer');
INSERT INTO session_windows VALUES('alpha','key',NULL,1790726400000,1790726401000,NULL);";

fn fixture(path: &Path) -> Connection {
    let connection = Connection::open(path).unwrap();
    connection.execute_batch(SCHEMA).unwrap();
    append(
        &connection,
        0,
        r#"{"type":"session","id":"alpha","cwd":"/work/cbioportal","timestamp":"2026-09-30T00:00:00Z"}"#,
    );
    append(
        &connection,
        1,
        r#"{"type":"message","id":"user","parentId":null,"message":{"role":"user","content":"Fix the cBioPortal importer"}}"#,
    );
    append(
        &connection,
        2,
        r#"{"type":"message","id":"assistant","parentId":"user","message":{"role":"assistant","content":[{"type":"text","text":"Repair mutation parsing"},{"type":"thinking","thinking":"reasoningmarker"},{"type":"toolCall","name":"read_file","arguments":{"text":"toolargumentmarker"}}]}}"#,
    );
    connection
}
fn append(connection: &Connection, seq: i64, json: &str) {
    connection.execute("INSERT INTO transcript_events(session_id,seq,event_json,created_at) VALUES('alpha',?1,?2,1790726401000)",rusqlite::params![seq,json]).unwrap();
}

#[test]
fn native_hot_and_compressed_events_preserve_prose_and_canonical_context() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("openclaw-agent.sqlite");
    let upstream = fixture(&path);
    append(
        &upstream,
        3,
        r#"{"type":"message","id":"tool","parentId":"assistant","message":{"role":"toolResult","content":[{"type":"text","text":"tooloutputmarker"}]}}"#,
    );
    let event: String = upstream
        .query_row(
            "SELECT event_json FROM transcript_events WHERE seq=2",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let compressed = zstd::stream::encode_all(event.as_bytes(), 1).unwrap();
    upstream.execute("UPDATE transcript_events SET event_json=NULL,event_zstd=?1,event_utf8_bytes=?2 WHERE seq=2",rusqlite::params![compressed,i64::try_from(event.len()).unwrap()]).unwrap();
    let sources = discover(&path).unwrap();
    let source = &sources[0].identity;
    let stamp = sources[0].stamp().unwrap();
    let conversation = parse(source).unwrap().conversation.unwrap();
    assert_eq!(conversation.messages.len(), 3);
    assert_eq!(conversation.project.as_deref(), Some("/work/cbioportal"));
    assert_eq!(conversation.title.as_deref(), Some("Mutation importer"));
    assert!(
        conversation.messages.inline()[1]
            .content
            .contains("reasoningmarker")
    );
    assert!(
        conversation.messages.inline()[1]
            .content
            .contains("toolargumentmarker")
    );
    assert_eq!(
        conversation.messages.inline()[1]
            .search_projection
            .as_deref(),
        Some("Repair mutation parsing")
    );
    assert_eq!(
        conversation.messages.inline()[2]
            .search_projection
            .as_deref(),
        Some("")
    );
    stamp.verify(source).unwrap();
    assert!(
        sqlite_source::open(&path)
            .unwrap()
            .execute("DELETE FROM transcript_events", [])
            .is_err()
    );
}

#[test]
fn branches_side_appends_and_reset_windows_select_visible_history() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("openclaw-agent.sqlite");
    let upstream = fixture(&path);
    append(
        &upstream,
        3,
        r#"{"type":"message","id":"discarded","parentId":"user","appendMode":"side","message":{"role":"assistant","content":"discardedmarker"}}"#,
    );
    append(
        &upstream,
        4,
        r#"{"type":"leaf","id":"navigation","parentId":"discarded","targetId":"assistant"}"#,
    );
    append(
        &upstream,
        5,
        r#"{"type":"message","id":"continued","parentId":"navigation","message":{"role":"user","content":"continued request"}}"#,
    );
    let sources = discover(&path).unwrap();
    let conversation = parse(&sources[0].identity).unwrap().conversation.unwrap();
    assert_eq!(conversation.messages.len(), 3);
    assert!(
        conversation
            .messages
            .inline()
            .iter()
            .all(|m| !m.content.contains("discardedmarker"))
    );
    append(
        &upstream,
        6,
        r#"{"type":"reset","id":"reset","parentId":"continued","firstKeptEntryId":"assistant","reason":"new"}"#,
    );
    append(
        &upstream,
        7,
        r#"{"type":"compaction","id":"compaction","parentId":"reset","summary":"compactionmarker","firstKeptEntryId":"reset","tokensBefore":5000}"#,
    );
    append(
        &upstream,
        8,
        r#"{"type":"message","id":"new","parentId":"compaction","message":{"role":"user","content":"new request"}}"#,
    );
    let conversation = parse(&sources[0].identity).unwrap().conversation.unwrap();
    assert_eq!(conversation.messages.len(), 4);
    assert!(
        conversation
            .messages
            .inline()
            .iter()
            .all(|m| !m.content.contains("Fix the cBioPortal"))
    );
    assert_eq!(
        conversation.messages.inline()[2]
            .search_projection
            .as_deref(),
        Some("")
    );
    assert_eq!(conversation.messages.inline()[3].content, "new request");
}

fn archive_bytes(connection: &Connection) -> (Vec<u8>, usize, i64) {
    #[derive(serde::Serialize)]
    struct Row {
        seq: i64,
        event_json: String,
        created_at: i64,
    }
    #[derive(serde::Serialize)]
    struct Record {
        kind: &'static str,
        row: Row,
    }
    let mut records = String::from(
        "{\"kind\":\"header\",\"version\":1,\"sessionId\":\"alpha\",\"generation\":\"g1\"}\n",
    );
    let mut statement = connection
        .prepare("SELECT seq,event_json,created_at FROM transcript_events ORDER BY seq")
        .unwrap();
    let events = statement
        .query_map([], |row| {
            Ok(Row {
                seq: row.get("seq")?,
                event_json: row.get("event_json")?,
                created_at: row.get("created_at")?,
            })
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let last = events.last().unwrap().seq;
    for row in events {
        records.push_str(&serde_json::to_string(&Record { kind: "event", row }).unwrap());
        records.push('\n');
    }
    let size = records.len();
    (
        zstd::stream::encode_all(records.as_bytes(), 1).unwrap(),
        size,
        last,
    )
}

fn make_cold(connection: &Connection, database: &Path, file: bool) -> Option<std::path::PathBuf> {
    use sha2::{Digest, Sha256};
    let (bytes, raw, last) = archive_bytes(connection);
    let hash = hex::encode(Sha256::digest(&bytes));
    let name = format!("{hash}.jsonl.zst");
    let count: i64 = connection
        .query_row("SELECT count(*) FROM transcript_events", [], |row| {
            row.get(0)
        })
        .unwrap();
    let path = if file {
        let directory = cold_directory(database);
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(&name);
        std::fs::write(&path, &bytes).unwrap();
        Some(path)
    } else {
        None
    };
    connection.execute("INSERT INTO session_transcript_cold_archives VALUES('alpha','g1',?1,?2,?3,?4,?5,?6,?7,?8)",
        rusqlite::params![name,hash,if file {"file"} else {"sqlite"},if file {None} else {Some(&bytes)},i64::try_from(bytes.len()).unwrap(),i64::try_from(raw).unwrap(),count,last]).unwrap();
    connection
        .execute("DELETE FROM transcript_events", [])
        .unwrap();
    path
}

#[test]
fn verified_cold_archives_read_from_sqlite_or_files_without_restoring_sources() {
    for file in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let agent = directory.path().join("agent");
        std::fs::create_dir(&agent).unwrap();
        let database = agent.join("openclaw-agent.sqlite");
        let upstream = fixture(&database);
        let original = discover(&database).unwrap();
        let expected = parse(&original[0].identity).unwrap().conversation.unwrap();
        let archive = make_cold(&upstream, &database, file);
        let sources = discover(&database).unwrap();
        assert_eq!(sources[0].dependencies.len(), usize::from(file));
        let before = sources[0].stamp().unwrap();
        let actual = parse(&sources[0].identity).unwrap().conversation.unwrap();
        assert_eq!(actual.messages.len(), expected.messages.len());
        assert_eq!(
            actual.messages.inline()[0].id,
            expected.messages.inline()[0].id
        );
        assert_eq!(
            actual.messages.inline()[1].content,
            expected.messages.inline()[1].content
        );
        assert_eq!(
            upstream
                .query_row("SELECT count(*) FROM transcript_events", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        before.verify(&sources[0].identity).unwrap();
        if let Some(path) = archive {
            std::fs::write(&path, b"corrupt archive").unwrap();
            assert!(before.verify(&sources[0].identity).is_err());
            assert!(parse(&sources[0].identity).is_err());
            std::fs::remove_file(path).unwrap();
            assert!(sources[0].stamp().is_err());
        } else {
            upstream
                .execute(
                    "UPDATE session_transcript_cold_archives SET archive_sha256=?1",
                    ["0".repeat(64)],
                )
                .unwrap();
            assert!(parse(&sources[0].identity).is_err());
        }
    }
}

#[test]
fn wal_refresh_and_malformed_events_do_not_destroy_previous_searchable_history() {
    use crate::ingestion::{IndexOptions, index};
    use crate::storage::Storage;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("openclaw-agent.sqlite");
    let upstream = fixture(&path);
    let options = IndexOptions {
        providers: [(Provider::Openclaw, vec![path.clone()])].into(),
        roots_are_authoritative: true,
        since_days: None,
    };
    let local = directory.path().join("index.sqlite3");
    let mut writer = Storage::open_writer(&local).unwrap();
    index(&mut writer, &options).unwrap();
    writer.commit_writer().unwrap();
    drop(writer);
    let mut writer = Storage::open_writer(&local).unwrap();
    assert_eq!(
        index(&mut writer, &options)
            .unwrap()
            .checkpoint_skipped_sources,
        1
    );
    writer.commit_writer().unwrap();
    drop(writer);
    let main = std::fs::read(&path).unwrap();
    append(
        &upstream,
        3,
        r#"{"type":"message","id":"new","parentId":"assistant","message":{"role":"user","content":"updated WAL evidence"}}"#,
    );
    assert_eq!(std::fs::read(&path).unwrap(), main);
    let mut writer = Storage::open_writer(&local).unwrap();
    assert_eq!(index(&mut writer, &options).unwrap().changed_messages, 1);
    writer.commit_writer().unwrap();
    drop(writer);
    upstream
        .execute(
            "UPDATE transcript_events SET event_json='invalid-json' WHERE seq=3",
            [],
        )
        .unwrap();
    let mut writer = Storage::open_writer(&local).unwrap();
    assert!(index(&mut writer, &options).is_err());
    drop(writer);
    let writer = Storage::open_writer(&local).unwrap();
    assert_eq!(
        writer
            .search("updated", 10, Some(Provider::Openclaw), None)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn compressed_size_limits_and_invalid_navigation_fail_closed() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("openclaw-agent.sqlite");
    let upstream = fixture(&database);
    let source = discover(&database).unwrap().remove(0).identity;
    let compressed =
        zstd::stream::encode_all(b"oversized compared to its claim".as_slice(), 1).unwrap();
    upstream.execute("UPDATE transcript_events SET event_json=NULL,event_zstd=?1,event_utf8_bytes=1 WHERE seq=2",[compressed]).unwrap();
    assert!(parse(&source).is_err());
    upstream
        .execute(
            "UPDATE transcript_events SET event_utf8_bytes=4194305 WHERE seq=2",
            [],
        )
        .unwrap();
    assert!(parse(&source).is_err());
    upstream
        .execute("DELETE FROM transcript_events WHERE seq=2", [])
        .unwrap();
    append(
        &upstream,
        2,
        r#"{"type":"leaf","id":"control","parentId":"user","targetId":"missing"}"#,
    );
    assert!(parse(&source).is_err());
    upstream
        .execute("DELETE FROM transcript_events WHERE seq=2", [])
        .unwrap();
    append(
        &upstream,
        2,
        r#"{"type":"message","id":"user","parentId":"user","message":{"role":"user","content":"duplicate"}}"#,
    );
    assert!(parse(&source).is_err());
}

#[tokio::test]
#[ignore = "requires MASH_TEST_MODELS_DIR with installed local models"]
async fn core_indexes_searches_and_views_native_openclaw_threads() {
    use crate::core::{IndexRequest, Operation, Paths, Request, Response};
    let directory = tempfile::tempdir().unwrap();
    let history = directory.path().join("openclaw-agent.sqlite");
    let _upstream = fixture(&history);
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        toml::to_string(&toml::toml! {
            version=1
            providers={openclaw={roots=[(history.to_str().unwrap())]}}
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
    let targets = crate::core::watch_targets(&Request {
        paths: paths.clone(),
        operation: Operation::Index(IndexRequest::default()),
    })
    .unwrap();
    assert!(targets.roots.contains(&cold_directory(&history)));
    crate::core::execute(Request {
        paths: paths.clone(),
        operation: Operation::Index(IndexRequest::default()),
    })
    .await
    .unwrap();
    let Response::Search(response) = crate::core::execute(Request {
        paths: paths.clone(),
        operation: Operation::Search {
            query: "cbioportal importer".into(),
            limit: 5,
            unit: crate::search_results::SearchUnit::Threads,
            filters: Box::new(crate::search_filter::Selection {
                provider: vec![Provider::Openclaw],
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
        panic!("threads")
    };
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].provider, Provider::Openclaw);
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
    assert!(
        view.messages
            .iter()
            .any(|m| m.content.contains("toolargumentmarker"))
    );
}

#[test]
fn session_horizons_progress_and_deletion_use_logical_windows() {
    use crate::ingestion::{IndexOptions, index};
    use crate::storage::Storage;
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("openclaw-agent.sqlite");
    let upstream = fixture(&database);
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    upstream
        .execute(
            "UPDATE session_windows SET transcript_updated_at=?1 WHERE session_id='alpha'",
            [now],
        )
        .unwrap();
    upstream.execute_batch("INSERT INTO session_nodes VALUES('other',NULL); INSERT INTO session_windows VALUES('beta','other',NULL,1000,1000,NULL);
        INSERT INTO transcript_events(session_id,seq,event_json,created_at) VALUES('beta',0,'{\"type\":\"session\",\"id\":\"beta\",\"cwd\":\"/old\"}',1000);
        INSERT INTO transcript_events(session_id,seq,event_json,created_at) VALUES('beta',1,'{\"type\":\"message\",\"id\":\"user\",\"parentId\":null,\"message\":{\"role\":\"user\",\"content\":\"old unrelated prompt\"}}',1000);").unwrap();
    let sources = discover(&database).unwrap();
    let options = IndexOptions {
        providers: [(Provider::Openclaw, vec![database])].into(),
        roots_are_authoritative: true,
        since_days: Some(1),
    };
    let local = directory.path().join("index.sqlite3");
    let mut writer = Storage::open_writer(&local).unwrap();
    let summary = index(&mut writer, &options).unwrap();
    writer.commit_writer().unwrap();
    assert_eq!(summary.processed_files, 1);
    assert_eq!(summary.processed_bytes, sources[0].estimated_bytes.unwrap());
    assert_eq!(writer.counts().unwrap().conversations, 1);
    drop(writer);
    upstream.execute_batch("DELETE FROM session_windows WHERE session_id='alpha'; DELETE FROM transcript_events WHERE session_id='alpha';").unwrap();
    let mut writer = Storage::open_writer(&local).unwrap();
    assert_eq!(
        index(&mut writer, &options).unwrap().purged_conversations,
        1
    );
    writer.commit_writer().unwrap();
    assert_eq!(writer.counts().unwrap().conversations, 0);
}
