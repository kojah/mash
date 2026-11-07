use super::{Command, Connection, Path, Value};

pub(super) use crate::test_support::{HANG_GUARD, create_valid_model_marker};

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
    let connection = open_silver(path);
    connection
        .execute_batch(include_str!("../storage/silver.sql"))
        .unwrap();
    connection
        .execute_batch(include_str!("../storage/silver_indexes.sql"))
        .unwrap();
    for schema in [
        include_str!("../storage/records.sql"),
        include_str!("../storage/record_provenance.sql"),
        include_str!("../storage/record_extractions.sql"),
    ] {
        connection.execute_batch(schema).unwrap();
    }
    let version = run_json(&["--version"])["silver_schema"].as_i64().unwrap();
    connection
        .pragma_update(None, "user_version", version)
        .unwrap();
    connection
        .execute_batch(
            "UPDATE silver_state SET interpretation_version=3 WHERE singleton=1;
         INSERT INTO chats(id,provider,source_path,source_key)
         VALUES ('session','codex','/tmp/session.jsonl','file:/tmp/session.jsonl');",
        )
        .expect("readiness history");
    connection
        .execute(
            "INSERT INTO messages(id,chat_id,ordinal,role,content_packed,search_projection)
             VALUES('message','session',0,'tool','canonical tool output',?1)",
            [dialogue_text],
        )
        .expect("readiness message");
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
    pub(super) config: std::path::PathBuf,
}

impl NodeFixture {
    pub(super) fn new() -> Self {
        let directory = tempfile::tempdir().expect("node fixture");
        let database = directory.path().join("index.sqlite3");
        let models = directory.path().join("models");
        let config = directory.path().join("config.toml");
        std::fs::write(&config, "version=1\nproviders={}\n").unwrap();
        create_valid_model_marker(&models);
        Self {
            _directory: directory,
            database,
            models,
            config,
        }
    }

    pub(super) fn seed(&self, projection: Option<&str>) {
        seed_readiness_database(&self.database, projection);
    }

    pub(super) fn status(&self) -> Value {
        run_json(&[
            "--config",
            self.config.to_str().unwrap(),
            "--db",
            self.database.to_str().unwrap(),
            "--models-dir",
            self.models.to_str().unwrap(),
            "status",
        ])
    }

    pub(super) fn complete_index(&self) {
        // This fixture owns synthetic canonical rows, so it supplies explicit scan
        // evidence. Gold and its certificate are produced only by the real CLI.
        open_silver(&self.database)
            .execute_batch(
                "UPDATE silver_state SET interpretation_version=3 WHERE singleton=1;
             INSERT INTO silver_scans(provider,interpretation_version,state)
             VALUES('codex',3,1)
             ON CONFLICT(provider) DO UPDATE SET interpretation_version=3,state=1;",
            )
            .unwrap();
        configure_qdrant(&self.config);
        run_json(&[
            "--config",
            self.config.to_str().unwrap(),
            "--db",
            self.database.to_str().unwrap(),
            "--models-dir",
            self.models.to_str().unwrap(),
            "index",
            "--rebuild-local-views",
        ]);
    }
}

/// Retired filename used only to prove old SQLite gold is ignored.
pub(super) fn views_path(silver: &Path) -> std::path::PathBuf {
    let name = silver.file_name().unwrap().to_string_lossy().into_owned();
    silver.with_file_name(name.strip_suffix(".sqlite3").map_or_else(
        || format!("{name}.views"),
        |stem| format!("{stem}.views.sqlite3"),
    ))
}

/// A raw silver connection that can read packed `content`.
pub(super) fn open_silver(path: impl AsRef<Path>) -> Connection {
    let connection = Connection::open(path).expect("local silver");
    mash::register_silver_codec(&connection).expect("silver codec");
    connection
}

pub(super) fn configure_qdrant(config: &Path) {
    let mut document: toml::Value =
        toml::from_str(&std::fs::read_to_string(config).unwrap()).unwrap();
    let qdrant = toml::Value::try_from(toml::toml! {
        url = (std::env::var("MASH_TEST_QDRANT_URL").expect("disposable Qdrant REST endpoint"))
        grpc_url = (std::env::var("MASH_TEST_QDRANT_GRPC_URL").expect("disposable Qdrant gRPC endpoint"))
    }).unwrap();
    document
        .as_table_mut()
        .unwrap()
        .insert("qdrant".into(), qdrant);
    std::fs::write(config, toml::to_string(&document).unwrap()).unwrap();
}

pub(super) fn cleanup_local_gold(database: &Path, node: &str) {
    super::shared_server::cleanup_group(&local_gold_group(database, node));
}

fn local_gold_group(database: &Path, node: &str) -> String {
    let instance: String = open_silver(database)
        .query_row("SELECT instance FROM local_database_identity", [], |row| {
            row.get(0)
        })
        .unwrap();
    let path = std::fs::canonicalize(database).unwrap();
    let mut hash = blake3::Hasher::new();
    hash.update(instance.as_bytes());
    hash.update(&[0]);
    hash.update(path.as_os_str().as_encoded_bytes());
    let node_hash = blake3::hash(node.as_bytes()).to_hex();
    format!("local-{}-{}", hash.finalize().to_hex(), &node_hash[..16])
}

/// Inspect complete persisted points to compare incremental builds with replay.
pub(super) fn local_gold_points(database: &Path, node: &str) -> Vec<Value> {
    let group = local_gold_group(database, node);
    let collection = format!(
        "mash_views_{}_messages",
        blake3::hash(group.as_bytes()).to_hex()
    );
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let origin = std::env::var("MASH_TEST_QDRANT_URL").unwrap();
        let client = reqwest::Client::new();
        let mut points = Vec::new();
        let mut offset = Value::Null;
        loop {
            let response: Value = client.post(format!("{origin}/collections/{collection}/points/scroll"))
                .json(&serde_json::json!({"limit":100,"with_payload":true,"with_vector":true,"offset":offset}))
                .send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
            points.extend(response["result"]["points"].as_array().unwrap().iter().cloned());
            offset = response["result"]["next_page_offset"].clone();
            if offset.is_null() { break; }
        }
        points.sort_by_key(|point| point["id"].to_string());
        points
    })
}
