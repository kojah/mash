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
                backend = "redis"
                redis_url_env = "MASH_TEST_REDIS_URL"
                nodes = (credentials.clone())
            }
        } else {
            toml::toml! {
                version = 1
                listen = "127.0.0.1:0"
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
