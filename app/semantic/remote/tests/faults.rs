//! Exercise actual HTTP body failures, including indexing readiness and retry.
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::{Api, Client, Configuration};
use crate::core::{IndexRequest, Operation, Paths, Request, execute};
use crate::storage::Storage;

#[derive(Clone, Copy, Debug)]
enum Fault {
    Oversized,
    Truncated,
}

impl Fault {
    fn retryable(self) -> bool {
        matches!(self, Self::Truncated)
    }

    fn message(self) -> &'static str {
        match self {
            Self::Oversized => "remote embedding response exceeds 2 MiB",
            Self::Truncated => "read remote embedding response",
        }
    }
}

fn configuration(url: String) -> Configuration {
    Configuration {
        url,
        model: "fault-contract".into(),
        dimensions: 2,
        api: Api::OpenaiCompatible,
        token_env: None,
        request_dimensions: false,
    }
}

async fn read_request(stream: &mut TcpStream) {
    let mut bytes = Vec::new();
    loop {
        let mut buffer = [0; 4096];
        let count = stream.read(&mut buffer).await.unwrap();
        assert_ne!(count, 0, "client closed before sending the request");
        bytes.extend_from_slice(&buffer[..count]);
        assert!(bytes.len() < 64 * 1024, "unexpected fixture request size");
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = std::str::from_utf8(&bytes[..end]).unwrap();
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .expect("fixture client must send Content-Length");
            if bytes.len() >= end + 4 + length {
                return;
            }
        }
    }
}

async fn fault_server(fault: Fault) -> (Configuration, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = configuration(format!(
        "http://{}/embeddings",
        listener.local_addr().unwrap()
    ));
    let task = tokio::spawn(async move {
        // Indexing may spend time on SQLite/filesystem setup before connecting.
        // Bound the HTTP exchange; nextest bounds the complete indexing test.
        let (mut stream, _) = listener.accept().await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            read_request(&mut stream).await;
            match fault {
                Fault::Oversized => {
                    stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").await.unwrap();
                    let mut chunk = vec![b'x'; 64 * 1024];
                    chunk[..21].copy_from_slice(b"private-provider-body");
                    for _ in 0..33 {
                        // The client may close as soon as it enforces its limit.
                        if stream.write_all(b"10000\r\n").await.is_err()
                            || stream.write_all(&chunk).await.is_err()
                            || stream.write_all(b"\r\n").await.is_err()
                        {
                            return;
                        }
                    }
                    let _ = stream.write_all(b"0\r\n\r\n").await;
                }
                Fault::Truncated => {
                    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10000\r\nConnection: close\r\n\r\nprivate-provider-body").await.unwrap();
                    stream.shutdown().await.unwrap();
                }
            }
        }).await.expect("HTTP fault fixture timed out");
    });
    (config, task)
}

async fn assert_fault(fault: Fault) {
    let (config, task) = fault_server(fault).await;
    let error = Client::new(config)
        .unwrap()
        .embed(&["query".into()], true)
        .await
        .unwrap_err();
    assert_eq!(error.error.kind, crate::ErrorKind::RemoteInference);
    assert_eq!(error.error.retryable, fault.retryable());
    assert_eq!(error.error.message, fault.message());
    assert!(
        !serde_json::to_string(&error)
            .unwrap()
            .contains("private-provider-body")
    );
    task.await.unwrap();
}

#[tokio::test]
async fn oversized_stream_is_bounded_and_error_is_sanitized() {
    assert_fault(Fault::Oversized).await;
}

#[tokio::test]
async fn interrupted_body_is_retryable_and_error_is_sanitized() {
    assert_fault(Fault::Truncated).await;
}

#[tokio::test]
async fn failed_remote_bodies_never_publish_readiness_and_indexing_can_resume() {
    for fault in [Fault::Oversized, Fault::Truncated] {
        let temp = tempfile::tempdir().unwrap();
        let history = temp.path().join("history");
        std::fs::create_dir(&history).unwrap();
        std::fs::write(history.join("session.jsonl"), concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"session\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"searchable history\"}]}}\n"
        )).unwrap();
        let paths = Paths {
            config: Some(temp.path().join("config.toml")),
            db: Some(temp.path().join("mash.sqlite3")),
            models_dir: Some(temp.path().join("no-models")),
        };
        let write_config = |remote: &Configuration| {
            let document = toml::toml! {
                version = 1
                providers = {codex = {roots = [(history.to_str().unwrap())]}}
                index = {all_history = true}
                embeddings = (toml::Value::try_from(remote).unwrap())
            };
            std::fs::write(
                paths.config.as_ref().unwrap(),
                toml::to_string(&document).unwrap(),
            )
            .unwrap();
        };
        let request = Request {
            paths: paths.clone(),
            operation: Operation::Index(IndexRequest::default()),
        };
        let (remote, task) = fault_server(fault).await;
        write_config(&remote);
        let error = execute(request.clone()).await.unwrap_err();
        assert_eq!(error.error.kind, crate::ErrorKind::RemoteInference);
        assert_eq!(error.error.retryable, fault.retryable());
        assert_eq!(error.error.message, fault.message());
        task.await.unwrap();
        let generation = crate::semantic::model_space("fault-contract", 2);
        {
            let storage = Storage::open_existing(paths.db.as_ref().unwrap()).unwrap();
            assert_eq!(storage.counts().unwrap().embeddings, 0);
            assert!(!storage.semantic_index_is_ready(&generation).unwrap());
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let healthy = configuration(format!(
            "http://{}/embeddings",
            listener.local_addr().unwrap()
        ));
        let server = tokio::spawn(async move {
            let router = axum::Router::new().route(
                "/embeddings",
                axum::routing::post(|| async {
                    axum::response::Response::builder()
                        .header("content-type", "application/json")
                        .body(axum::body::Body::from(
                            r#"{"data":[{"index":0,"embedding":[1,0]}]}"#,
                        ))
                        .unwrap()
                }),
            );
            axum::serve(listener, router).await.unwrap();
        });
        write_config(&healthy);
        execute(request).await.unwrap();
        server.abort();
        let storage = Storage::open_existing(paths.db.as_ref().unwrap()).unwrap();
        assert_eq!(storage.counts().unwrap().messages, 1);
        assert_eq!(storage.counts().unwrap().embeddings, 1);
        assert!(storage.semantic_index_is_ready(&generation).unwrap());
        assert!(!paths.models_dir.unwrap().exists());
    }
}
