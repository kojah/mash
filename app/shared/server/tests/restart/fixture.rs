//! Owned database containers and real API processes; no shared service restarts.
use serde::{Serialize, de::DeserializeOwned};
use std::io::BufRead;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

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
    name: String,
    removed: bool,
}

pub(super) struct Backup {
    collection: String,
    path: std::path::PathBuf,
}

#[derive(serde::Deserialize)]
struct SnapshotReply {
    result: SnapshotName,
}
#[derive(serde::Deserialize)]
struct SnapshotName {
    name: String,
}
#[derive(serde::Deserialize)]
struct CollectionsReply {
    result: Collections,
}
#[derive(serde::Deserialize)]
struct Collections {
    collections: Vec<CollectionName>,
}
#[derive(serde::Deserialize)]
struct CollectionName {
    name: String,
}
impl Database {
    pub(super) fn start() -> Self {
        let mut nonce = [0; 12];
        getrandom::fill(&mut nonce).unwrap();
        let image = "qdrant/qdrant:v1.19.1";
        let port = "6333";
        let database = Self {
            name: format!("mash-restart-qdrant-{}", hex::encode(nonce)),
            removed: false,
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
        args.extend(["--mount", "type=volume,dst=/qdrant/storage", image]);
        let result = docker(&args, 30);
        assert!(
            result.status.success(),
            "create Qdrant: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        database
    }

    pub(super) fn endpoint(&self) -> String {
        let port = docker(&["port", &self.name, "6333"], 5);
        assert!(port.status.success(), "container port");
        let port = String::from_utf8(port.stdout).unwrap();
        let port = port.trim().rsplit(':').next().unwrap();
        format!("http://127.0.0.1:{port}")
    }

    pub(super) async fn ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        loop {
            let ready = client
                .get(format!("{}/readyz", self.endpoint()))
                .send()
                .await
                .is_ok_and(|r| r.status().is_success());
            if ready {
                return;
            }
            assert!(Instant::now() < deadline, "Qdrant database readiness");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    pub(super) async fn restart(&self) {
        let result = docker(&["restart", "--time", "10", &self.name], 20);
        assert!(result.status.success(), "restart Qdrant");
        self.ready().await;
    }

    pub(super) async fn crash_and_restart(&self) {
        let result = docker(&["kill", "--signal", "KILL", &self.name], 10);
        assert!(result.status.success(), "kill Qdrant");
        let state = docker(
            &["inspect", "--format", "{{.State.Running}}", &self.name],
            5,
        );
        assert!(state.status.success(), "inspect crashed Qdrant");
        assert_eq!(state.stdout, b"false\n", "database must be stopped");
        let result = docker(&["start", &self.name], 20);
        assert!(result.status.success(), "start crashed Qdrant");
        self.ready().await;
    }

    /// Caller stops all owned writers first: collection snapshots are separate,
    /// and do not provide a cross-collection transaction during publication.
    pub(super) async fn backup(&self, root: &std::path::Path) -> Vec<Backup> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap();
        let url = self.endpoint();
        let collections: CollectionsReply = client
            .get(format!("{url}/collections"))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        let mut backups = Vec::new();
        for collection in collections.result.collections {
            let name = collection.name;
            let snapshot: SnapshotReply = client
                .post(format!("{url}/collections/{name}/snapshots?wait=true"))
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .json()
                .await
                .unwrap();
            let bytes = client
                .get(format!(
                    "{url}/collections/{name}/snapshots/{}",
                    snapshot.result.name
                ))
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .bytes()
                .await
                .unwrap();
            // Synthetic fixtures only; this is not a real-corpus backup driver.
            assert!(
                bytes.len() <= 8 * 1024 * 1024,
                "fixture snapshot exceeds bound"
            );
            let path = root.join(format!("{name}.snapshot"));
            std::fs::write(&path, bytes).unwrap();
            backups.push(Backup {
                collection: name,
                path,
            });
        }
        backups
    }

    pub(super) async fn restore(&self, backups: &[Backup]) {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap();
        let url = self.endpoint();
        for backup in backups {
            // Small owned binary fixtures; avoid adding a multipart dependency
            // or enabling local-file recovery on the database.
            let boundary = "mash-snapshot-fixture";
            let mut body = format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"snapshot\"; filename=\"backup.snapshot\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            ).into_bytes();
            let bytes = std::fs::read(&backup.path).unwrap();
            assert!(
                bytes.len() <= 8 * 1024 * 1024,
                "fixture snapshot exceeds bound"
            );
            body.extend(bytes);
            body.extend(format!("\r\n--{boundary}--\r\n").bytes());
            client
                .post(format!(
                    "{url}/collections/{}/snapshots/upload?priority=snapshot&wait=true",
                    backup.collection
                ))
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(body)
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap();
        }
    }

    pub(super) fn destroy(mut self) {
        let result = docker(&["rm", "-f", "-v", &self.name], 15);
        assert!(
            result.status.success(),
            "remove original database and volume"
        );
        self.removed = true;
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        if self.removed {
            return;
        }
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
        Self::launch(database, root, None).0
    }

    /// Start the API, optionally replaying gold from silver first; returns the
    /// JSON reports printed before the readiness line.
    pub(super) fn launch(
        database: &Database,
        root: &std::path::Path,
        replay_models: Option<&std::path::Path>,
    ) -> (Self, Vec<serde_json::Value>) {
        // Shared silver lives beside the config and survives database restarts.
        let config = format!(
            "version=1\nlisten='127.0.0.1:0'\nqdrant_url_env='MASH_RESTART_URL'\nsilver_database={silver:?}\n[[nodes]]\ngroup_id='restart-main'\nnode_id='first'\ntoken_env='MASH_RESTART_FIRST'\n[[nodes]]\ngroup_id='restart-main'\nnode_id='second'\ntoken_env='MASH_RESTART_SECOND'\n[[nodes]]\ngroup_id='restart-other'\nnode_id='first'\ntoken_env='MASH_RESTART_OTHER'\n",
            silver = root.join("shared-silver.sqlite3").to_str().unwrap()
        );
        let path = root.join("server.toml");
        std::fs::write(&path, config).unwrap();
        let mut command = std::process::Command::new(
            std::env::var_os("MASH_TEST_SERVER_BINARY")
                .expect("set MASH_TEST_SERVER_BINARY to the built mash binary"),
        );
        if let Some(models) = replay_models {
            command.arg("--models-dir").arg(models);
        }
        command.arg("serve").arg("--server-config").arg(path);
        if replay_models.is_some() {
            command.arg("--replay-gold");
        }
        let child = command
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
            let mut stderr = std::io::BufReader::new(stderr);
            loop {
                let mut line = String::new();
                if stderr.read_line(&mut line).unwrap() == 0 || sender.send(line).is_err() {
                    break;
                }
            }
        });
        let mut reports = Vec::new();
        loop {
            let line = receiver
                .recv_timeout(Duration::from_secs(120))
                .expect("API readiness");
            if let Ok(listening) = serde_json::from_str::<Listening>(&line) {
                api.url = format!("http://{}", listening.listening);
                return (api, reports);
            }
            reports.push(
                serde_json::from_str(&line).unwrap_or_else(|e| panic!("API startup: {e}: {line}")),
            );
        }
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
        self.upload_stream("index", token, events).await
    }
    pub(super) async fn upload_stream<T: Serialize>(
        &self,
        route: &str,
        token: &str,
        events: &[T],
    ) -> reqwest::Response {
        let mut body = Vec::new();
        for event in events {
            serde_json::to_writer(&mut body, event).unwrap();
            body.push(b'\n');
        }
        self.client
            .post(format!("{}/v1/{route}", self.url))
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
