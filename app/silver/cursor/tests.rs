use super::*;
use crate::{
    silver::{IndexOptions, index},
    storage::Storage,
};
use prost::Message;
use rusqlite::{Connection, params};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

mod failures;
mod retrieval;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    metadata_hex: String,
    blobs: Vec<Blob>,
}
#[derive(Deserialize)]
struct Blob {
    id: String,
    hex: String,
}

fn fixture(path: &Path) -> Connection {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/cursor-agent-native-v1.json"
    ))
    .unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let connection = Connection::open(path).unwrap();
    connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA user_version=1; CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT); CREATE TABLE blobs(id TEXT PRIMARY KEY,data BLOB);").unwrap();
    connection
        .execute("INSERT INTO meta VALUES('0',?1)", [fixture.metadata_hex])
        .unwrap();
    for blob in fixture.blobs {
        connection
            .execute(
                "INSERT INTO blobs VALUES(?1,?2)",
                params![blob.id, hex::decode(blob.hex).unwrap()],
            )
            .unwrap();
    }
    connection
}
fn path(root: &Path) -> PathBuf {
    root.join("chats/workspace/session/store.db")
}
fn options(path: &Path) -> IndexOptions {
    IndexOptions {
        providers: [(Provider::Cursor, vec![path.into()])].into(),
        since_days: None,
        roots_are_authoritative: true,
    }
}
fn parsed(path: &Path) -> Conversation {
    parse(&discover(path).unwrap().remove(0).identity)
        .unwrap()
        .conversation
        .unwrap()
}
fn metadata(connection: &Connection) -> models::Metadata {
    let value: String = connection
        .query_row("SELECT value FROM meta WHERE key='0'", [], |r| r.get(0))
        .unwrap();
    serde_json::from_slice(&hex::decode(value).unwrap()).unwrap()
}
fn put(connection: &Connection, bytes: &[u8]) -> Vec<u8> {
    let id = Sha256::digest(bytes).to_vec();
    connection
        .execute(
            "INSERT OR REPLACE INTO blobs VALUES(?1,?2)",
            params![hex::encode(&id), bytes],
        )
        .unwrap();
    id
}
fn root(connection: &Connection) -> proto::Root {
    let id = metadata(connection).latest_root_blob_id;
    let bytes: Vec<u8> = connection
        .query_row("SELECT data FROM blobs WHERE id=?1", [id], |r| r.get(0))
        .unwrap();
    proto::Root::decode(bytes.as_slice()).unwrap()
}
fn checkpoint(connection: &Connection, root: &proto::Root) {
    let id = put(connection, &root.encode_to_vec());
    let mut meta = metadata(connection);
    meta.latest_root_blob_id = hex::encode(id);
    set_metadata(connection, &meta);
}
fn set_metadata(connection: &Connection, meta: &models::Metadata) {
    let value = hex::encode(serde_json::to_vec(meta).unwrap());
    connection
        .execute("UPDATE meta SET value=?1 WHERE key='0'", [value])
        .unwrap();
}
fn searchable(conversation: &Conversation) -> Vec<&NormalizedMessage> {
    conversation
        .messages
        .inline()
        .iter()
        .filter(|m| !m.dialogue_text.as_deref().unwrap_or("").is_empty())
        .collect()
}

#[test]
fn first_party_native_fixture_hydrates_archives_and_only_visible_prose() {
    let dir = tempfile::tempdir().unwrap();
    let path = path(dir.path());
    let connection = fixture(&path);
    let conversation = parsed(&path);
    assert_eq!(conversation.messages.len(), 6);
    let searchable = searchable(&conversation);
    assert_eq!(searchable.len(), 4);
    assert_eq!(
        searchable[0].content,
        "How does the cBioPortal mutation importer work?"
    );
    assert_eq!(searchable[2].content, "Fix the cBioPortal importer.");
    assert_eq!(
        searchable[3].dialogue_text.as_deref(),
        Some("Validate structural variants and mutation metadata.")
    );
    assert!(
        searchable
            .iter()
            .all(|m| !m.dialogue_text.as_deref().unwrap().contains("private-"))
    );
    assert!(
        conversation.messages.inline()[5]
            .content
            .contains("private-reasoning-context")
    );
    assert_eq!(conversation.project.as_deref(), Some("/work/cbioportal"));
    assert_eq!(conversation.repository, None);
    assert_eq!(
        searchable[0].created_at, None,
        "do not invent archive timestamps"
    );
    assert_eq!(searchable[2].created_at, Some(1_790_762_400_001));
    assert_eq!(conversation.created_at, Some(1_790_762_400_000));
    assert_eq!(conversation.updated_at, Some(1_790_762_400_002));
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        parsed(&path)
            .messages
            .inline()
            .iter()
            .map(|m| &m.id)
            .collect::<Vec<_>>(),
        conversation
            .messages
            .inline()
            .iter()
            .map(|m| &m.id)
            .collect::<Vec<_>>()
    );
}

#[test]
fn repeated_requests_keep_distinct_stable_ids_through_archive_moves_and_rewinds() {
    let dir = tempfile::tempdir().unwrap();
    let path = path(dir.path());
    let connection = fixture(&path);
    let mut root = root(&connection);
    let old = parsed(&path);
    for i in 0..2 {
        let user = proto::User {
            text: "Fix the cBioPortal importer.".into(),
            id: format!("new-{i}"),
            ..Default::default()
        };
        let id = put(&connection, &user.encode_to_vec());
        let turn = proto::Turn {
            kind: Some(proto::TurnKind::Agent(proto::AgentTurn {
                user: id,
                steps: Vec::new(),
            })),
        };
        root.turns.push(put(&connection, &turn.encode_to_vec()));
        root.messages.push(root.messages[2].clone());
    }
    checkpoint(&connection, &root);
    let repeated = parsed(&path);
    let requests = repeated
        .messages
        .inline()
        .iter()
        .filter(|m| m.content == "Fix the cBioPortal importer.")
        .collect::<Vec<_>>();
    assert_eq!(requests.len(), 3);
    assert_ne!(requests[0].id, requests[1].id);
    assert_ne!(requests[1].id, requests[2].id);
    assert_eq!(requests[0].id, old.messages.inline()[4].id);
    let ids = searchable(&repeated)
        .into_iter()
        .map(|m| m.id.clone())
        .collect::<Vec<_>>();
    root.archives.push(put(
        &connection,
        &proto::Archive {
            messages: std::mem::take(&mut root.messages),
        }
        .encode_to_vec(),
    ));
    root.turns.clear();
    checkpoint(&connection, &root);
    assert_eq!(
        searchable(&parsed(&path))
            .into_iter()
            .map(|m| m.id.clone())
            .collect::<Vec<_>>(),
        ids
    );
    let archive: proto::Archive = {
        let bytes: Vec<u8> = connection
            .query_row(
                "SELECT data FROM blobs WHERE id=?1",
                [hex::encode(root.archives.last().unwrap())],
                |r| r.get(0),
            )
            .unwrap();
        proto::Archive::decode(bytes.as_slice()).unwrap()
    };
    let mut rewind = archive;
    rewind.messages.pop();
    let archived = put(&connection, &rewind.encode_to_vec());
    *root.archives.last_mut().unwrap() = archived;
    checkpoint(&connection, &root);
    assert_eq!(searchable(&parsed(&path)).len(), 5);
    assert_eq!(
        searchable(&parsed(&path)).last().unwrap().id,
        requests[1].id
    );
}

#[test]
fn simulated_users_and_subagent_sessions_stay_context_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = path(dir.path());
    let connection = fixture(&path);
    let mut root = root(&connection);
    let user = proto::User {
        text: "Fix the cBioPortal importer.".into(),
        simulated: Some(true),
        simulated_reason: Some(31),
        ..Default::default()
    };
    let user = put(&connection, &user.encode_to_vec());
    root.turns[0] = put(
        &connection,
        &proto::Turn {
            kind: Some(proto::TurnKind::Agent(proto::AgentTurn {
                user,
                steps: Vec::new(),
            })),
        }
        .encode_to_vec(),
    );
    checkpoint(&connection, &root);
    let conversation = parsed(&path);
    assert_eq!(
        conversation.messages.inline()[4].dialogue_text.as_deref(),
        Some("")
    );
    let mut meta = metadata(&connection);
    meta.subagent_info = Some(models::Subagent {
        parent_agent_id: "parent".into(),
        root_parent_agent_id: "root".into(),
        tool_call_id: "call".into(),
        type_name: "worker".into(),
    });
    set_metadata(&connection, &meta);
    assert_eq!(searchable(&parsed(&path)), Vec::<&NormalizedMessage>::new());
    assert_eq!(parsed(&path).messages.len(), 6);
}
