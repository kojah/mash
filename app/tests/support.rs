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

pub(super) fn seed_readiness_database(path: &Path, dialogue_text: Option<&str>) {
    // Initialize through the real command so fixtures cannot drift from the
    // production schema or pretend an older schema is search-ready.
    let setup = tempfile::tempdir().expect("readiness setup");
    let config = setup.path().join("config.toml");
    let models = setup.path().join("models");
    std::fs::write(&config, "version = 1\nproviders = {}\n").unwrap();
    create_valid_model_marker(&models);
    run_json(&[
        "--config",
        config.to_str().unwrap(),
        "--db",
        path.to_str().unwrap(),
        "--models-dir",
        models.to_str().unwrap(),
        "index",
    ]);
    let connection = Connection::open(path).expect("readiness database");
    connection.execute_batch(
        "UPDATE derived_state SET search_dirty=0,semantic_ready_generation=NULL WHERE singleton=1;
         INSERT INTO conversations(id,provider,source_path,source_key)
         VALUES ('session','codex','/tmp/session.jsonl','file:/tmp/session.jsonl');"
    ).expect("readiness history");
    connection
        .execute(
            "INSERT INTO messages(
                id, conversation_id, ordinal, role, content, search_projection
             ) VALUES ('message', 'session', 0, 'tool', 'canonical tool output', ?1)",
            [dialogue_text],
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
        let connection = Connection::open(&self.database).unwrap();
        // Synthetic rows inserted after empty-index setup need explicit silver
        // evidence and a manifest revision as well as the vector-ready marker.
        connection.execute_batch(
            "INSERT INTO silver_scans(provider,interpretation_version,state) VALUES('codex',1,1)
             ON CONFLICT(provider) DO UPDATE SET interpretation_version=1,state=1;
             UPDATE gold_manifest SET manifest=json_set(manifest,
                 '$.silver.revision',(SELECT revision FROM silver_state WHERE singleton=1),
                 '$.silver.completeness','complete') WHERE singleton=1;"
        ).unwrap();
        connection
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
