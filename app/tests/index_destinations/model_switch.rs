//! Full CLI/HTTP model routing with controlled embeddings, not model-quality evidence.
use super::*;
use axum::{Json, Router, extract::State, routing::post};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
enum Model {
    #[serde(rename = "contract-a")]
    A,
    #[serde(rename = "contract-b")]
    B,
    #[serde(rename = "contract-c")]
    C,
}
impl Model {
    fn name(self) -> &'static str {
        match self {
            Self::A => "contract-a",
            Self::B => "contract-b",
            Self::C => "contract-c",
        }
    }
    fn dimensions(self) -> usize {
        if self == Self::C { 8 } else { 4 }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmbeddingRequest {
    model: Model,
    input: Vec<String>,
    dimensions: usize,
    encoding_format: String,
}
#[derive(Serialize)]
struct Output {
    data: Vec<Item>,
}
#[derive(Serialize)]
struct Item {
    index: usize,
    embedding: Vec<f32>,
}

async fn embed(
    State(calls): State<Arc<Mutex<Vec<Model>>>>,
    Json(input): Json<EmbeddingRequest>,
) -> Json<Output> {
    assert_eq!(input.dimensions, input.model.dimensions());
    assert_eq!(input.encoding_format, "float");
    assert!(input.input.iter().all(|text| !text.is_empty()));
    calls.lock().unwrap().push(input.model);
    let coordinate = match input.model {
        Model::A => 0,
        Model::B => 1,
        Model::C => 2,
    };
    Json(Output {
        data: input
            .input
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let mut embedding = vec![0.0; input.dimensions];
                embedding[coordinate] = 1.0;
                Item { index, embedding }
            })
            .collect(),
    })
}

struct Embeddings {
    url: String,
    calls: Arc<Mutex<Vec<Model>>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Embeddings {
    fn new() -> Self {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let captured = calls.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        let (shutdown, stopped) = tokio::sync::oneshot::channel();
        let thread = std::thread::spawn(move || {
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(async move {
                    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                    sender
                        .send(format!(
                            "http://{}/embeddings",
                            listener.local_addr().unwrap()
                        ))
                        .unwrap();
                    let app = Router::new()
                        .route("/embeddings", post(embed))
                        .with_state(captured);
                    axum::serve(listener, app)
                        .with_graceful_shutdown(async {
                            let _ = stopped.await;
                        })
                        .await
                        .unwrap();
                });
        });
        Self {
            url: receiver
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap(),
            calls,
            shutdown: Some(shutdown),
            thread: Some(thread),
        }
    }
}
impl Drop for Embeddings {
    fn drop(&mut self) {
        let _ = self.shutdown.take().unwrap().send(());
        self.thread.take().unwrap().join().unwrap();
    }
}

fn configure(fixture: &Fixture, service: &Embeddings, model: Model) {
    fixture.configure("both", "split");
    let mut document: toml::Table =
        toml::from_str(&std::fs::read_to_string(&fixture.config).unwrap()).unwrap();
    document.insert(
        "embeddings".into(),
        toml::toml! {
            api = "openai-compatible"
            url = (service.url.as_str())
            model = (model.name())
        dimensions = (i64::try_from(model.dimensions()).unwrap())
            request_dimensions = true
        }
        .into(),
    );
    std::fs::write(&fixture.config, toml::to_string(&document).unwrap()).unwrap();
}

fn activate(fixture: &mut Fixture, service: &Embeddings, model: Model) {
    drop(fixture.server.take());
    fixture.server = Some(shared_server::Server::with_embedding_space(
        fixture.directory.path(),
        &fixture.group,
        &["split"],
        model.name(),
        model.dimensions(),
    ));
    configure(fixture, service, model);
}

fn search(fixture: &Fixture) -> Value {
    fixture.json(
        &[
            "search",
            "canonical silver",
            "--shared",
            "--no-worker",
            "--ranking",
            "fusion",
            "--unit",
            "messages",
        ],
        true,
    )
}

#[test]
#[ignore = "requires disposable MASH_TEST_QDRANT_URL; controlled HTTP embeddings"]
fn qdrant_cli_remote_model_and_dimension_switch_rebuilds_without_touching_silver() {
    let service = Embeddings::new();
    let mut fixture = Fixture::new();
    activate(&mut fixture, &service, Model::A);
    std::fs::write(fixture.codex.join("session.jsonl"), concat!(
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"model-session\"}}\n",
        "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"canonical silver for model switching\"}]}}\n"
    )).unwrap();
    assert_eq!(
        fixture.json(&["index", "--all-history"], true)["shared"]["state"],
        "published"
    );
    let connection = Connection::open(&fixture.database).unwrap();
    rebuild::snapshot_local(&connection);
    let canonical = rebuild::remote_silver(&fixture);
    let initial = fixture.remote_node("split");
    assert_eq!(search(&fixture)["results"].as_array().unwrap().len(), 1);
    std::fs::remove_dir_all(&fixture.codex).unwrap();
    std::fs::remove_dir_all(fixture.directory.path().join("claude")).unwrap();
    for model in [Model::B, Model::C, Model::A] {
        activate(&mut fixture, &service, model);
        assert_eq!(fixture.remote_node("split")["gold_current"], false);
        let exact = fixture.json(
            &[
                "search",
                "canonical",
                "--shared",
                "--no-worker",
                "--intent",
                "exact-mention",
            ],
            true,
        );
        assert_eq!(exact["results"].as_array().unwrap().len(), 1);
        let built = fixture.json(&["index", "--rebuild-shared-gold"], true);
        assert_eq!(built["model_inferences"], 1);
        let current = fixture.remote_node("split");
        assert_eq!(current["gold_current"], true);
        for key in [
            "silver_revision",
            "revision",
            "last_updated_at",
            "last_synced_at",
            "silver_coverage",
            "messages",
            "conversations",
        ] {
            assert_eq!(current[key], initial[key], "{key}");
        }
        assert_eq!(rebuild::remote_silver(&fixture), canonical);
        rebuild::assert_local_unchanged(&connection);
        assert_eq!(search(&fixture)["results"].as_array().unwrap().len(), 1);
        if model == Model::C {
            rebuild::assert_mcp_rebuild(&fixture);
        }
        let previous = if model == Model::A {
            Model::C
        } else {
            Model::A
        };
        configure(&fixture, &service, previous);
        let mismatch = fixture.run(
            &[
                "search",
                "canonical silver",
                "--shared",
                "--no-worker",
                "--ranking",
                "fusion",
            ],
            true,
        );
        assert!(!mismatch.status.success(), "mismatched model was accepted");
        configure(&fixture, &service, model);
        assert_eq!(fixture.remote_node("split"), current);
        assert!(!fixture.codex.exists());
    }
    let calls = service.calls.lock().unwrap();
    for model in [Model::A, Model::B, Model::C] {
        assert!(calls.contains(&model), "model not called: {model:?}");
    }
}
