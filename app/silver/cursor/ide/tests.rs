use super::*;
use crate::silver::NormalizedMessage;
use crate::silver::{IndexOptions, discovered_sources, index};
use crate::storage::Storage;
use models::{Bubble, Kind};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::path::PathBuf;

mod failures;

#[derive(Deserialize)]
struct Fixture {
    rows: Vec<Row>,
    #[serde(rename = "expectedBubbleIds")]
    expected_bubble_ids: Vec<String>,
}
#[derive(Deserialize)]
struct Row {
    key: String,
    value: String,
}

pub(in crate::silver::cursor) fn fixture(path: &Path) -> Connection {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../../fixtures/cursor-ide-native-v18.json"
    ))
    .unwrap();
    assert_eq!(fixture.expected_bubble_ids.len(), 6);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let connection = Connection::open(path).unwrap();
    connection.execute_batch("PRAGMA user_version=1; PRAGMA journal_mode=WAL; CREATE TABLE ItemTable(key TEXT UNIQUE ON CONFLICT REPLACE,value BLOB); CREATE TABLE cursorDiskKV(key TEXT UNIQUE ON CONFLICT REPLACE,value BLOB);").unwrap();
    for row in fixture.rows {
        connection
            .execute(
                "INSERT INTO cursorDiskKV VALUES(?1,?2)",
                params![row.key, row.value],
            )
            .unwrap();
    }
    connection
}
fn path(root: &Path) -> PathBuf {
    root.join("User/globalStorage/state.vscdb")
}
fn root_key() -> &'static str {
    "composerData:fixture-ide-session"
}
fn bubble_key(id: &str) -> String {
    format!("bubbleId:fixture-ide-session:{id}")
}
fn read<T: DeserializeOwned>(connection: &Connection, key: &str) -> T {
    let value: String = connection
        .query_row("SELECT value FROM cursorDiskKV WHERE key=?1", [key], |r| {
            r.get(0)
        })
        .unwrap();
    serde_json::from_str(&value).unwrap()
}
fn write<T: Serialize>(connection: &Connection, key: &str, value: &T) {
    connection
        .execute(
            "INSERT OR REPLACE INTO cursorDiskKV VALUES(?1,?2)",
            params![key, serde_json::to_string(value).unwrap()],
        )
        .unwrap();
}
fn parsed(path: &Path) -> Chat {
    super::super::parse(&super::super::discover(path).unwrap().remove(0).identity)
        .unwrap()
        .chat
        .unwrap()
}
fn options(path: &Path) -> IndexOptions {
    IndexOptions {
        providers: [(Provider::Cursor, vec![path.into()])].into(),
        since_days: None,
        roots_are_authoritative: true,
    }
}
fn searchable(chat: &Chat) -> Vec<&NormalizedMessage> {
    chat.messages
        .inline()
        .iter()
        .filter(|m| !m.dialogue_text.as_deref().unwrap_or_default().is_empty())
        .collect()
}

#[test]
fn native_root_owns_order_metadata_and_distinct_repeated_requests() {
    let dir = tempfile::tempdir().unwrap();
    let path = path(dir.path());
    let connection = fixture(&path);
    let chat = parsed(&path);
    assert_eq!(chat.messages.len(), 6);
    assert_eq!(searchable(&chat).len(), 4);
    assert_eq!(chat.project.as_deref(), Some("/work/cbioportal"));
    assert_eq!(chat.repository.as_deref(), Some("/work/cbioportal"));
    assert_eq!(chat.title.as_deref(), Some("cBioPortal native IDE fixture"));
    assert_eq!(chat.created_at, Some(1_790_762_400_000));
    assert_eq!(chat.updated_at, Some(1_790_762_400_001));
    assert_eq!(
        chat.messages.inline()[0].content,
        chat.messages.inline()[4].content
    );
    assert_ne!(chat.messages.inline()[0].id, chat.messages.inline()[4].id);
    assert_eq!(
        chat.messages.inline()[0].created_at,
        Some(1_790_762_400_000)
    );
    assert_eq!(chat.messages.inline()[1].created_at, None);
    assert!(
        chat.messages.inline()[1]
            .content
            .contains("private-ide-reasoning")
    );
    assert!(
        chat.messages
            .inline()
            .iter()
            .all(|m| !m.content.contains("unsent-draft")
                && !m.content.contains("ide-summary")
                && !m.content.contains("abandoned-branch")
                && !m.content.contains("AAAAAAAA"))
    );
    assert!(
        searchable(&chat)
            .iter()
            .all(|m| !m.dialogue_text.as_deref().unwrap().contains("private-"))
    );
    let mut bubble: Bubble = read(&connection, &bubble_key("request-1"));
    bubble.text = "Updated cBioPortal importer request".into();
    write(&connection, &bubble_key("request-1"), &bubble);
    let edited = parsed(&path);
    assert_eq!(edited.messages.inline()[0].id, chat.messages.inline()[0].id);
    assert_ne!(
        edited.messages.inline()[0].content,
        chat.messages.inline()[0].content
    );
    let mut root: Root = read(&connection, root_key());
    root.full_conversation_headers_only.truncate(2);
    write(&connection, root_key(), &root);
    let rewind = parsed(&path);
    assert_eq!(rewind.messages.len(), 2);
    assert_eq!(rewind.messages.inline()[1].id, chat.messages.inline()[1].id);
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM cursorDiskKV WHERE key LIKE 'bubbleId:%'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        7
    );
}

#[test]
fn logical_composers_are_discovered_independently_and_schema_detection_is_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let path = path(dir.path());
    let connection = fixture(&path);
    let mut root: Root = read(&connection, root_key());
    root.composer_id = "second".into();
    root.full_conversation_headers_only.clear();
    write(&connection, "composerData:second", &root);
    let sources = discovered_sources(Provider::Cursor, &[dir.path().into()]);
    assert_eq!(sources.len(), 2);
    assert_ne!(sources[0].identity.key(), sources[1].identity.key());
    assert!(
        sources
            .iter()
            .all(|s| s.identity.session.as_deref().unwrap().starts_with("ide:"))
    );
    let db = dir.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&db).unwrap();
    index(&mut storage, &options(dir.path())).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().chats, 2);
    assert_eq!(storage.counts().unwrap().messages, 6);
    connection
        .execute_batch(
            "CREATE TABLE meta(key TEXT,value TEXT); CREATE TABLE blobs(id TEXT,data BLOB);",
        )
        .unwrap();
    assert!(super::super::discover(&path).is_err());
}

/// Found on macOS with Cursor 3.30.8: 19 of 27 workspace databases had no
/// chat table, and discovery refused the whole provider.
#[test]
fn workspace_state_without_a_chat_table_holds_no_chats_and_is_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    fixture(&path(dir.path()));
    let settings_only = dir
        .path()
        .join("workspaceStorage/settings-only/state.vscdb");
    std::fs::create_dir_all(settings_only.parent().unwrap()).unwrap();
    Connection::open(&settings_only)
        .unwrap()
        .execute_batch("CREATE TABLE ItemTable(key TEXT UNIQUE ON CONFLICT REPLACE,value BLOB);")
        .unwrap();
    assert!(super::super::discover(&settings_only).unwrap().is_empty());
    let sources = discovered_sources(Provider::Cursor, &[dir.path().into()]);
    assert_eq!(sources.len(), 1);
    let db = dir.path().join("index.sqlite3");
    let mut storage = Storage::open_writer(&db).unwrap();
    index(&mut storage, &options(dir.path())).unwrap();
    storage.commit_writer().unwrap();
    assert_eq!(storage.counts().unwrap().chats, 1);
}

#[test]
fn synthetic_operational_and_unknown_bubbles_remain_context_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = path(dir.path());
    let connection = fixture(&path);
    let key = bubble_key("request-1");
    let original: String = connection
        .query_row("SELECT value FROM cursorDiskKV WHERE key=?1", [&key], |r| {
            r.get(0)
        })
        .unwrap();
    let variants: [fn(&mut Bubble); 7] = [
        |b| b.is_placeholder = true,
        |b| b.is_simulated_msg = true,
        |b| b.simulated_msg_reason = Some(999),
        |b| {
            b.simulated_message_metadata = Some(models::Simulated {
                suppress_ui_display: true,
            });
        },
        |b| b.sent_by_agent_id = Some("sender".into()),
        |b| {
            b.service_status_update = Some(models::ProtoRecord::Json(
                "{\"message\":\"private-status-context\"}".into(),
            ));
        },
        |b| {
            b.event_notification = Some(models::Notification {
                headline: "private-notification-context".into(),
                detail: None,
            });
        },
    ];
    for modify in variants {
        let mut bubble: Bubble = serde_json::from_str(&original).unwrap();
        modify(&mut bubble);
        write(&connection, &key, &bubble);
        let chat = parsed(&path);
        assert_eq!(searchable(&chat).len(), 3);
        assert_eq!(chat.messages.inline()[0].dialogue_text.as_deref(), Some(""));
    }
    let mut bubble: Bubble = serde_json::from_str(&original).unwrap();
    bubble.kind = Kind::Other(999);
    write(&connection, &key, &bubble);
    let mut root: Root = read(&connection, root_key());
    root.full_conversation_headers_only[0].kind = bubble.kind;
    write(&connection, root_key(), &root);
    assert_eq!(searchable(&parsed(&path)).len(), 3);
    root.subagent_info = Some(models::Subagent {
        parent_composer_id: "parent".into(),
    });
    write(&connection, root_key(), &root);
    assert_eq!(searchable(&parsed(&path)), Vec::<&NormalizedMessage>::new());
    assert_eq!(parsed(&path).messages.len(), 6);
}

#[test]
fn wal_changes_refresh_sources_without_mixing_snapshot_revisions() {
    let dir = tempfile::tempdir().unwrap();
    let path = path(dir.path());
    let connection = fixture(&path);
    connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA wal_autocheckpoint=0;")
        .unwrap();
    let source = super::super::discover(&path).unwrap().remove(0);
    let stamp = source.stamp().unwrap();
    let mut snapshot = Store::open(&path).unwrap();
    let _: Root = snapshot.json(root_key()).unwrap();
    let key = bubble_key("request-1");
    let mut bubble: Bubble = read(&connection, &key);
    bubble.text = "WAL cBioPortal changes".into();
    write(&connection, &key, &bubble);
    let old: Bubble = snapshot.json(&key).unwrap();
    assert_ne!(old.text, bubble.text);
    assert_eq!(parsed(&path).messages.inline()[0].content, bubble.text);
    assert_ne!(stamp.fingerprint, source.stamp().unwrap().fingerprint);
    assert!(path.with_file_name("state.vscdb-wal").exists());
}
