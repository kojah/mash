use super::*;
use crate::silver::{IndexOptions, index, source_stamp_for};
use crate::storage::Storage;

mod enrichment;
mod memory;

// Required columns from upstream core/session/sql.ts and core/project/sql.ts.
const SCHEMA: &str = "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
CREATE TABLE project(id TEXT PRIMARY KEY,worktree TEXT,vcs TEXT);
CREATE TABLE session(id TEXT PRIMARY KEY,project_id TEXT,title TEXT,directory TEXT,time_created INTEGER,time_updated INTEGER);
CREATE TABLE message(id TEXT PRIMARY KEY,session_id TEXT,time_created INTEGER,data TEXT);
CREATE TABLE part(id TEXT PRIMARY KEY,message_id TEXT,session_id TEXT,data TEXT);
CREATE TABLE session_message(id TEXT PRIMARY KEY,session_id TEXT,type TEXT,seq INTEGER,time_created INTEGER,data TEXT);
INSERT INTO project VALUES('project','/work/repository','git');
INSERT INTO session VALUES('alpha','project','Mutation import','/work/cbioportal',1790726400000,1790726401000);
INSERT INTO session VALUES('beta','project','Earlier run','/work/cbioportal',1790726400000,1790726401000);
INSERT INTO session_message VALUES('user','alpha','user',1,1790726400000,'{\"text\":\"Fix the cBioPortal importer\"}');
INSERT INTO session_message VALUES('assistant','alpha','assistant',2,1790726401000,'{\"content\":[{\"type\":\"text\",\"text\":\"Repair mutation parsing\"},{\"type\":\"reasoning\",\"text\":\"reasoningmarker\"},{\"type\":\"tool\",\"name\":\"read\",\"state\":{\"output\":\"toolmarker\"}}]}');
INSERT INTO message VALUES('legacy','beta',1790726400000,'{\"role\":\"user\"}');
INSERT INTO part VALUES('part1','legacy','beta','{\"type\":\"text\",\"text\":\"independent native history\"}');";

fn fixture(path: &Path) -> Connection {
    let connection = Connection::open(path).unwrap();
    connection.execute_batch(SCHEMA).unwrap();
    connection
}
fn options(path: &Path) -> IndexOptions {
    IndexOptions {
        providers: [(Provider::Opencode, vec![path.into()])].into(),
        roots_are_authoritative: true,
        since_days: None,
    }
}

#[test]
fn current_sessions_are_discovered_from_session_v2() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("opencode.db");
    let connection = Connection::open(&path).unwrap();
    // Modern layout only: no legacy `session` rows at all.
    connection.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE project(id TEXT PRIMARY KEY,worktree TEXT,vcs TEXT);
         CREATE TABLE session(id TEXT PRIMARY KEY,project_id TEXT,title TEXT,directory TEXT,time_created INTEGER,time_updated INTEGER);
         CREATE TABLE session_v2(id TEXT PRIMARY KEY,project_id TEXT,title TEXT,directory TEXT,time_created INTEGER,time_updated INTEGER);
         CREATE TABLE message(id TEXT PRIMARY KEY,session_id TEXT,time_created INTEGER,data TEXT);
         CREATE TABLE part(id TEXT PRIMARY KEY,message_id TEXT,session_id TEXT,data TEXT);
         CREATE TABLE session_message(id TEXT PRIMARY KEY,session_id TEXT,type TEXT,seq INTEGER,time_created INTEGER,data TEXT);
         INSERT INTO project VALUES('project','/work/current','git');
         INSERT INTO session_v2 VALUES('gamma','project','Current session','/work/current',1790726400000,1790726500000);
         INSERT INTO session_message VALUES('u','gamma','user',1,1790726400000,'{\"text\":\"modern marker\"}');
         INSERT INTO session_message VALUES('a','gamma','assistant',2,1790726401000,'{\"content\":[{\"type\":\"text\",\"text\":\"current answer\"}]}');
         INSERT INTO session_message VALUES('c','gamma','compaction',3,1790726402000,'{\"status\":\"completed\",\"summary\":\"compaction summary\",\"model\":{\"id\":\"m\",\"providerID\":\"p\"}}');
         INSERT INTO session_message VALUES('u2','gamma','user',4,1790726403000,'{\"text\":\"inline screenshot\",\"files\":[{\"type\":\"inline\",\"mime\":\"image/png\",\"data\":\"AAAA\",\"name\":\"image.png\"}]}');",
    )
    .unwrap();
    drop(connection);

    let sources = discover(&path).unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].identity.session.as_deref(), Some("gamma"));
    let parsed = parse(&sources[0].identity).unwrap().conversation.unwrap();
    assert_eq!(parsed.title.as_deref(), Some("Current session"));
    assert_eq!(parsed.project.as_deref(), Some("/work/current"));
    assert_eq!(parsed.repository.as_deref(), Some("/work/current"));
    assert_eq!(parsed.messages.len(), 4);
    assert!(
        parsed
            .messages
            .inline()
            .iter()
            .any(|message| message.content.contains("inline screenshot"))
    );
    assert!(
        parsed
            .messages
            .inline()
            .iter()
            .any(|message| message.content.contains("compaction summary"))
    );
}

#[test]
fn native_layouts_preserve_prose_metadata_and_context_without_duplicate_mirrors() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("opencode.db");
    let upstream = fixture(&path);
    upstream.execute_batch("INSERT INTO message VALUES('mirror','alpha',1790726400000,'{\"role\":\"user\"}');
        INSERT INTO part VALUES('mirrorpart','mirror','alpha','{\"type\":\"text\",\"text\":\"duplicate mirror\"}');
        INSERT INTO part VALUES('part2','legacy','beta','{\"type\":\"text\",\"text\":\"syntheticmarker\",\"synthetic\":true}');
        INSERT INTO part VALUES('part3','legacy','beta','{\"type\":\"text\",\"text\":\"ignoredmarker\",\"ignored\":true}');
        INSERT INTO session_message VALUES('shell','alpha','shell',3,1790726401000,'{\"command\":\"echo test\",\"output\":\"shellmarker\"}');
        INSERT INTO session_message VALUES('synthetic','alpha','synthetic',4,1790726401000,'{\"text\":\"injectionmarker\"}');").unwrap();
    let sources = discover(&path).unwrap();
    assert_eq!(sources.len(), 2);
    assert!(sources.iter().all(|s| s.estimated_bytes.unwrap() > 0));
    let before = source_stamp_for(&sources[0].identity).unwrap();
    let parsed = parse(&sources[0].identity).unwrap().conversation.unwrap();
    assert_eq!(parsed.messages.len(), 4);
    assert_eq!(parsed.project.as_deref(), Some("/work/cbioportal"));
    assert_eq!(parsed.repository.as_deref(), Some("/work/repository"));
    assert!(parsed.messages.inline()[1].content.contains("toolmarker"));
    assert!(
        parsed.messages.inline()[1]
            .content
            .contains("reasoningmarker")
    );
    assert_eq!(
        parsed.messages.inline()[1].dialogue_text.as_deref(),
        Some("Repair mutation parsing")
    );
    assert_eq!(
        parsed.messages.inline()[2].dialogue_text.as_deref(),
        Some("")
    );
    assert_eq!(
        parsed.messages.inline()[3].dialogue_text.as_deref(),
        Some("")
    );
    let legacy = parse(&sources[1].identity).unwrap().conversation.unwrap();
    assert!(
        legacy.messages.inline()[0]
            .content
            .contains("syntheticmarker")
    );
    assert_eq!(
        legacy.messages.inline()[0].dialogue_text.as_deref(),
        Some("independent native history")
    );
    assert_eq!(source_stamp_for(&sources[0].identity).unwrap(), before);
    assert!(
        sqlite_source::open(&path)
            .unwrap()
            .execute("DELETE FROM session", [])
            .is_err()
    );
}

#[test]
fn wal_refresh_deletion_and_malformed_history_preserve_index_consistency() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("opencode.db");
    let upstream = fixture(&path);
    let local = directory.path().join("index.sqlite3");
    let mut writer = Storage::open_writer(&local).unwrap();
    index(&mut writer, &options(&path)).unwrap();
    writer.commit_writer().unwrap();
    drop(writer);
    let main = std::fs::read(&path).unwrap();
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
        .execute(
            "UPDATE part SET data=?1 WHERE id='part1'",
            [r#"{"type":"text","text":"updated WAL evidence"}"#],
        )
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), main);
    let mut writer = Storage::open_writer(&local).unwrap();
    assert_eq!(
        index(&mut writer, &options(&path))
            .unwrap()
            .changed_messages,
        1
    );
    writer.commit_writer().unwrap();
    drop(writer);
    upstream
        .execute(
            "UPDATE session_message SET data='not-json' WHERE id='user'",
            [],
        )
        .unwrap();
    let mut writer = Storage::open_writer(&local).unwrap();
    assert!(index(&mut writer, &options(&path)).is_err());
    drop(writer);
    let writer = Storage::open_writer(&local).unwrap();
    assert_eq!(
        crate::storage::test_support::projections_with_text(
            &writer,
            "updated",
            Some(Provider::Opencode)
        )
        .len(),
        1
    );
    drop(writer);
    upstream
        .execute(
            "UPDATE session_message SET data=?1 WHERE id='user'",
            [r#"{"text":"Fix the cBioPortal importer"}"#],
        )
        .unwrap();
    upstream.execute_batch("DELETE FROM session WHERE id='beta'; DELETE FROM message WHERE session_id='beta'; DELETE FROM part WHERE session_id='beta';").unwrap();
    let mut writer = Storage::open_writer(&local).unwrap();
    assert_eq!(
        index(&mut writer, &options(&path))
            .unwrap()
            .purged_conversations,
        1
    );
    writer.commit_writer().unwrap();
    assert_eq!(writer.counts().unwrap().conversations, 1);
}

#[test]
fn old_native_databases_and_oversized_parts_are_handled_explicitly() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("opencode.db");
    let upstream = fixture(&path);
    upstream
        .execute_batch("DROP TABLE session_message; DELETE FROM session WHERE id='alpha';")
        .unwrap();
    let sources = discover(&path).unwrap();
    assert_eq!(
        parse(&sources[0].identity)
            .unwrap()
            .conversation
            .unwrap()
            .messages
            .len(),
        1
    );
    upstream
        .execute(
            "UPDATE part SET data=?1 WHERE id='part1'",
            ["x".repeat(8 * 1024 * 1024 + 1)],
        )
        .unwrap();
    let error = parse(&sources[0].identity).err().unwrap();
    assert!(error.error.message.contains("8 MiB"));
}

#[test]
fn session_horizon_and_attachments_do_not_promote_operational_text() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("opencode.db");
    let upstream = fixture(&path);
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    upstream
        .execute("UPDATE session SET time_updated=?1 WHERE id='alpha'", [now])
        .unwrap();
    upstream
        .execute("UPDATE session SET time_updated=1000 WHERE id='beta'", [])
        .unwrap();
    upstream.execute("UPDATE session_message SET data=?1 WHERE id='user'",[r#"{"text":"Fix the cBioPortal importer","files":[{"uri":"file:///private.txt","name":"attachmentmarker","description":"contextmarker"}]}"#]).unwrap();
    let sources = discover(&path).unwrap();
    let message = parse(&sources[0].identity)
        .unwrap()
        .conversation
        .unwrap()
        .messages
        .inline_mut()
        .remove(0);
    assert!(message.content.contains("attachmentmarker"));
    assert_eq!(
        message.dialogue_text.as_deref(),
        Some("Fix the cBioPortal importer")
    );
    let mut writer = Storage::open_writer(&directory.path().join("index.sqlite3")).unwrap();
    let mut options = options(&path);
    options.since_days = Some(1);
    let summary = index(&mut writer, &options).unwrap();
    writer.commit_writer().unwrap();
    assert_eq!(summary.processed_files, 1);
    assert_eq!(summary.processed_bytes, sources[0].estimated_bytes.unwrap());
    assert_eq!(writer.counts().unwrap().conversations, 1);
    assert!(
        crate::storage::test_support::projections_with_text(&writer, "attachmentmarker", None)
            .is_empty()
    );
}

#[tokio::test]
#[ignore = "requires MASH_TEST_MODELS_DIR with installed local models"]
async fn core_indexes_searches_and_views_native_opencode_threads() {
    use crate::core::{IndexRequest, Operation, Paths, Request, Response};
    let directory = tempfile::tempdir().unwrap();
    let history = directory.path().join("opencode.db");
    let _upstream = fixture(&history);
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        toml::to_string(&toml::toml! {
            version=1
            providers={opencode={roots=[(history.to_str().unwrap())]}}
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
    let Response::Search(response) = crate::core::execute(Request {
        paths: paths.clone(),
        operation: Operation::Search {
            shared_vector_search: None,
            fusion: crate::views::config::FusionOverrides::default(),
            ranking: None,
            diagnostics: false,
            query: "cbioportal importer".into(),
            intent: None,
            limit: Some(5),
            offset: 0,
            unit: Some(crate::search_results::SearchUnit::Chats),
            filters: Box::new(crate::search_filter::Selection {
                provider: vec![Provider::Opencode],
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
    let crate::search_results::Results::Chats(threads) = response.results else {
        panic!("chats")
    };
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].provider, Provider::Opencode);
    assert_eq!(
        threads[0].metadata.repository.as_deref(),
        Some("/work/repository")
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
            .any(|m| m.content.contains("toolmarker"))
    );
}
