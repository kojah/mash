use super::*;

use rusqlite::params;

#[test]
fn schema_migrations_preserve_context_vectors_and_require_projection_refresh() {
    for version in [23, 28, 29, 30, 31] {
        provider_schema_migration(version);
    }
}

#[test]
fn legacy_readiness_backfills_do_not_override_projection_refresh() {
    for version in [8, 9, 15] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("legacy.sqlite3");
        let storage = Storage::open(&path).unwrap();
        storage.connection.execute_batch(
            "INSERT INTO conversations(id,provider,source_path,source_key) VALUES('session','codex','history','file:history');
             INSERT INTO messages(id,conversation_id,ordinal,role,content) VALUES('message','session',0,'user','durable history');
             INSERT INTO message_embeddings VALUES('message','fixture',1,127.0,x'7f');
             UPDATE derived_state SET search_dirty=0,semantic_ready_generation='fixture';"
        ).unwrap();
        storage
            .connection
            .pragma_update(None, "user_version", version)
            .unwrap();
        drop(storage);
        let upgraded = Storage::open(&path).unwrap();
        assert!(upgraded.derived_search_is_dirty().unwrap());
        assert!(!upgraded.semantic_index_is_ready("fixture").unwrap());
        assert_eq!(upgraded.embedding_count("fixture").unwrap(), 1);
        assert_eq!(
            upgraded.view("message", 0).unwrap()[0].content,
            "durable history"
        );
    }
}

fn provider_schema_migration(version: i64) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("old.sqlite3");
    let connection = Connection::open(&path).unwrap();
    // Both native predecessors have the same message layout; provider checks
    // differ. Verify old and immediately preceding schema migrations.
    let mut schema = include_str!("../schema.sql").to_owned();
    if version < 29 {
        schema = schema.replace(", 'antigravity'", "");
    }
    if version == 23 {
        for provider in ["copilot", "gemini-cli", "cline", "cursor", "windsurf"] {
            schema = schema.replace(&format!(", '{provider}'"), "");
        }
    }
    connection.execute_batch(&schema).unwrap();
    connection.execute_batch(
        "INSERT INTO conversations(id,provider,source_path,source_key) VALUES('session','pi','/tmp/pi.jsonl','file:/tmp/pi.jsonl');
         INSERT INTO messages(storage_id,id,conversation_id,ordinal,role,content,search_projection,context_parent)
             VALUES(42,'root','session',0,'user','original request','original request',-1),
                   (43,'child','session',1,'assistant','branch reply','branch reply',0);
         INSERT INTO message_fts(rowid,content,message_id,conversation_id)
             VALUES(42,'original request','root','session'),(43,'branch reply','child','session');
         INSERT INTO message_embeddings VALUES('root','fixture',2,127.0,x'7f00'),('child','fixture',2,127.0,x'7f00');
         INSERT INTO source_checkpoints(provider,source_path,source_key,size_bytes,modified_ns)
             VALUES('pi','/tmp/pi.jsonl','file:/tmp/pi.jsonl',100,200);
         UPDATE derived_state SET search_dirty=0,semantic_ready_generation='fixture';
         "
    ).unwrap();
    connection
        .pragma_update(None, "user_version", version)
        .unwrap();
    crate::storage::semantic_chunks::rebuild_all_semantic_chunks(&connection).unwrap();
    if version < 29 {
        assert!(connection.execute("INSERT INTO conversations(id,provider,source_path,source_key) VALUES('new','antigravity','new','new')",[]).is_err());
    }
    drop(connection);
    let storage = Storage::open(&path).unwrap();
    assert_eq!(storage.embedding_count("fixture").unwrap(), 2);
    assert!(storage.derived_search_is_dirty().unwrap());
    assert!(!storage.semantic_index_is_ready("fixture").unwrap());
    assert_eq!(
        storage.search("branch", 10, None, None).unwrap()[0].id,
        "child"
    );
    assert_eq!(
        storage
            .view("child", 1)
            .unwrap()
            .iter()
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>(),
        ["root", "child"]
    );
    let rows: Vec<(i64, Option<i64>)> = storage
        .connection
        .prepare("SELECT storage_id,context_parent FROM messages ORDER BY ordinal")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(rows, [(42, Some(-1)), (43, Some(0))]);
    assert!(
        !storage
            .source_checkpoint_matches(
                crate::provider::Provider::Pi,
                &crate::ingestion::SourceIdentity::file("/tmp/pi.jsonl"),
                &crate::ingestion::SourceStamp::file(100, 200)
            )
            .unwrap()
    );
    storage.connection.execute("INSERT INTO conversations(id,provider,source_path,source_key) VALUES('new','antigravity','new','new')",[]).unwrap();
}

#[test]
fn version_eighteen_migration_preserves_history_and_rescans_sources_once() {
    use crate::ingestion::SourceIdentity;
    use crate::provider::Provider;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("old.sqlite3");
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch(
        "CREATE TABLE conversations (
            id TEXT PRIMARY KEY, provider TEXT NOT NULL CHECK(provider IN ('claude-code','codex','aider')),
            source_path TEXT NOT NULL UNIQUE, title TEXT, created_at INTEGER, updated_at INTEGER,
            source_fingerprint TEXT NOT NULL DEFAULT '', project TEXT, repository TEXT
         );
         CREATE TABLE source_checkpoints (
            provider TEXT NOT NULL, source_path TEXT NOT NULL, size_bytes INTEGER NOT NULL,
            modified_ns INTEGER NOT NULL, PRIMARY KEY(provider, source_path)
         );"
    ).unwrap();
    connection
        .execute_batch(include_str!("../schema.sql"))
        .unwrap();
    connection.execute_batch(
        "INSERT INTO conversations(id,provider,source_path,project,repository)
            VALUES('existing','codex','/tmp/history.jsonl','/work/project','https://example.test/repo.git');
         INSERT INTO messages(storage_id,id,conversation_id,ordinal,role,content)
            VALUES(42,'message','existing',0,'user','durable evidence');
         INSERT INTO message_embeddings VALUES('message','fixture',2,127.0,x'7f00');
         INSERT INTO source_checkpoints VALUES('codex','/tmp/history.jsonl',100,200);
         UPDATE derived_state SET search_dirty=0,semantic_ready_generation='fixture';
         PRAGMA user_version=18;"
    ).unwrap();
    drop(connection);
    let mut storage = Storage::open_writer(&path).unwrap();
    assert_eq!(storage.counts().unwrap().messages, 1);
    assert_eq!(storage.embedding_count("fixture").unwrap(), 1);
    assert!(
        !storage
            .source_checkpoint_matches(
                Provider::Codex,
                &SourceIdentity::file("/tmp/history.jsonl"),
                &crate::ingestion::SourceStamp::file(100, 200)
            )
            .unwrap()
    );
    let metadata: (i64, String, Option<String>, String, String) = storage.connection.query_row(
        "SELECT m.storage_id,c.source_key,c.source_session,c.project,c.repository FROM messages m JOIN conversations c ON c.id=m.conversation_id",
        [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))
    ).unwrap();
    assert_eq!(
        metadata,
        (
            42,
            "file:/tmp/history.jsonl".into(),
            None,
            "/work/project".into(),
            "https://example.test/repo.git".into()
        )
    );
    assert!(storage.derived_search_is_dirty().unwrap());
    assert!(!storage.semantic_index_is_ready("fixture").unwrap());
    assert_eq!(storage.search("durable", 10, None, None).unwrap().len(), 1);
    storage
        .record_source_checkpoint(
            Provider::Codex,
            &SourceIdentity::file("/tmp/history.jsonl"),
            &crate::ingestion::SourceStamp::file(100, 200),
        )
        .unwrap();
    storage.commit_writer().unwrap();
    drop(storage);
    let storage = Storage::open(&path).unwrap();
    assert!(
        storage
            .source_checkpoint_matches(
                Provider::Codex,
                &SourceIdentity::file("/tmp/history.jsonl"),
                &crate::ingestion::SourceStamp::file(100, 200)
            )
            .unwrap()
    );
    assert_eq!(storage.counts().unwrap().messages, 1);
}

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
        .record_source_checkpoint(
            crate::provider::Provider::Codex,
            &crate::ingestion::SourceIdentity::file("/tmp/old.jsonl"),
            &crate::ingestion::SourceStamp::file(10, 20),
        )
        .unwrap();
    storage.commit_writer().unwrap();
    storage.connection.execute_batch("ALTER TABLE conversations DROP COLUMN project; ALTER TABLE conversations DROP COLUMN repository; PRAGMA user_version=14;").unwrap();
    drop(storage);
    let storage = Storage::open(&path).unwrap();
    assert_eq!(storage.counts().unwrap().messages, 1);
    assert_eq!(storage.embedding_count("fixture-generation").unwrap(), 1);
    assert!(
        !storage
            .source_checkpoint_matches(
                crate::provider::Provider::Codex,
                &crate::ingestion::SourceIdentity::file("/tmp/old.jsonl"),
                &crate::ingestion::SourceStamp::file(10, 20)
            )
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
            &crate::ingestion::SourceIdentity::file("/tmp/source.jsonl"),
            &crate::ingestion::SourceStamp::file(10, 20),
        )
        .unwrap();
    storage.commit_writer().unwrap();
    storage
        .connection
        .execute_batch("PRAGMA user_version=16")
        .unwrap();
    drop(storage);
    let storage = Storage::open(&path).unwrap();
    assert_eq!(storage.counts().unwrap().messages, 1);
    assert_eq!(storage.embedding_count("fixture").unwrap(), 1);
    assert!(
        !storage
            .source_checkpoint_matches(
                crate::provider::Provider::Codex,
                &crate::ingestion::SourceIdentity::file("/tmp/source.jsonl"),
                &crate::ingestion::SourceStamp::file(10, 20)
            )
            .unwrap()
    );
    assert_eq!(
        storage.view(&conversation.messages[0].id, 0).unwrap()[0].content,
        "preserved request"
    );
    assert!(Storage::open_search(&path, "fixture").is_err());
}

#[test]
fn provider_schema_upgrade_preserves_metadata_and_requires_explicit_refresh() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("old.sqlite3");
    let mut storage = Storage::open_writer(&path).unwrap();
    let mut value = conversation("keep this history");
    value.project = Some("/work/importer".into());
    value.repository = Some("https://example.test/importer.git".into());
    storage.replace_conversation(&value).unwrap();
    storage.commit_writer().unwrap();
    // Recreate the previous provider CHECK while retaining the same metadata.
    let schema: String = storage
        .connection
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name='conversations'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    storage
        .connection
        .execute_batch("PRAGMA writable_schema=ON")
        .unwrap();
    storage
        .connection
        .execute(
            "UPDATE sqlite_schema SET sql=?1 WHERE name='conversations'",
            [schema.replace(", 'aider'", "")],
        )
        .unwrap();
    storage
        .connection
        .execute_batch("PRAGMA writable_schema=OFF; PRAGMA user_version=17")
        .unwrap();
    drop(storage);
    let storage = Storage::open(&path).unwrap();
    let metadata: (String, String) = storage
        .connection
        .query_row("SELECT project,repository FROM conversations", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    assert_eq!(
        metadata,
        (value.project.unwrap(), value.repository.unwrap())
    );
    assert_eq!(
        storage.view(&value.messages[0].id, 0).unwrap()[0].content,
        "keep this history"
    );
    assert!(Storage::open_search(&path, "fixture").is_err());
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
                VALUES ('unsupported-session', 'github-copilot', '/tmp/opencode.jsonl', 'source');
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
                VALUES ('github-copilot', '/tmp/opencode.jsonl', 10, 20);
             INSERT INTO tombstones(provider, conversation_id, forgotten_at)
                VALUES ('github-copilot', 'forgotten-opencode', 30);
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
        let query = format!("SELECT count(*) FROM {table} WHERE provider = 'github-copilot'");
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
    assert!(storage.derived_search_is_dirty().unwrap());
    assert!(!storage.semantic_index_is_ready("old-generation").unwrap());
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
    assert!(schema.contains("'opencode'"));
    for provider in ["github-copilot", "unknown-provider"] {
        let error = storage
            .connection
            .execute(
                "INSERT INTO conversations(id, provider, source_path, source_key)
                 VALUES (?1, ?2, ?3, ?3)",
                params![
                    format!("rejected-{provider}"),
                    provider,
                    format!("/tmp/rejected-{provider}.jsonl")
                ],
            )
            .expect_err("unsupported provider rejected");
        assert!(matches!(error, rusqlite::Error::SqliteFailure(_, _)));
    }
    for &provider in crate::provider::Provider::ALL {
        storage.connection.execute(
            "INSERT INTO conversations(id, provider, source_path, source_key) VALUES (?1, ?2, ?3, ?3)",
            params![format!("accepted-{provider}"),provider,format!("file:/tmp/{provider}.history")],
        ).expect("supported provider accepted after migration");
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
