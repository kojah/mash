use super::*;

use rusqlite::params;

#[test]
fn metadata_upgrade_preserves_history_and_vectors_and_requests_source_rescan() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("upgrade.sqlite3");
    let mut storage = Storage::open_writer(&path).unwrap();
    let conversation = conversation("hello metadata upgrade");
    storage.replace_conversation(&conversation).unwrap();
    storage
        .replace_embeddings(
            "fixture-generation",
            &[EmbeddingWrite {
                message_id: &conversation.messages[0].id,
                vector: &[127, 0],
                norm: 127.0,
            }],
        )
        .unwrap();
    storage
        .record_source_checkpoint(crate::provider::Provider::Codex, "/tmp/old.jsonl", 10, 20)
        .unwrap();
    storage.commit_writer().unwrap();
    storage.connection.execute_batch("ALTER TABLE conversations DROP COLUMN project; ALTER TABLE conversations DROP COLUMN repository; PRAGMA user_version=14;").unwrap();
    drop(storage);
    let storage = Storage::open(&path).unwrap();
    assert_eq!(storage.counts().unwrap().messages, 1);
    assert_eq!(storage.embedding_count("fixture-generation").unwrap(), 1);
    assert!(
        !storage
            .source_checkpoint_matches(crate::provider::Provider::Codex, "/tmp/old.jsonl", 10, 20)
            .unwrap()
    );
    let metadata: (Option<String>, Option<String>) = storage
        .connection
        .query_row("SELECT project,repository FROM conversations", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(metadata, (None, None));
}

#[test]
fn search_text_upgrade_rescans_sources_without_discarding_history_or_vectors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("upgrade.sqlite3");
    let mut storage = Storage::open_writer(&path).unwrap();
    let conversation = conversation("preserved request");
    storage.replace_conversation(&conversation).unwrap();
    storage
        .replace_embeddings(
            "fixture",
            &[EmbeddingWrite {
                message_id: &conversation.messages[0].id,
                vector: &[127, 0],
                norm: 127.0,
            }],
        )
        .unwrap();
    storage
        .record_source_checkpoint(
            crate::provider::Provider::Codex,
            "/tmp/source.jsonl",
            10,
            20,
        )
        .unwrap();
    storage.commit_writer().unwrap();
    storage
        .connection
        .execute_batch("PRAGMA user_version=15")
        .unwrap();
    drop(storage);
    let storage = Storage::open(&path).unwrap();
    assert_eq!(storage.counts().unwrap().messages, 1);
    assert_eq!(storage.embedding_count("fixture").unwrap(), 1);
    assert!(
        !storage
            .source_checkpoint_matches(
                crate::provider::Provider::Codex,
                "/tmp/source.jsonl",
                10,
                20
            )
            .unwrap()
    );
    assert_eq!(
        storage.view(&conversation.messages[0].id, 0).unwrap()[0].content,
        "preserved request"
    );
}

const VERSION_SEVEN_SCHEMA_FIXTURE: &str = "CREATE TABLE conversations (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL CHECK (provider IN (
        'claude-code', 'codex', 'opencode', 'github-copilot', 'hermes', 'pi'
    )),
    source_path TEXT NOT NULL UNIQUE, title TEXT, created_at INTEGER, updated_at INTEGER,
    source_fingerprint TEXT NOT NULL DEFAULT ''
 );
 CREATE TABLE messages (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL,
    created_at INTEGER, fingerprint TEXT NOT NULL DEFAULT '',
    UNIQUE(conversation_id, ordinal)
 );
 CREATE TABLE message_embeddings (
    message_id TEXT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    generation TEXT NOT NULL DEFAULT '', dimensions INTEGER NOT NULL,
    norm REAL NOT NULL, vector BLOB NOT NULL
 );
 CREATE VIRTUAL TABLE message_fts USING fts5(
    content, message_id UNINDEXED, conversation_id UNINDEXED, tokenize = 'unicode61'
 );
 CREATE TABLE tombstones (
    provider TEXT NOT NULL, conversation_id TEXT NOT NULL,
    forgotten_at INTEGER NOT NULL, PRIMARY KEY(provider, conversation_id)
 );
 CREATE TABLE source_checkpoints (
    provider TEXT NOT NULL, source_path TEXT NOT NULL,
    size_bytes INTEGER NOT NULL, modified_ns INTEGER NOT NULL,
    PRIMARY KEY(provider, source_path)
 );
 CREATE TABLE derived_state (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    search_dirty INTEGER NOT NULL CHECK(search_dirty IN (0, 1))
 );
 INSERT INTO derived_state VALUES (1, 0);
 INSERT INTO conversations(id, provider, source_path, source_fingerprint)
    VALUES ('session-1', 'codex', '/tmp/session-1.jsonl', 'old-source');
 INSERT INTO messages(id, conversation_id, ordinal, role, content, fingerprint)
    VALUES ('message-1', 'session-1', 0, 'user', 'preserved', 'old-message');
 INSERT INTO message_fts(content, message_id, conversation_id)
    VALUES ('preserved', 'message-1', 'session-1');
 INSERT INTO message_embeddings(message_id, generation, dimensions, norm, vector)
    VALUES ('message-1', 'old-generation', 1, 1.0, X'7F');
 INSERT INTO source_checkpoints(provider, source_path, size_bytes, modified_ns)
    VALUES ('codex', '/tmp/session-1.jsonl', 10, 20);
 PRAGMA user_version = 7;";

fn version_eight_provider_fixture() -> String {
    VERSION_SEVEN_SCHEMA_FIXTURE
        .replace(
            "created_at INTEGER, fingerprint TEXT NOT NULL DEFAULT ''",
            "search_projection TEXT, created_at INTEGER, fingerprint TEXT NOT NULL DEFAULT ''",
        )
        .replace(
            "PRAGMA user_version = 7;",
            "INSERT INTO tombstones(provider, conversation_id, forgotten_at)
                VALUES ('codex', 'forgotten-codex', 25);
             INSERT INTO conversations(id, provider, source_path, source_fingerprint)
                VALUES ('unsupported-session', 'opencode', '/tmp/opencode.jsonl', 'source');
             INSERT INTO messages(
                id, conversation_id, ordinal, role, content, search_projection, fingerprint
             ) VALUES (
                'unsupported-message', 'unsupported-session', 0, 'user',
                'unsupported sentinel', NULL, 'message'
             );
             INSERT INTO message_fts(content, message_id, conversation_id)
                VALUES ('unsupported sentinel', 'unsupported-message', 'unsupported-session');
             INSERT INTO message_embeddings(message_id, generation, dimensions, norm, vector)
                VALUES ('unsupported-message', 'generation', 1, 1.0, X'7F');
             INSERT INTO source_checkpoints(provider, source_path, size_bytes, modified_ns)
                VALUES ('opencode', '/tmp/opencode.jsonl', 10, 20);
             INSERT INTO tombstones(provider, conversation_id, forgotten_at)
                VALUES ('opencode', 'forgotten-opencode', 30);
             PRAGMA user_version = 8;",
        )
}

#[test]
fn supported_schema_migrates_once_and_preserves_rows() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let connection = Connection::open(&path).expect("seed database");
    connection
        .execute_batch(VERSION_SEVEN_SCHEMA_FIXTURE)
        .expect("seed older schema");
    drop(connection);

    let storage = Storage::open(&path).expect("migrate database");
    let counts = storage.counts().expect("counts");
    assert_eq!(counts.messages, 1);
    assert_eq!(counts.embeddings, 0);
    assert_eq!(counts.searchable_messages, 1);
    assert!(
        storage
            .derived_search_is_dirty()
            .expect("dirty derived state")
    );
    assert_eq!(
        storage
            .connection
            .query_row("SELECT count(*) FROM message_fts", [], |row| row
                .get::<_, i64>(0))
            .expect("cleared FTS"),
        0
    );
    assert_eq!(
        storage
            .connection
            .query_row(
                "SELECT search_projection FROM messages WHERE id = 'message-1'",
                [],
                |row| row.get::<_, Option<String>>(0)
            )
            .expect("projection column"),
        None
    );
    assert_eq!(
        storage.view("message-1", 0).expect("canonical view")[0].content,
        "preserved"
    );
    assert_eq!(
        storage
            .connection
            .query_row("SELECT count(*) FROM source_checkpoints", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("cleared checkpoints"),
        0
    );
    assert_eq!(
        storage
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .expect("schema version"),
        SCHEMA_VERSION
    );
    drop(storage);
    let reopened = Storage::open(&path).expect("idempotent second open");
    assert_eq!(reopened.counts().expect("reopened counts").messages, 1);
    assert!(reopened.derived_search_is_dirty().expect("still dirty"));
}

#[test]
fn version_eight_migration_removes_every_unsupported_provider_surface() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let connection = Connection::open(&path).expect("seed database");
    let fixture = version_eight_provider_fixture();
    connection
        .execute_batch(&fixture)
        .expect("seed version eight");
    drop(connection);

    let storage = Storage::open(&path).expect("migrate database");
    for table in ["conversations", "source_checkpoints", "tombstones"] {
        let query = format!("SELECT count(*) FROM {table} WHERE provider = 'opencode'");
        assert_eq!(
            storage
                .connection
                .query_row(&query, [], |row| row.get::<_, i64>(0))
                .expect("unsupported provider count"),
            0,
            "unsupported rows remain in {table}"
        );
    }
    assert_eq!(
        storage
            .connection
            .query_row(
                "SELECT count(*) FROM source_checkpoints WHERE provider = 'codex'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("supported checkpoint count"),
        0
    );
    assert_eq!(
        storage
            .connection
            .query_row(
                "SELECT count(*) FROM tombstones WHERE provider = 'codex'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("supported tombstone count"),
        1
    );
    assert_eq!(storage.counts().expect("counts").conversations, 1);
    assert_eq!(storage.counts().expect("counts").messages, 1);
    assert_eq!(storage.counts().expect("counts").embeddings, 1);
    assert!(
        storage
            .semantic_index_is_ready("old-generation")
            .expect("backfilled semantic readiness")
    );
    assert_eq!(
        storage
            .connection
            .query_row(
                "SELECT count(*) FROM message_fts WHERE content = 'unsupported sentinel'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("unsupported FTS count"),
        0
    );
    let schema: String = storage
        .connection
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name = 'conversations'",
            [],
            |row| row.get(0),
        )
        .expect("provider schema");
    assert!(schema.contains("'claude-code'"));
    assert!(schema.contains("'codex'"));
    assert!(!schema.contains("'opencode'"));
    for provider in ["opencode", "github-copilot", "hermes", "pi"] {
        let error = storage
            .connection
            .execute(
                "INSERT INTO conversations(id, provider, source_path)
                 VALUES (?1, ?2, ?3)",
                params![
                    format!("rejected-{provider}"),
                    provider,
                    format!("/tmp/rejected-{provider}.jsonl")
                ],
            )
            .expect_err("unsupported provider rejected");
        assert!(matches!(error, rusqlite::Error::SqliteFailure(_, _)));
    }
    assert_eq!(
        storage
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .expect("schema version"),
        SCHEMA_VERSION
    );
}

#[test]
fn newer_schema_is_rejected_without_rewriting_version() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mash.sqlite3");
    let connection = Connection::open(&path).expect("seed database");
    connection
        .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .expect("newer version");
    drop(connection);

    let error = Storage::open(&path).err().expect("newer schema rejected");
    assert_eq!(error.error.kind, crate::ErrorKind::SchemaIncompatible);
    let connection = Connection::open(&path).expect("reopen seed");
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .expect("schema version"),
        SCHEMA_VERSION + 1
    );
}
