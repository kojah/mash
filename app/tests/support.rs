use super::{Command, Connection, Path, Value};

pub(super) use crate::test_support::create_valid_model_marker;

pub(super) fn run_json(arguments: &[&str]) -> Value {
    let output = Command::cargo_bin("mash")
        .expect("mash binary")
        .args(arguments)
        .output()
        .expect("run mash");
    assert!(
        output.status.success(),
        "mash failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("JSON response")
}

pub(super) fn error_kind(output: &std::process::Output) -> Value {
    serde_json::from_slice::<Value>(&output.stderr).unwrap_or_else(|error| {
        panic!(
            "typed error: {error}; status={:?}; stdout={}; stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })["error"]["kind"]
        .clone()
}

pub(super) fn seed_readiness_database(path: &Path, search_projection: Option<&str>) {
    let connection = Connection::open(path).expect("readiness database");
    connection
        .execute_batch(
            "CREATE TABLE conversations (
                id TEXT PRIMARY KEY, provider TEXT NOT NULL, source_path TEXT NOT NULL UNIQUE,
                title TEXT, created_at INTEGER, updated_at INTEGER, project TEXT, repository TEXT,
                source_fingerprint TEXT NOT NULL DEFAULT ''
             );
             CREATE TABLE messages (
                id TEXT PRIMARY KEY, conversation_id TEXT NOT NULL,
                ordinal INTEGER NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL,
                search_projection TEXT, created_at INTEGER,
                fingerprint TEXT NOT NULL DEFAULT '', UNIQUE(conversation_id, ordinal)
             );
             CREATE TABLE message_embeddings (
                message_id TEXT PRIMARY KEY, generation TEXT NOT NULL,
                dimensions INTEGER NOT NULL, norm REAL NOT NULL, vector BLOB NOT NULL
             );
             CREATE TABLE pending_embeddings (
                message_id TEXT PRIMARY KEY
             ) WITHOUT ROWID;
             CREATE VIRTUAL TABLE message_fts USING fts5(
                content, message_id UNINDEXED, conversation_id UNINDEXED,
                tokenize = 'unicode61'
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
             CREATE TABLE semantic_chunks (
                chunk_id INTEGER PRIMARY KEY, generation TEXT NOT NULL,
                dimensions INTEGER NOT NULL, vector_count INTEGER NOT NULL,
                message_rowids BLOB NOT NULL, norms BLOB NOT NULL,
                providers BLOB NOT NULL, created_ats BLOB NOT NULL, vectors BLOB NOT NULL
             );
             CREATE TABLE derived_state (
                singleton INTEGER PRIMARY KEY, search_dirty INTEGER NOT NULL,
                semantic_ready_generation TEXT
             );
             INSERT INTO derived_state VALUES (1, 0, NULL);
             INSERT INTO conversations(id, provider, source_path)
                VALUES ('session', 'codex', '/tmp/session.jsonl');
             PRAGMA user_version = 15;",
        )
        .expect("readiness schema");
    connection
        .execute(
            "INSERT INTO messages(
                id, conversation_id, ordinal, role, content, search_projection
             ) VALUES ('message', 'session', 0, 'tool', 'canonical tool output', ?1)",
            [search_projection],
        )
        .expect("readiness message");
}

pub(super) fn current_embedding_generation() -> String {
    let specification = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        concat!(
            "fastembed=6.0.1;model=AllMiniLML6V2Q;",
            "backend=coreml-fp32;batch=32;sequence=512;",
            "vector=i8-per-vector-symmetric;",
            "cosine=quantized-flat-exact;schema=4"
        )
    } else {
        concat!(
            "fastembed=6.0.1;model=AllMiniLML6V2Q;",
            "batch=8;workers=8;threads=2;",
            "vector=i8-per-vector-symmetric;",
            "cosine=quantized-flat-exact;schema=3"
        )
    };
    blake3::hash(specification.as_bytes()).to_hex().to_string()
}

pub(super) fn write_config(path: &Path, claude_root: &Path, codex_root: &Path) {
    std::fs::write(
        path,
        toml::to_string(&toml::toml! {
            version = 1
            providers = {
            claude-code = {roots = [(claude_root.to_str().unwrap())]},
            codex = {roots = [(codex_root.to_str().unwrap())]}
            }
            index = {since_days = 30}
        })
        .expect("configuration TOML"),
    )
    .expect("configuration file");
}

/// A local node with valid installation metadata and an initially absent index.
pub(super) struct NodeFixture {
    _directory: tempfile::TempDir,
    pub(super) database: std::path::PathBuf,
    pub(super) models: std::path::PathBuf,
}

impl NodeFixture {
    pub(super) fn new() -> Self {
        let directory = tempfile::tempdir().expect("node fixture");
        let database = directory.path().join("index.sqlite3");
        let models = directory.path().join("models");
        create_valid_model_marker(&models);
        Self {
            _directory: directory,
            database,
            models,
        }
    }

    pub(super) fn seed(&self, projection: Option<&str>) {
        seed_readiness_database(&self.database, projection);
    }

    pub(super) fn status(&self) -> Value {
        run_json(&[
            "--db",
            self.database.to_str().unwrap(),
            "--models-dir",
            self.models.to_str().unwrap(),
            "status",
        ])
    }

    pub(super) fn complete_index(&self) {
        Connection::open(&self.database)
            .unwrap()
            .execute(
                "UPDATE derived_state SET semantic_ready_generation = ?1 WHERE singleton = 1",
                [current_embedding_generation()],
            )
            .expect("completed index generation");
    }

    pub(super) fn insert_embedding(&self) {
        let connection = Connection::open(&self.database).expect("ready database");
        let generation = current_embedding_generation();
        connection
            .execute(
                "INSERT INTO message_embeddings(message_id, generation, dimensions, norm, vector)
             VALUES ('message', ?1, 1, 127.0, X'7F')",
                [&generation],
            )
            .expect("current embedding");
        connection
            .execute(
                "INSERT INTO semantic_chunks(
                chunk_id, generation, dimensions, vector_count, message_rowids,
                norms, providers, created_ats, vectors
             ) VALUES (0, ?1, 1, 1, ?2, ?3, X'02', ?4, X'7F')",
                rusqlite::params![
                    generation,
                    1_i64.to_le_bytes().to_vec(),
                    127.0_f32.to_le_bytes().to_vec(),
                    i64::MIN.to_le_bytes().to_vec()
                ],
            )
            .expect("packed semantic chunk");
    }
}
