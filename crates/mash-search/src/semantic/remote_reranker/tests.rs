use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn reply(request_id: &[u8], entries: &[(u32, f32)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(RESPONSE_MAGIC);
    bytes.extend_from_slice(request_id);
    bytes.extend_from_slice(&9u32.to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(entries.len()).unwrap().to_le_bytes());
    for (id, score) in entries {
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(&score.to_le_bytes());
    }
    bytes
}

struct Decoded {
    recipe: String,
    request_id: Vec<u8>,
    query: String,
    documents: Vec<(u32, String)>,
}

fn decode(bytes: &[u8]) -> Decoded {
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    assert_eq!(&bytes[..4], REQUEST_MAGIC);
    let recipe_length = usize::from(u16::from_le_bytes(bytes[4..6].try_into().unwrap()));
    let recipe = std::str::from_utf8(&bytes[6..6 + recipe_length])
        .unwrap()
        .to_owned();
    let mut cursor = 6 + recipe_length;
    let request_id = bytes[cursor..cursor + REQUEST_ID_BYTES].to_vec();
    cursor += REQUEST_ID_BYTES;
    let length = usize::try_from(word(cursor)).unwrap();
    cursor += 4;
    let query = std::str::from_utf8(&bytes[cursor..cursor + length])
        .unwrap()
        .to_owned();
    cursor += length;
    let count = word(cursor);
    cursor += 4;
    let mut documents = Vec::new();
    for _ in 0..count {
        let id = word(cursor);
        let length = usize::try_from(word(cursor + 4)).unwrap();
        cursor += 8;
        documents.push((
            id,
            std::str::from_utf8(&bytes[cursor..cursor + length])
                .unwrap()
                .to_owned(),
        ));
        cursor += length;
    }
    assert_eq!(cursor, bytes.len());
    Decoded {
        recipe,
        request_id,
        query,
        documents,
    }
}

fn body(headers: &axum::http::HeaderMap, body: &[u8]) -> Vec<u8> {
    if headers.contains_key(axum::http::header::CONTENT_ENCODING) {
        zstd::stream::decode_all(body).unwrap()
    } else {
        body.to_vec()
    }
}

/// Scores each document by its length (at most 999), with entries reversed.
async fn scoring(headers: axum::http::HeaderMap, bytes: axum::body::Bytes) -> Vec<u8> {
    assert_eq!(headers["authorization"], "Bearer test-credential");
    let request = decode(&body(&headers, &bytes));
    assert_eq!(request.recipe, RECIPE);
    let entries = request
        .documents
        .iter()
        .rev()
        .map(|(id, text)| (*id, f32::from(u16::try_from(text.len().min(999)).unwrap())))
        .collect::<Vec<_>>();
    reply(&request.request_id, &entries)
}

struct Server {
    url: String,
    calls: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(router: axum::Router) -> Server {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let router = router.layer(axum::middleware::from_fn(
        move |request: axum::extract::Request, next: axum::middleware::Next| {
            counted.fetch_add(1, Ordering::SeqCst);
            next.run(request)
        },
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    Server { url, calls, task }
}

fn client(url: &str, timeout: Duration) -> Client {
    Client {
        http: http_client().unwrap(),
        url: url.to_owned(),
        token: "test-credential".into(),
        timeout,
    }
}

#[test]
fn remote_reranking_frames_are_bound_to_one_request() {
    let documents = ["first", "second"];
    let request = Request::new("query", &documents);
    let decoded = decode(&request.encode());
    assert_eq!(decoded.recipe, RECIPE);
    assert_eq!(decoded.query, "query");
    assert_eq!(
        decoded.documents,
        vec![(0, "first".to_owned()), (1, "second".to_owned())]
    );
    let id = request.id.as_bytes();
    let scores = request
        .validate(&reply(id, &[(1, -2.5), (0, 1.25)]))
        .unwrap();
    assert_eq!(scores.scores, vec![1.25, -2.5]);
    assert_eq!(scores.inference_milliseconds, 9);
    let mut wrong_id = reply(id, &[(0, 1.0), (1, 2.0)]);
    wrong_id[5] ^= 0xff;
    let mut trailing = reply(id, &[(0, 1.0), (1, 2.0)]);
    trailing.push(0);
    let mut magic = reply(id, &[(0, 1.0), (1, 2.0)]);
    magic[0] = b'X';
    for defect in [
        wrong_id,
        trailing,
        magic,
        reply(id, &[(0, 1.0)]),
        reply(id, &[(0, 1.0), (0, 2.0)]),
        reply(id, &[(0, 1.0), (2, 2.0)]),
        reply(id, &[(0, f32::NAN), (1, 2.0)]),
        reply(id, &[(0, f32::INFINITY), (1, 2.0)]),
        reply(id, &[(0, 1001.0), (1, 2.0)]),
        Vec::new(),
    ] {
        assert!(request.validate(&defect).is_none());
    }
    // Identity covers the recipe, the query and every document boundary.
    let other = ["firsts", "econd"];
    assert_ne!(Request::new("query", &other).id, request.id);
    assert_ne!(Request::new("other", &documents).id, request.id);
}

#[tokio::test]
async fn remote_reranking_returns_scores_in_input_order_and_compresses_large_requests() {
    let compressed = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&compressed);
    let server = serve(axum::Router::new().route(
        "/",
        axum::routing::post(
            move |headers: axum::http::HeaderMap, bytes: axum::body::Bytes| {
                if headers.contains_key(axum::http::header::CONTENT_ENCODING) {
                    seen.fetch_add(1, Ordering::SeqCst);
                }
                scoring(headers, bytes)
            },
        ),
    ))
    .await;
    let client = client(&server.url, Duration::from_secs(5));
    let result = client.rerank("query", &["ab", "a", "abc"]).await.unwrap();
    assert_eq!(result.scores, vec![2.0, 1.0, 3.0]);
    assert_eq!(result.inference_milliseconds, 9);
    assert_eq!(compressed.load(Ordering::SeqCst), 0);
    let long = "x".repeat(COMPRESSION_THRESHOLD);
    let result = client.rerank("query", &[&long, "a"]).await.unwrap();
    assert_eq!(result.scores, vec![999.0, 1.0]);
    assert_eq!(compressed.load(Ordering::SeqCst), 1);
    assert_eq!(server.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn remote_reranking_reports_each_failure_once_without_retrying() {
    use axum::{http::StatusCode, routing::post};
    let server = serve(
        axum::Router::new()
            .route(
                "/unavailable",
                post(|| async { (StatusCode::SERVICE_UNAVAILABLE, "private-input") }),
            )
            // The worker rejects a recipe it does not serve before inference.
            .route(
                "/recipe",
                post(|| async { (StatusCode::BAD_REQUEST, "unsupported reranking recipe") }),
            )
            .route(
                "/redirect",
                post(|| async { (StatusCode::TEMPORARY_REDIRECT, [("location", "/valid")]) }),
            )
            .route("/malformed", post(|| async { "private-input" }))
            .route(
                "/mismatched",
                post(|| async { reply(&[0; REQUEST_ID_BYTES], &[(0, 1.0)]) }),
            )
            .route(
                "/oversized",
                post(|| async { vec![0u8; MAX_RESPONSE_BYTES + 1] }),
            )
            .route("/valid", post(scoring)),
    )
    .await;
    for (path, expected) in [
        ("unavailable", RerankerFallback::Unavailable),
        ("recipe", RerankerFallback::Unavailable),
        ("redirect", RerankerFallback::Unavailable),
        ("malformed", RerankerFallback::InvalidResponse),
        ("mismatched", RerankerFallback::InvalidResponse),
        ("oversized", RerankerFallback::InvalidResponse),
    ] {
        let before = server.calls.load(Ordering::SeqCst);
        let client = client(&format!("{}/{path}", server.url), Duration::from_secs(5));
        let reason = client.rerank("query", &["document"]).await.err().unwrap();
        assert_eq!(reason, expected, "{path}");
        assert_eq!(server.calls.load(Ordering::SeqCst), before + 1, "{path}");
    }
}

#[tokio::test]
async fn remote_reranking_stops_waiting_for_a_cold_worker_within_its_bound() {
    let server = serve(axum::Router::new().route(
        "/",
        axum::routing::post(
            |headers: axum::http::HeaderMap, bytes: axum::body::Bytes| async move {
                tokio::time::sleep(Duration::from_secs(10)).await;
                scoring(headers, bytes).await
            },
        ),
    ))
    .await;
    let client = client(&server.url, Duration::from_millis(200));
    let started = std::time::Instant::now();
    let reason = client.rerank("query", &["document"]).await.err().unwrap();
    assert_eq!(reason, RerankerFallback::Timeout);
    assert!(started.elapsed() < Duration::from_secs(2));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(server.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn remote_reranking_refuses_oversized_input_without_a_request() {
    let server = serve(axum::Router::new().route("/", axum::routing::post(scoring))).await;
    let client = client(&server.url, Duration::from_secs(5));
    let many = vec!["document"; MAX_DOCUMENTS + 1];
    let long = "x".repeat(MAX_QUERY_BYTES + 1);
    let large = "x".repeat(MAX_DOCUMENT_BYTES / 2 + 1);
    for (query, documents) in [
        ("query", many.as_slice()),
        ("query", &[][..]),
        ("", &["document"][..]),
        (long.as_str(), &["document"][..]),
        ("query", &[large.as_str(), large.as_str()][..]),
    ] {
        assert_eq!(
            client.rerank(query, documents).await.err(),
            Some(RerankerFallback::InputLimit)
        );
    }
    assert_eq!(server.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn warm_up_sends_an_empty_batch_without_chat_text() {
    let server = serve(axum::Router::new().route(
        "/",
        axum::routing::post(
            |headers: axum::http::HeaderMap, bytes: axum::body::Bytes| async move {
                let request = decode(&body(&headers, &bytes));
                assert_eq!((request.query.as_str(), request.documents.len()), ("", 0));
                reply(&request.request_id, &[])
            },
        ),
    ))
    .await;
    let configuration = Reranking::Remote {
        url: server.url.clone(),
        // Any set variable stands in for a credential against a fake worker.
        token_env: "HOME".into(),
        timeout_milliseconds: 100,
    };
    warm_up(&configuration).await.unwrap();
    assert_eq!(server.calls.load(Ordering::SeqCst), 1);
    let unset = Reranking::Remote {
        url: server.url.clone(),
        token_env: "MASH_TEST_UNSET_RERANKING_TOKEN".into(),
        timeout_milliseconds: 100,
    };
    assert!(warm_up(&unset).await.is_err());
    assert!(warm_up(&Reranking::Local {}).await.is_err());
    assert_eq!(server.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn the_worker_is_woken_only_when_the_default_search_reranks_on_it() {
    let directory = tempfile::tempdir().unwrap();
    let load = |text: &str| {
        let path = directory.path().join("config.toml");
        std::fs::write(&path, format!("version=1\nproviders={{}}\n{text}")).unwrap();
        crate::config::load(Some(&path)).unwrap()
    };
    let remote = "[reranking]\nbackend='remote'\nurl='https://example.com/rerank'\ntoken_env='MASH_TEST_RERANK'\n";
    // Word search is the default: nothing to wake, whatever is configured.
    assert!(!warms(&load(remote)));
    assert!(!warms(&load("")));
    let meaning = "[views.query]\nintent='related-discussion'\n";
    assert!(warms(&load(&format!("{remote}{meaning}"))));
    // Meaning by default, but ranked without the reranker, or on this CPU.
    assert!(!warms(&load(&format!(
        "{remote}{meaning}ranking='fusion'\n"
    ))));
    assert!(!warms(&load(meaning)));
}

#[test]
fn connect_uses_remote_only_for_the_pinned_installed_reranker_and_a_credential() {
    let root = tempfile::tempdir().unwrap();
    let repository = root.path().join(crate::semantic::RERANKER_REPOSITORY);
    let install = |revision: &str| {
        std::fs::create_dir_all(repository.join("refs")).unwrap();
        std::fs::create_dir_all(repository.join("snapshots").join(revision)).unwrap();
        std::fs::write(repository.join("refs/main"), revision).unwrap();
    };
    let remote = |token_env: &str| Reranking::Remote {
        url: "https://rerank.example".into(),
        token_env: token_env.into(),
        timeout_milliseconds: 1500,
    };
    assert!(connect(&Reranking::Local {}, root.path()).is_none());
    assert_eq!(
        connect(&remote("HOME"), root.path()).map(Result::err),
        Some(Some(RerankerFallback::RecipeMismatch))
    );
    install(&"a".repeat(40));
    assert_eq!(
        connect(&remote("HOME"), root.path()).map(Result::err),
        Some(Some(RerankerFallback::RecipeMismatch))
    );
    install(REVISION);
    assert_eq!(
        connect(&remote("MASH_TEST_UNSET_RERANKING_TOKEN"), root.path()).map(Result::err),
        Some(Some(RerankerFallback::CredentialMissing))
    );
    let connected = connect(&remote("HOME"), root.path()).unwrap().unwrap();
    assert_eq!(connected.url, "https://rerank.example");
    assert_eq!(connected.timeout, Duration::from_millis(1500));
}

/// Deployed worker round trips, and score parity with the local CPU reranker
/// when `MASH_TEST_MODELS_DIR` holds an installation.
#[tokio::test]
#[ignore = "requires an explicitly deployed reranking service and invocation token"]
async fn remote_reranking_live_service_contract() {
    let configuration = Reranking::Remote {
        url: std::env::var("MASH_TEST_RERANKING_URL").expect("set MASH_TEST_RERANKING_URL"),
        token_env: "MASH_RERANKING_TOKEN".into(),
        timeout_milliseconds: 10_000,
    };
    let started = std::time::Instant::now();
    warm_up(&configuration).await.unwrap();
    eprintln!(
        "remote reranking warm-up: roundtrip_ms={}",
        started.elapsed().as_millis()
    );
    let client = shared_client(&configuration).unwrap();
    let query = "how do I rotate a database password";
    let documents = [
        "The cat slept on the warm windowsill all afternoon.",
        "Rotate the Postgres password with ALTER ROLE and update the secret.",
        "Matched message: we discussed password rotation for the staging database last week",
        "東京の天気は晴れです。",
    ];
    let mut remote = Vec::new();
    for iteration in 0..5 {
        let started = std::time::Instant::now();
        let result = client.rerank(query, &documents).await.unwrap();
        eprintln!(
            "remote reranking call {iteration}: roundtrip_ms={} inference_ms={}",
            started.elapsed().as_millis(),
            result.inference_milliseconds
        );
        remote = result.scores;
    }
    assert!(remote[1] > remote[0]);
    if let Some(root) = std::env::var_os("MASH_TEST_MODELS_DIR") {
        let models = crate::semantic::Models::new(root.into());
        let cpu = crate::blocking(move || {
            let mut backend = models
                .load_search(crate::views::config::Ranking::ContextualRerank)?
                .ok_or_else(AppError::models_missing)?;
            backend.rerank_cpu(query, &documents)
        })
        .await
        .unwrap();
        eprintln!("remote scores {remote:?}\ncpu scores {cpu:?}");
        for (remote, cpu) in remote.iter().zip(&cpu) {
            assert!((remote - cpu).abs() < 0.01, "remote {remote} cpu {cpu}");
        }
    }
}
