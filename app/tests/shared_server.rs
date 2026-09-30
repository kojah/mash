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
        Self::start(root, group, nodes, false)
    }
    pub fn redis(root: &Path, group: &str, nodes: &[&str]) -> Self {
        Self::start(root, group, nodes, true)
    }
    fn start(root: &Path, group: &str, nodes: &[&str], redis: bool) -> Self {
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
        let config = if redis {
            toml::toml! {
                version = 1
                listen = "127.0.0.1:0"
                redis_url_env = "MASH_TEST_REDIS_URL"
                nodes = (credentials.clone())
            }
        } else {
            toml::toml! {
                version = 1
                listen = "127.0.0.1:0"
                backend = "postgres"
                database_url_env = "MASH_TEST_PG_URL"
                nodes = (credentials.clone())
            }
        };
        std::fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
        let mut command = std::process::Command::new(assert_cmd::cargo::cargo_bin!("mash"));
        command
            .args(["serve", "--server-config", path.to_str().unwrap()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
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
fn server_rejects_invalid_configuration_before_connecting() {
    assert_startup_rejected(b"\xff", &[], "must be UTF-8 TOML");
    assert_startup_rejected(&vec![b' '; 65_537], &[], "exceeds 65536 bytes");
    for (setting, message) in [
        ("version = 2", "requires version 1"),
        ("version = 1\nmax_connections = 0", "1..128 connections"),
        ("version = 1\nmax_connections = 129", "1..128 connections"),
        (
            "version = 1\nmax_concurrent_uploads = 0",
            "max_concurrent_uploads",
        ),
        (
            "version = 1\nmax_concurrent_uploads = 17",
            "max_concurrent_uploads",
        ),
        ("version = 1\nredis_max_node_mib = 0", "redis_max_node_mib"),
        (
            "version = 1\nredis_max_node_mib = 4097",
            "redis_max_node_mib",
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
fn server_rejects_backend_credential_configuration_mixups() {
    for (setting, message) in [
        ("", "Redis requires redis_url_env"),
        (
            "database_url_env = 'MASH_UNUSED_DATABASE'",
            "Redis backend does not accept database_url_env",
        ),
        (
            "backend = 'postgres'",
            "PostgreSQL requires database_url_env",
        ),
        (
            "backend = 'postgres'\nredis_url_env = 'MASH_UNUSED_REDIS'",
            "PostgreSQL backend does not accept redis_url_env",
        ),
        (
            "redis_url_env = '1INVALID'",
            "invalid credential environment variable name",
        ),
    ] {
        assert_startup_rejected(
            format!("version = 1\n{setting}{STARTUP_NODE}").as_bytes(),
            &[("MASH_STARTUP_TOKEN_A", STARTUP_TOKEN)],
            message,
        );
    }
}
