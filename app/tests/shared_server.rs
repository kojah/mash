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
        Self::start(root, group, nodes, "dialogue", false, None)
    }
    pub fn with_recipe(root: &Path, group: &str, nodes: &[&str], recipe: &str) -> Self {
        Self::start(root, group, nodes, recipe, true, None)
    }
    fn start(
        root: &Path,
        group: &str,
        nodes: &[&str],
        recipe: &str,
        activate: bool,
        space: Option<(&str, usize)>,
    ) -> Self {
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
        let silver = root.join("silver").to_str().unwrap().to_owned();
        let mut config = toml::toml! {
            version = 1
            listen = "127.0.0.1:0"
            qdrant_url_env = "MASH_TEST_QDRANT_URL"
            silver_dir = silver
            nodes = (credentials)
        };
        config.insert(
            "views".into(),
            toml::toml! {build = {embedding_text = (recipe)}}.into(),
        );
        if let Some((model, dimensions)) = space {
            config.insert(
                "embeddings".into(),
                toml::toml! { model = (model) dimensions = (i64::try_from(dimensions).unwrap()) }
                    .into(),
            );
        }
        std::fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
        let mut command = std::process::Command::new(assert_cmd::cargo::cargo_bin!("mash"));
        command
            .args(["serve", "--server-config", path.to_str().unwrap()])
            .stdin(Stdio::piped())
            .env("MASH_TEST_EXIT_WITH_STDIN", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        if activate {
            command.arg("--activate-views");
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
        .timeout(super::HANG_GUARD);
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
fn server_rejects_unknown_views_recipe_and_fields_before_backend_access() {
    for recipe in ["embedding_text='typo'", "unknown=true"] {
        let config = format!("version=1\n{STARTUP_NODE}\n[views.build]\n{recipe}\n");
        assert_startup_rejected(config.as_bytes(), &[], "invalid server configuration");
    }
}

#[test]
fn server_rejects_relative_silver_dir_before_backend_access() {
    let config = format!("version=1\nsilver_dir='relative/silver'\n{STARTUP_NODE}");
    assert_startup_rejected(
        config.as_bytes(),
        &[("MASH_STARTUP_TOKEN_A", STARTUP_TOKEN)],
        "silver_dir must be an absolute path",
    );
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
            "mash_views_{}_{}",
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
async fn qdrant_server_views_activation_rejects_conflicting_startup_and_survives_restart() {
    let root = tempfile::tempdir().unwrap();
    let group = format!(
        "views-startup-{}",
        root.path().file_name().unwrap().to_string_lossy()
    );
    let path = root.path().join("server.toml");
    let serve = || {
        assert_cmd::Command::cargo_bin("mash")
            .unwrap()
            .args(["serve", "--server-config", path.to_str().unwrap()])
            .env("MASH_TEST_TOKEN_0", token("node"))
            .timeout(super::HANG_GUARD)
            .output()
            .unwrap()
    };
    let old = Server::new(root.path(), &group, &["node"]);
    // One API process owns a silver directory; a second fails before Qdrant.
    let second = serve();
    assert!(!second.status.success());
    assert_eq!(super::error_kind(&second), "configuration");
    assert!(String::from_utf8_lossy(&second.stderr).contains("already using"));
    drop(old);
    let document = std::fs::read_to_string(&path)
        .unwrap()
        .replace("dialogue", "title-and-dialogue");
    std::fs::write(&path, document).unwrap();
    let output = serve();
    assert!(!output.status.success());
    assert_eq!(super::error_kind(&output), "configuration");
    assert!(String::from_utf8_lossy(&output.stderr).contains("--activate-views"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(&token("node")));
    let active = Server::with_recipe(root.path(), &group, &["node"], "title-and-dialogue");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    let freshness = |url: String| {
        let client = client.clone();
        async move {
            client
                .get(format!("{url}/v1/freshness"))
                .bearer_auth(token("node"))
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .json::<serde_json::Value>()
                .await
                .unwrap()
        }
    };
    let before = freshness(active.url.clone()).await;
    assert_eq!(
        before["group_views"]["build"]["embedding_text"],
        "title-and-dialogue"
    );
    assert_eq!(before["nodes"], serde_json::json!([]));
    drop(active);
    let restarted = Server::start(
        root.path(),
        &group,
        &["node"],
        "title-and-dialogue",
        false,
        None,
    );
    assert_eq!(before, freshness(restarted.url.clone()).await);
    drop(restarted);
    for kind in ["state", "messages"] {
        client
            .delete(format!(
                "{}/collections/mash_views_{}_{kind}",
                std::env::var("MASH_TEST_QDRANT_URL")
                    .unwrap()
                    .trim_end_matches('/'),
                blake3::hash(group.as_bytes()).to_hex(),
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
        ("version = 1\nmax_upload_mib = 0", "max_upload_mib"),
        ("version = 1\nmax_upload_mib = 4097", "max_upload_mib"),
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
            "[views.query.vector_search]\nalgorithm='hnsw'\nef_runtime=0",
            "ef_runtime",
        ),
        (
            "[views.query.vector_search]\nalgorithm='exact'\nef_runtime=20",
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
            let collection = format!(
                "mash_views_{}_{kind}",
                blake3::hash(group.as_bytes()).to_hex()
            );
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

#[test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL"]
fn a_test_server_exits_when_its_test_goes_away() {
    let root = tempfile::tempdir().unwrap();
    let group = format!(
        "parent-exit-{}",
        root.path().file_name().unwrap().to_string_lossy()
    );
    let mut server = Server::new(root.path(), &group, &["node"]);
    // Closing stdin is what the kernel does when the test process dies.
    drop(server.child.stdin.take());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while server.child.try_wait().unwrap().is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "server outlived its test"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
