use std::io::BufRead;
use std::path::Path;
use std::process::{Child, Stdio};

pub fn token(node: &str) -> String {
    format!("mash-http-test-node-token-32-bytes-{node}")
}

pub struct Server {
    child: Child,
    pub url: String,
}
impl Server {
    pub fn new(root: &Path, group: &str, nodes: &[&str]) -> Self {
        Self::start(root, group, nodes, "dialogue", false)
    }
    pub fn with_recipe(root: &Path, group: &str, nodes: &[&str], recipe: &str) -> Self {
        Self::start(root, group, nodes, recipe, true)
    }
    fn start(root: &Path, group: &str, nodes: &[&str], recipe: &str, activate: bool) -> Self {
        let path = root.join("server.toml");
        let credentials: Vec<_> = nodes
            .iter()
            .enumerate()
            .map(|(i, node)| {
                toml::toml! {
                    group_id = (group)
                    node_id = (*node)
                    token_env = (format!("MASH_TEST_TOKEN_{i}"))
                }
            })
            .collect();
        let mut config = toml::toml! {
            version = 1
            listen = "127.0.0.1:0"
            qdrant_url_env = "MASH_TEST_QDRANT_URL"
            nodes = (credentials)
        };
        config.insert(
            "gold".into(),
            toml::toml! {build = {embedding_text = (recipe)}}.into(),
        );
        std::fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
        let mut command = std::process::Command::new(assert_cmd::cargo::cargo_bin!("mash"));
        command
            .args(["serve", "--server-config", path.to_str().unwrap()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        if activate {
            command.arg("--activate-gold");
        }
        for (i, node) in nodes.iter().enumerate() {
            command.env(format!("MASH_TEST_TOKEN_{i}"), token(node));
        }
        let mut child = command.spawn().unwrap();
        let stderr = child.stderr.take().unwrap();
        let mut server = Self {
            child,
            url: String::new(),
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            std::io::BufReader::new(stderr)
                .read_line(&mut line)
                .unwrap();
            let _ = sender.send(line);
        });
        let line = receiver
            .recv_timeout(std::time::Duration::from_secs(45))
            .expect("server startup");
        let response: serde_json::Value = serde_json::from_str(&line).unwrap();
        server.url = format!(
            "http://{}",
            response["listening"]
                .as_str()
                .unwrap_or_else(|| panic!("server startup failed: {line}"))
        );
        server
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn assert_startup_rejected(config: &[u8], tokens: &[(&str, &str)], message: &str) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("server.toml");
    std::fs::write(&path, config).unwrap();
    let mut command = assert_cmd::Command::cargo_bin("mash").unwrap();
    command
        .args(["serve", "--server-config", path.to_str().unwrap()])
        .env_remove("MASH_STARTUP_TOKEN_A")
        .env_remove("MASH_STARTUP_TOKEN_B")
        .timeout(std::time::Duration::from_secs(5));
    for (name, value) in tokens {
        command.env(name, value);
    }
    let output = command.output().unwrap();
    assert!(!output.status.success(), "server unexpectedly started");
    assert_eq!(super::error_kind(&output), "configuration");
    assert_eq!(output.stdout, [] as [u8; 0]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(message), "{stderr}");
    for (_, token) in tokens {
        assert!(!stderr.contains(token), "credential leaked in error");
    }
}

const STARTUP_NODE: &str = "\n[[nodes]]\ngroup_id = 'test-group'\nnode_id = 'test-node'\ntoken_env = 'MASH_STARTUP_TOKEN_A'\n";
const STARTUP_TOKEN: &str = "startup-test-credential-with-32-bytes";

#[test]
fn server_rejects_unknown_gold_recipe_and_fields_before_backend_access() {
    for recipe in ["embedding_text='typo'", "unknown=true"] {
        let config = format!("version=1\n{STARTUP_NODE}\n[gold.build]\n{recipe}\n");
        assert_startup_rejected(config.as_bytes(), &[], "invalid server configuration");
    }
}

#[test]
fn server_rejects_invalid_mutation_snapshot_budget_before_backend_access() {
    for budget in [0, 65537] {
        let config = format!("version=1\nmax_mutation_snapshot_mib={budget}\n{STARTUP_NODE}");
        assert_startup_rejected(
            config.as_bytes(),
            &[("MASH_STARTUP_TOKEN_A", STARTUP_TOKEN)],
            "max_mutation_snapshot_mib must be between 1 and 65536",
        );
    }
}

#[tokio::test]
#[ignore = "requires MASH_TEST_QDRANT_URL with Qdrant 1.19+"]
async fn qdrant_default_server_configuration_starts_and_creates_group_collections() {
    let root = tempfile::tempdir().unwrap();
    let group = format!(
        "default-startup-{}",
        root.path().file_name().unwrap().to_string_lossy()
    );
    let server = Server::new(root.path(), &group, &["node"]);
    let document = std::fs::read_to_string(root.path().join("server.toml")).unwrap();
    assert!(!document.contains("backend"));
    let origin = std::env::var("MASH_TEST_QDRANT_URL").unwrap();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    let mut statuses = Vec::new();
    for kind in ["state", "messages"] {
        let collection = format!(
            "mash_q_{}_{}",
            blake3::hash(group.as_bytes()).to_hex(),
            kind
        );
        let url = format!("{}/collections/{collection}", origin.trim_end_matches('/'));
        statuses.push(client.get(&url).send().await.unwrap().status());
        client
            .delete(&url)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
    }
    drop(server);
    assert!(statuses.iter().all(reqwest::StatusCode::is_success));
}

#[tokio::test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
async fn qdrant_server_gold_activation_rejects_conflicting_startup_and_survives_restart() {
    let root = tempfile::tempdir().unwrap();
    let group = format!(
        "gold-startup-{}",
        root.path().file_name().unwrap().to_string_lossy()
    );
    let old = Server::new(root.path(), &group, &["node"]);
    let path = root.path().join("server.toml");
    let document = std::fs::read_to_string(&path)
        .unwrap()
        .replace("dialogue", "title-and-dialogue");
    std::fs::write(&path, document).unwrap();
    let output = assert_cmd::Command::cargo_bin("mash")
        .unwrap()
        .args(["serve", "--server-config", path.to_str().unwrap()])
        .env("MASH_TEST_TOKEN_0", token("node"))
        .timeout(std::time::Duration::from_secs(10))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(super::error_kind(&output), "configuration");
    assert!(String::from_utf8_lossy(&output.stderr).contains("--activate-gold"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(&token("node")));
    let active = Server::with_recipe(root.path(), &group, &["node"], "title-and-dialogue");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    let stale = client
        .get(format!("{}/v1/state", old.url))
        .bearer_auth(token("node"))
        .send()
        .await
        .unwrap();
    assert_eq!(stale.status(), reqwest::StatusCode::CONFLICT);
    let before: serde_json::Value = client
        .get(format!("{}/v1/freshness", active.url))
        .bearer_auth(token("node"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        before["group_gold"]["build"]["embedding_text"],
        "title-and-dialogue"
    );
    assert_eq!(before["nodes"], serde_json::json!([]));
    drop(active);
    let restarted = Server::start(root.path(), &group, &["node"], "title-and-dialogue", false);
    let after: serde_json::Value = client
        .get(format!("{}/v1/freshness", restarted.url))
        .bearer_auth(token("node"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(before, after);
    drop(restarted);
    drop(old);
    for kind in ["state", "messages"] {
        client
            .delete(format!(
                "{}/collections/mash_q_{}_{}",
                std::env::var("MASH_TEST_QDRANT_URL")
                    .unwrap()
                    .trim_end_matches('/'),
                blake3::hash(group.as_bytes()).to_hex(),
                kind
            ))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
    }
}

#[test]
fn server_rejects_invalid_configuration_before_connecting() {
    assert_startup_rejected(b"\xff", &[], "must be UTF-8 TOML");
    assert_startup_rejected(&vec![b' '; 65_537], &[], "exceeds 65536 bytes");
    for (setting, message) in [
        ("version = 2", "requires version 1"),
        (
            "version = 1\nmax_concurrent_uploads = 0",
            "max_concurrent_uploads",
        ),
        (
            "version = 1\nmax_concurrent_uploads = 17",
            "max_concurrent_uploads",
        ),
        (
            "version = 1\nqdrant_max_node_mib = 0",
            "qdrant_max_node_mib",
        ),
        (
            "version = 1\nqdrant_max_node_mib = 4097",
            "qdrant_max_node_mib",
        ),
        (
            "version = 1\n[embeddings]\nmodel = 'test'\ndimensions = 0",
            "invalid server embedding",
        ),
        (
            "version = 1\n[embeddings]\nmodel = 'test'\ndimensions = 2049",
            "invalid server embedding",
        ),
        (
            "version = 1\n[embeddings]\nmodel = ' '\ndimensions = 384",
            "invalid server embedding",
        ),
    ] {
        assert_startup_rejected(format!("{setting}{STARTUP_NODE}").as_bytes(), &[], message);
    }
    assert_startup_rejected(b"version = 1\nnodes = []", &[], "configured nodes");
}

#[test]
fn server_rejects_missing_invalid_and_duplicate_credentials() {
    let config = format!("version = 1{STARTUP_NODE}");
    assert_startup_rejected(config.as_bytes(), &[], "set MASH_STARTUP_TOKEN_A");
    for token in [
        "short".to_owned(),
        "x".repeat(4097),
        format!("{STARTUP_TOKEN} "),
    ] {
        assert_startup_rejected(
            config.as_bytes(),
            &[("MASH_STARTUP_TOKEN_A", &token)],
            "printable ASCII",
        );
    }
    assert_startup_rejected(
        config
            .replace("MASH_STARTUP_TOKEN_A", "1INVALID")
            .as_bytes(),
        &[],
        "invalid credential environment variable name",
    );
    let duplicate_node = format!(
        "{config}{}",
        STARTUP_NODE.replace("MASH_STARTUP_TOKEN_A", "MASH_STARTUP_TOKEN_B")
    );
    assert_startup_rejected(
        duplicate_node.as_bytes(),
        &[
            ("MASH_STARTUP_TOKEN_A", STARTUP_TOKEN),
            (
                "MASH_STARTUP_TOKEN_B",
                "different-startup-credential-32-bytes",
            ),
        ],
        "must be unique",
    );
    let duplicate_token = duplicate_node.replace(
        "node_id = 'test-node'\ntoken_env = 'MASH_STARTUP_TOKEN_B'",
        "node_id = 'other-node'\ntoken_env = 'MASH_STARTUP_TOKEN_B'",
    );
    assert_startup_rejected(
        duplicate_token.as_bytes(),
        &[
            ("MASH_STARTUP_TOKEN_A", STARTUP_TOKEN),
            ("MASH_STARTUP_TOKEN_B", STARTUP_TOKEN),
        ],
        "must be unique",
    );
}

#[test]
fn server_rejects_removed_backends_and_settings() {
    for setting in [
        "backend = 'postgres'",
        "backend = 'redis'",
        "backend = 'qdrant'",
        "database_url_env = 'MASH_UNUSED'",
        "redis_url_env = 'MASH_UNUSED'",
        "max_connections = 8",
        "redis_max_node_mib = 64",
        "[redis_vector_index]\nalgorithm = 'hnsw'",
        "[qdrant_vector_index]\nalgorithm = 'exact'",
    ] {
        assert_startup_rejected(
            format!("version=1\n{setting}{STARTUP_NODE}").as_bytes(),
            &[],
            "invalid server configuration",
        );
    }
}
#[test]
fn qdrant_server_rejects_invalid_settings_before_network_access() {
    for (setting, message) in [
        ("", "Qdrant requires qdrant_url_env"),
        ("qdrant_memory = 'disk'", "invalid server configuration"),
        ("qdrant_url_env = '1INVALID'", "invalid credential"),
        (
            "[gold.query.vector_search]\nalgorithm='hnsw'\nef_runtime=0",
            "ef_runtime",
        ),
        (
            "[gold.query.vector_search]\nalgorithm='exact'\nef_runtime=20",
            "invalid server configuration",
        ),
    ] {
        assert_startup_rejected(
            format!("version=1\n{setting}{STARTUP_NODE}").as_bytes(),
            &[("MASH_STARTUP_TOKEN_A", STARTUP_TOKEN)],
            message,
        );
    }
}

pub fn cleanup_group(group: &str) {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let origin = std::env::var("MASH_TEST_QDRANT_URL").unwrap();
        let client = reqwest::Client::new();
        for kind in ["state", "messages"] {
            let collection = format!("mash_q_{}_{kind}", blake3::hash(group.as_bytes()).to_hex());
            client
                .delete(format!(
                    "{}/collections/{collection}",
                    origin.trim_end_matches('/')
                ))
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap();
        }
    });
}
