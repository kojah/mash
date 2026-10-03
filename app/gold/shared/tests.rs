use super::*;
use crate::message_context::MessageContext;
use crate::role::Role;
use crate::silver::{NormalizedMessage, SourceIdentity, messages::Messages};
use std::sync::{Arc, Mutex};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct EmbeddingRequest {
    model: String,
    input: Vec<String>,
    encoding_format: String,
}
#[derive(serde::Serialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingItem>,
}
#[derive(serde::Serialize)]
struct EmbeddingItem {
    index: usize,
    embedding: [f32; 4],
}

async fn embedding_endpoint(
    axum::extract::State(requests): axum::extract::State<Arc<Mutex<Vec<Vec<String>>>>>,
    axum::Json(request): axum::Json<EmbeddingRequest>,
) -> axum::Json<EmbeddingResponse> {
    assert_eq!(request.model, "fixture");
    assert_eq!(request.encoding_format, "float");
    let response = EmbeddingResponse {
        data: request
            .input
            .iter()
            .enumerate()
            .map(|(index, _)| EmbeddingItem {
                index,
                embedding: [1.0, 0.0, 0.0, 0.0],
            })
            .collect(),
    };
    requests.lock().unwrap().push(request.input);
    axum::Json(response)
}

fn conversation() -> Conversation {
    Conversation {
        id: "thread".to_owned(),
        provider: crate::provider::Provider::Codex,
        source: SourceIdentity::file("fixture.jsonl"),
        claude_lineage: None,
        project: Some("project".to_owned()),
        repository: Some("repository".to_owned()),
        title: Some("title".to_owned()),
        created_at: Some(0),
        updated_at: Some(36),
        messages: Messages::stage_fixture(
            (0..37)
                .map(|ordinal| NormalizedMessage {
                    id: format!("message-{ordinal}"),
                    ordinal,
                    context: if ordinal == 0 {
                        MessageContext::Root
                    } else {
                        MessageContext::Parent { ordinal: 0 }
                    },
                    role: Role::User,
                    content: format!("canonical {ordinal}"),
                    dialogue_text: Some(if ordinal % 4 == 0 {
                        String::new()
                    } else {
                        projection(ordinal)
                    }),
                    created_at: Some(ordinal),
                })
                .collect(),
        )
        .unwrap(),
    }
}

fn result() -> Build {
    Build::default()
}

fn projection(ordinal: i64) -> String {
    format!("projection {ordinal} {} trailingevidence", "é".repeat(300))
}

fn canonical_rows(conversation: &Conversation) -> Vec<u8> {
    let rows = conversation
        .messages
        .rows()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    serde_json::to_vec(&rows).unwrap()
}

#[tokio::test]
async fn stored_silver_without_sources_preserves_canonical_rows_and_embedding_groups() {
    for recipe in [
        crate::gold::config::EmbeddingText::Dialogue,
        crate::gold::config::EmbeddingText::TitleAndDialogue,
    ] {
        for budget in [None, Some(256_u32.try_into().unwrap())] {
            verify_materialization(crate::gold::config::Build {
                max_embedding_chars: budget,
                embedding_text: recipe,
            })
            .await;
        }
    }
}

async fn verify_materialization(recipe: crate::gold::config::Build) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = semantic::remote::Configuration {
        url: format!("http://{}/embeddings", listener.local_addr().unwrap()),
        model: "fixture".to_owned(),
        dimensions: 4,
        api: semantic::remote::Api::OpenaiCompatible,
        token_env: None,
        request_dimensions: false,
    };
    let requests = Arc::new(Mutex::new(Vec::new()));
    let app = axum::Router::new()
        .route("/embeddings", axum::routing::post(embedding_endpoint))
        .with_state(requests.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let mut file = tempfile::NamedTempFile::new().unwrap();
    let mut result = result();
    result.recipe = recipe.clone();
    let directory = tempfile::tempdir().unwrap();
    let mut silver = conversation();
    silver.source = SourceIdentity::file(directory.path().join("missing-provider.jsonl"));
    assert!(!silver.source.path.exists());
    let canonical_before = canonical_rows(&silver);
    materialize(
        file.as_file_mut(),
        &silver,
        Path::new("unused"),
        Some(&config),
        &mut None,
        &mut result,
    )
    .await
    .unwrap();
    assert_eq!(canonical_rows(&silver), canonical_before);
    assert!(!silver.source.path.exists());
    assert_eq!(result.messages, 37);
    let events = serde_json::Deserializer::from_reader(std::fs::File::open(file.path()).unwrap())
        .into_iter::<Event>()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_published_silver(events);
    let expected = (0..37)
        .filter(|ordinal| ordinal % 4 != 0)
        .map(|ordinal| recipe.prepare(projection(ordinal), Some("title")))
        .collect::<Vec<_>>();
    let requests = requests.lock().unwrap();
    if recipe.max_embedding_chars.is_some() {
        assert!(
            requests
                .iter()
                .flatten()
                .all(|text| text.chars().count() == 256 && !text.contains("trailingevidence"))
        );
    }
    assert_eq!(requests.iter().map(Vec::len).collect::<Vec<_>>(), [24, 3]);
    assert_eq!(
        requests.iter().flatten().cloned().collect::<Vec<_>>(),
        expected
    );
    assert_eq!(result.embeddings.stored_vectors, 27);
    assert_eq!(result.embeddings.model_inferences, 27);
    server.abort();
}

fn assert_published_silver(events: Vec<Event>) {
    let mut sizes = Vec::new();
    let mut messages = Vec::new();
    for event in events {
        match event {
            Event::Batch(Batch::Conversations { records }) => {
                assert_eq!(records.len(), 1);
                assert_eq!(records[0].project.as_deref(), Some("project"));
            }
            Event::Batch(Batch::Messages { records }) => {
                sizes.push(records.len());
                messages.extend(records);
            }
            _ => panic!("unexpected publication event"),
        }
    }
    assert_eq!(sizes, [32, 5]);
    for (ordinal, message) in messages.iter().enumerate() {
        assert_eq!(message.ordinal, i64::try_from(ordinal).unwrap());
        assert_eq!(message.content, format!("canonical {ordinal}"));
        assert_eq!(
            message.projection,
            if ordinal % 4 == 0 {
                String::new()
            } else {
                projection(i64::try_from(ordinal).unwrap())
            }
        );
        assert_eq!(message.vector.is_none(), ordinal % 4 == 0);
        assert_eq!(
            message.context,
            if ordinal == 0 {
                MessageContext::Root
            } else {
                MessageContext::Parent { ordinal: 0 }
            }
        );
    }
}

#[tokio::test]
async fn corrupt_staging_cannot_finish_a_shared_publication_plan() {
    let conversation = conversation();
    std::fs::write(conversation.messages.staging_path().unwrap(), b"{broken").unwrap();
    let mut file = tempfile::NamedTempFile::new().unwrap();
    let mut result = result();
    let error = materialize(
        file.as_file_mut(),
        &conversation,
        Path::new("unused"),
        None,
        &mut None,
        &mut result,
    )
    .await
    .err()
    .unwrap();
    assert_eq!(error.error.kind, crate::ErrorKind::Io);
    assert_eq!(file.as_file().metadata().unwrap().len(), 0);
    assert_eq!(result.messages, 0);
}
