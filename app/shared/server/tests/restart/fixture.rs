//! Owned database containers and real API processes; no shared service restarts.
use serde::{Serialize, de::DeserializeOwned};
use std::io::BufRead;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub(super) enum Backend {
    Postgres,
    Redis,
    Qdrant,
}
impl Backend {
    fn settings(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Postgres => ("postgres", "pgvector/pgvector:pg17", "5432"),
            Self::Redis => ("redis", "redis:8.2", "6379"),
            Self::Qdrant => ("qdrant", "qdrant/qdrant:v1.19.1", "6333"),
        }
    }
}

fn docker(args: &[&str], timeout: u64) -> std::process::Output {
    let mut command = if std::env::var("MASH_TEST_DOCKER_SG").as_deref() == Ok("1") {
        let quoted = args
            .iter()
            .map(|arg| format!("'{}'", arg.replace('\'', "'\\''")))
            .collect::<Vec<_>>()
            .join(" ");
        let mut command = assert_cmd::Command::new("sg");
        command.args(["docker", "-c", &format!("docker {quoted}")]);
        command
    } else {
        let mut command = assert_cmd::Command::new("docker");
        command.args(args);
        command
    };
    command.timeout(Duration::from_secs(timeout));
    command.output().expect("Docker command")
}

pub(super) struct Database {
    pub(super) backend: Backend,
    name: String,
}
impl Database {
    pub(super) fn start(backend: Backend) -> Self {
        let mut nonce = [0; 12];
        getrandom::fill(&mut nonce).unwrap();
        let (label, image, port) = backend.settings();
        let database = Self {
            backend,
            name: format!("mash-restart-{label}-{}", hex::encode(nonce)),
        };
        let publication = format!("127.0.0.1::{port}");
        let owner = format!(
            "mash.test.owner={}",
            std::env::var("MASH_TEST_RESTART_OWNER").unwrap_or_else(|_| database.name.clone())
        );
        let mut args = vec![
            "run",
            "-d",
            "--pull=never",
            "--name",
            &database.name,
            "--label",
            "mash.test=backend-restart",
            "--label",
            &owner,
            "--cpus",
            "2",
            "--memory",
            "512m",
            "-p",
            &publication,
        ];
        match backend {
            Backend::Postgres => args.extend([
                "-e",
                "POSTGRES_PASSWORD=mash-restart-test-password",
                "-e",
                "POSTGRES_DB=mash_test",
                image,
            ]),
            Backend::Redis => args.extend([
                image,
                "redis-server",
                "--appendonly",
                "yes",
                "--appendfsync",
                "always",
                "--maxmemory-policy",
                "noeviction",
            ]),
            Backend::Qdrant => args.extend(["--mount", "type=volume,dst=/qdrant/storage", image]),
        }
        let result = docker(&args, 30);
        assert!(
            result.status.success(),
            "create {backend:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        database
    }

    pub(super) fn endpoint(&self) -> String {
        let port = docker(&["port", &self.name, self.backend.settings().2], 5);
        assert!(port.status.success(), "container port");
        let port = String::from_utf8(port.stdout).unwrap();
        let port = port.trim().rsplit(':').next().unwrap();
        match self.backend {
            Backend::Postgres => format!(
                "postgresql://postgres:mash-restart-test-password@127.0.0.1:{port}/mash_test?sslmode=disable"
            ),
            Backend::Redis => format!("redis://127.0.0.1:{port}/"),
            Backend::Qdrant => format!("http://127.0.0.1:{port}"),
        }
    }

    pub(super) async fn ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        loop {
            let ready = match self.backend {
                Backend::Qdrant => client
                    .get(format!("{}/readyz", self.endpoint()))
                    .send()
                    .await
                    .is_ok_and(|r| r.status().is_success()),
                Backend::Postgres => docker(
                    &[
                        "exec",
                        &self.name,
                        "pg_isready",
                        "-h",
                        "127.0.0.1",
                        "-U",
                        "postgres",
                        "-d",
                        "mash_test",
                    ],
                    3,
                )
                .status
                .success(),
                Backend::Redis => {
                    let result = docker(&["exec", &self.name, "redis-cli", "--raw", "PING"], 3);
                    result.status.success() && result.stdout == b"PONG\n"
                }
            };
            if ready {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "database readiness {:?}",
                self.backend
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    pub(super) async fn restart(&self) {
        let result = docker(&["restart", "--time", "10", &self.name], 20);
        assert!(result.status.success(), "restart {:?}", self.backend);
        self.ready().await;
    }

    pub(super) async fn crash_and_restart(&self) {
        let result = docker(&["kill", "--signal", "KILL", &self.name], 10);
        assert!(result.status.success(), "kill {:?}", self.backend);
        let state = docker(
            &["inspect", "--format", "{{.State.Running}}", &self.name],
            5,
        );
        assert!(state.status.success(), "inspect crashed {:?}", self.backend);
        assert_eq!(state.stdout, b"false\n", "database must be stopped");
        let result = docker(&["start", &self.name], 20);
        assert!(result.status.success(), "start crashed {:?}", self.backend);
        self.ready().await;
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        let result = docker(&["rm", "-f", "-v", &self.name], 15);
        if !result.status.success() {
            eprintln!("owned restart fixture cleanup failed: {}", self.name);
        }
    }
}

pub(super) struct Api {
    child: Child,
    url: String,
    client: reqwest::Client,
}
#[derive(serde::Deserialize)]
struct Listening {
    listening: std::net::SocketAddr,
}
impl Api {
    pub(super) fn start(database: &Database, root: &std::path::Path) -> Self {
        let label = database.backend.settings().0;
        let setting = match database.backend {
            Backend::Postgres => "database_url_env",
            Backend::Redis => "redis_url_env",
            Backend::Qdrant => "qdrant_url_env",
        };
        let config = format!(
            "version=1\nlisten='127.0.0.1:0'\nbackend='{label}'\n{setting}='MASH_RESTART_URL'\n[[nodes]]\ngroup_id='restart-main'\nnode_id='first'\ntoken_env='MASH_RESTART_FIRST'\n[[nodes]]\ngroup_id='restart-main'\nnode_id='second'\ntoken_env='MASH_RESTART_SECOND'\n[[nodes]]\ngroup_id='restart-other'\nnode_id='first'\ntoken_env='MASH_RESTART_OTHER'\n"
        );
        let path = root.join("server.toml");
        std::fs::write(&path, config).unwrap();
        let child = std::process::Command::new(
            std::env::var_os("MASH_TEST_SERVER_BINARY")
                .expect("set MASH_TEST_SERVER_BINARY to the built mash binary"),
        )
        .arg("serve")
        .arg("--server-config")
        .arg(path)
        .env("MASH_RESTART_URL", database.endpoint())
        .env("MASH_RESTART_FIRST", super::FIRST)
        .env("MASH_RESTART_SECOND", super::SECOND)
        .env("MASH_RESTART_OTHER", super::OTHER)
        .env_remove("MASH_TRACE")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
        let mut api = Self {
            child,
            url: String::new(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
        };
        let stderr = api.child.stderr.take().unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            std::io::BufReader::new(stderr)
                .read_line(&mut line)
                .unwrap();
            let _ = sender.send(line);
        });
        let line = receiver
            .recv_timeout(Duration::from_secs(30))
            .expect("API readiness");
        let listening: Listening =
            serde_json::from_str(&line).unwrap_or_else(|e| panic!("API startup: {e}: {line}"));
        api.url = format!("http://{}", listening.listening);
        api
    }

    pub(super) async fn post<T: Serialize>(
        &self,
        route: &str,
        token: &str,
        body: &T,
    ) -> reqwest::Response {
        self.client
            .post(format!("{}/v1/{route}", self.url))
            .bearer_auth(token)
            .json(body)
            .send()
            .await
            .unwrap()
    }
    pub(super) async fn post_json<T: Serialize, R: DeserializeOwned>(
        &self,
        route: &str,
        token: &str,
        body: &T,
    ) -> R {
        self.post(route, token, body)
            .await
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    pub(super) async fn get<T: DeserializeOwned>(&self, route: &str, token: &str) -> T {
        self.client
            .get(format!("{}/v1/{route}", self.url))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    pub(super) async fn upload(
        &self,
        token: &str,
        events: &[crate::shared::protocol::Event],
    ) -> reqwest::Response {
        let mut body = Vec::new();
        for event in events {
            serde_json::to_writer(&mut body, event).unwrap();
            body.push(b'\n');
        }
        self.client
            .post(format!("{}/v1/index", self.url))
            .bearer_auth(token)
            .header("content-type", "application/x-ndjson")
            .body(body)
            .send()
            .await
            .unwrap()
    }
}
impl Drop for Api {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
