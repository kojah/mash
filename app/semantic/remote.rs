use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::AppError;

#[cfg(test)]
#[path = "remote/tests/faults.rs"]
mod faults;

#[derive(Serialize)]
#[serde(untagged)]
enum Request<'a> {
    OpenAi {
        model: &'a str,
        input: &'a [String],
        encoding_format: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        dimensions: Option<usize>,
    },
    Voyage {
        model: &'a str,
        input: &'a [String],
        output_dimension: usize,
        input_type: InputType,
        truncation: bool,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum InputType {
    Query,
    Document,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "WireConfiguration")]
pub(crate) struct Configuration {
    /// The complete OpenAI-compatible embeddings endpoint, including its path.
    pub(crate) url: String,
    pub(crate) model: String,
    pub(crate) dimensions: usize,
    #[serde(default)]
    pub(crate) api: Api,
    pub(crate) token_env: Option<String>,
    /// Some fixed-dimension self-hosted endpoints do not accept this parameter.
    #[serde(default)]
    pub(crate) request_dimensions: bool,
    /// Whether credentials are masked before text leaves for this endpoint;
    /// set from the `[redaction]` table, not this one.
    #[serde(skip)]
    pub(crate) redaction: crate::redaction::Policy,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireConfiguration {
    url: String,
    model: String,
    dimensions: Option<usize>,
    #[serde(default)]
    api: Api,
    token_env: Option<String>,
    #[serde(default)]
    request_dimensions: bool,
}

impl TryFrom<WireConfiguration> for Configuration {
    type Error = &'static str;

    fn try_from(raw: WireConfiguration) -> Result<Self, Self::Error> {
        // Unknown/custom endpoints must declare their vector space; assuming the
        // local encoder's width can silently select an incompatible index.
        let dimensions = raw
            .dimensions
            .or_else(
                || match raw.model.strip_prefix("voyageai/").unwrap_or(&raw.model) {
                    "voyage-4" | "voyage-code-4" | "voyage-4-large" | "voyage-4-lite" => Some(1024),
                    "all-MiniLM-L6-v2" | "sentence-transformers/all-MiniLM-L6-v2" => Some(384),
                    _ => None,
                },
            )
            .ok_or("embeddings.dimensions is required for an unknown/custom model")?;
        Ok(Self {
            url: raw.url,
            model: raw.model,
            dimensions,
            api: raw.api,
            token_env: raw.token_env,
            request_dimensions: raw.request_dimensions,
            redaction: crate::redaction::Policy::ON,
        })
    }
}

#[cfg(test)]
#[test]
fn dimensions_follow_model_instead_of_local_encoder_default() {
    for model in ["voyage-4", "voyageai/voyage-code-4"] {
        let config: Configuration = toml::from_str(&format!(
            "url='https://example.com/embeddings'\nmodel='{model}'\n"
        ))
        .unwrap();
        assert_eq!(config.dimensions, 1024);
    }
    let explicit: Configuration =
        toml::from_str("url='https://example.com/embeddings'\nmodel='custom'\ndimensions=2048\n")
            .unwrap();
    assert_eq!(explicit.dimensions, 2048);
    assert!(
        toml::from_str::<Configuration>("url='https://example.com/embeddings'\nmodel='custom'\n")
            .is_err()
    );
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Api {
    #[default]
    OpenaiCompatible,
    Voyage,
}

impl Configuration {
    pub(crate) fn validate(&self) -> Result<(), AppError> {
        if !(1..=2048).contains(&self.dimensions)
            || (self.api == Api::Voyage && ![256, 512, 1024, 2048].contains(&self.dimensions))
        {
            return Err(AppError::configuration(
                "invalid embedding dimensions (1..2048; Voyage: 256, 512, 1024, 2048)",
            ));
        }
        let url = reqwest::Url::parse(&self.url)
            .map_err(|_| AppError::configuration("invalid embeddings URL"))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || self.model.trim().is_empty()
            || self.model.len() > 255
            || self.model.chars().any(char::is_control)
        {
            return Err(AppError::configuration(
                "embeddings require an HTTP(S) URL without credentials, query or fragment, and a nonempty model",
            ));
        }
        if let Some(name) = &self.token_env
            && (name.is_empty()
                || !name.bytes().enumerate().all(|(i, b)| {
                    b.is_ascii_alphabetic() || b == b'_' || (i > 0 && b.is_ascii_digit())
                }))
        {
            return Err(AppError::configuration("invalid embeddings token_env"));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(crate) struct Client {
    client: reqwest::Client,
    config: Configuration,
}

#[derive(Deserialize)]
struct Response {
    data: Vec<Item>,
}
#[derive(Deserialize)]
struct Item {
    index: usize,
    embedding: Vec<f32>,
}

impl Client {
    pub(crate) fn new(config: Configuration) -> Result<Self, AppError> {
        config.validate()?;
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(name) = &config.token_env {
            let token = std::env::var(name).map_err(|_| {
                AppError::configuration(format!("set {name} for remote embeddings"))
            })?;
            if token.is_empty()
                || token.len() > 4096
                || !token.bytes().all(|b| b.is_ascii_graphic())
            {
                return Err(AppError::configuration("invalid remote embeddings token"));
            }
            let mut value = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|_| AppError::configuration("invalid remote embeddings token"))?;
            value.set_sensitive(true);
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|_| AppError::configuration("create remote embeddings client"))?;
        Ok(Self { client, config })
    }

    pub(crate) async fn embed(
        &self,
        texts: &[String],
        query: bool,
    ) -> Result<Vec<Vec<f32>>, AppError> {
        let mut result = Vec::with_capacity(texts.len());
        // Bound requests and responses independently of the local worker layout.
        for batch in texts.chunks(32) {
            if batch
                .iter()
                .any(|text| text.is_empty() || text.len() > 1024 * 1024)
            {
                return Err(error(
                    "remote embedding input is empty or exceeds 1 MiB",
                    false,
                ));
            }
            let safe = batch
                .iter()
                .map(|text| self.config.redaction.text(text))
                .collect::<Vec<_>>();
            let batch = safe.as_slice();
            let body = if self.config.api == Api::Voyage {
                Request::Voyage {
                    model: &self.config.model,
                    input: batch,
                    output_dimension: self.config.dimensions,
                    input_type: if query {
                        InputType::Query
                    } else {
                        InputType::Document
                    },
                    truncation: false,
                }
            } else {
                Request::OpenAi {
                    model: &self.config.model,
                    input: batch,
                    encoding_format: "float",
                    dimensions: self
                        .config
                        .request_dimensions
                        .then_some(self.config.dimensions),
                }
            };
            let mut response = self
                .client
                .post(&self.config.url)
                .json(&body)
                .send()
                .await
                .map_err(|_| {
                    error(
                        "remote embedding request failed; check endpoint availability",
                        true,
                    )
                })?;
            let status = response.status();
            if !status.is_success() {
                return Err(error(
                    format!(
                        "remote embedding endpoint returned HTTP {}",
                        status.as_u16()
                    ),
                    status.is_server_error() || matches!(status.as_u16(), 408 | 429),
                ));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| error("read remote embedding response", true))?
            {
                if bytes.len().saturating_add(chunk.len()) > 2 * 1024 * 1024 {
                    return Err(error("remote embedding response exceeds 2 MiB", false));
                }
                bytes.extend_from_slice(&chunk);
            }
            result.extend(decode(&bytes, batch.len(), self.config.dimensions)?);
        }
        Ok(result)
    }

    /// SQLite and local model pipelines live on `spawn_blocking` workers. Drive
    /// async HTTP on the caller's Tokio runtime without blocking a runtime thread.
    pub(crate) fn embed_on_worker(
        &self,
        texts: &[String],
        query: bool,
    ) -> Result<Vec<Vec<f32>>, AppError> {
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| AppError::internal("remote embeddings require a Tokio runtime"))?;
        handle.block_on(self.embed(texts, query))
    }
}

fn error(message: impl Into<String>, retryable: bool) -> AppError {
    AppError::new(4, crate::ErrorKind::RemoteInference, message, retryable)
}

fn decode(bytes: &[u8], count: usize, dimensions: usize) -> Result<Vec<Vec<f32>>, AppError> {
    let response: Response = serde_json::from_slice(bytes)
        .map_err(|_| error("invalid remote embedding response", false))?;
    if response.data.len() != count {
        return Err(error("unexpected remote embedding result count", false));
    }
    let mut ordered = vec![None; count];
    for item in response.data {
        if item.index >= count
            || ordered[item.index].is_some()
            || item.embedding.len() != dimensions
            || item
                .embedding
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 1.0e10)
            || item.embedding.iter().all(|v| v.abs() < f32::MIN_POSITIVE)
        {
            return Err(error(
                "invalid remote embedding index or vector dimensions",
                false,
            ));
        }
        ordered[item.index] = Some(item.embedding);
    }
    ordered
        .into_iter()
        .map(|v| v.ok_or_else(|| error("missing remote embedding result", false)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, extract::State, routing::post};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicU16, Ordering},
    };

    #[derive(Clone, Default)]
    struct Mock {
        requests: Arc<Mutex<Vec<serde_json::Value>>>,
        failure: Arc<AtomicU16>,
    }
    async fn endpoint(
        State(mock): State<Mock>,
        Json(body): Json<serde_json::Value>,
    ) -> (axum::http::StatusCode, Json<serde_json::Value>) {
        mock.requests.lock().unwrap().push(body.clone());
        let code = mock.failure.load(Ordering::Relaxed);
        if code != 0 {
            return (
                axum::http::StatusCode::from_u16(code).unwrap(),
                Json(serde_json::json!({"error":"secret echoed by provider"})),
            );
        }
        let data: Vec<_> = body["input"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .rev()
            .map(|(index, _)| {
                let mut vector =
                    vec![
                        0.0;
                        usize::try_from(body["output_dimension"].as_u64().unwrap_or(1024)).unwrap()
                    ];
                vector[index] = 1.0;
                serde_json::json!({"index":index,"embedding":vector})
            })
            .collect();
        (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({"data":data})),
        )
    }
    async fn mock() -> (Configuration, Mock, tokio::task::JoinHandle<()>) {
        let mock = Mock::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = Configuration {
            url: format!("http://{}/v1/embeddings", listener.local_addr().unwrap()),
            model: "voyage-4".into(),
            dimensions: 1024,
            api: Api::Voyage,
            token_env: None,
            request_dimensions: false,
            redaction: crate::redaction::Policy::ON,
        };
        let app = Router::new()
            .route("/v1/embeddings", post(endpoint))
            .with_state(mock.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (config, mock, task)
    }

    #[test]
    fn responses_require_complete_unique_indexes_and_finite_nonzero_vectors() {
        for body in [
            serde_json::json!({"data":[]}),
            serde_json::json!({"data":[{"index":2,"embedding":[1,0]}]}),
            serde_json::json!({"data":[{"index":0,"embedding":[0,0]}]}),
            serde_json::json!({"data":[{"index":0,"embedding":[1]}]}),
        ] {
            assert!(decode(&serde_json::to_vec(&body).unwrap(), 1, 2).is_err());
        }
        let duplicate = serde_json::json!({"data":[{"index":0,"embedding":[1,0]},{"index":0,"embedding":[0,1]}]});
        assert!(decode(&serde_json::to_vec(&duplicate).unwrap(), 2, 2).is_err());
    }

    #[tokio::test]
    async fn redaction_precedes_remote_embedding_request_serialization() {
        let (config, mock, task) = mock().await;
        let client = Client::new(config).unwrap();
        let input = vec!["password=\"hunter2\" Authorization: Bearer opaque-value".into()];
        client.embed(&input, false).await.unwrap();
        client.embed(&input, true).await.unwrap();
        for request in mock.requests.lock().unwrap().iter() {
            let body = request.to_string();
            assert!(!body.contains("hunter2") && !body.contains("opaque-value"));
            assert!(body.contains("REDACTED_SECRET"));
        }
        assert!(input[0].contains("hunter2"));
        task.abort();
    }

    #[tokio::test]
    async fn remote_embedding_masking_can_be_turned_off() {
        let (mut config, mock, task) = mock().await;
        config.redaction = crate::redaction::Policy::new(false);
        let client = Client::new(config).unwrap();
        let input = vec!["password=\"hunter2\"".into()];
        client.embed(&input, false).await.unwrap();
        let body = mock.requests.lock().unwrap()[0].to_string();
        assert!(body.contains("hunter2") && !body.contains("REDACTED_SECRET"));
        task.abort();
    }

    #[tokio::test]
    async fn fusion_backend_embeds_through_remote_api_without_local_assets() {
        let (config, _mock, task) = mock().await;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("absent-models");
        let models = crate::semantic::Models::new(path.clone()).with_remote(Some(config));
        let vector = crate::blocking(move || {
            models.require_search(crate::views::config::Ranking::Fusion)?;
            let mut backend = models
                .load_search(crate::views::config::Ranking::Fusion)?
                .unwrap();
            crate::semantic::shared_query_vector(&mut backend, "first")
        })
        .await
        .unwrap();
        assert_eq!(vector.len(), 1024);
        assert!(!path.exists());
        task.abort();
    }

    #[tokio::test]
    async fn voyage_requests_use_query_document_types_restore_order_and_sanitize_errors() {
        let (config, mock, task) = mock().await;
        let client = Client::new(config).unwrap();
        let vectors = client
            .embed(&["first".into(), "second".into()], false)
            .await
            .unwrap();
        assert_eq!(vectors[0][0], 1.0);
        assert_eq!(vectors[1][1], 1.0);
        client.embed(&["query".into()], true).await.unwrap();
        {
            let requests = mock.requests.lock().unwrap();
            assert_eq!(requests[0]["model"], "voyage-4");
            assert_eq!(requests[0]["output_dimension"], 1024);
            assert_eq!(requests[0]["input_type"], "document");
            assert_eq!(requests[1]["input_type"], "query");
            assert!(requests[0].get("dimensions").is_none());
        }
        mock.failure.store(429, Ordering::Relaxed);
        let error = client.embed(&["retry".into()], false).await.unwrap_err();
        assert!(error.error.retryable);
        assert!(!error.error.message.contains("secret"));
        mock.failure.store(401, Ordering::Relaxed);
        assert!(
            !client
                .embed(&["unauthorized".into()], false)
                .await
                .unwrap_err()
                .error
                .retryable
        );
        task.abort();
    }

    #[tokio::test]
    #[ignore = "requires disposable MASH_TEST_QDRANT_URL and MASH_TEST_QDRANT_GRPC_URL"]
    async fn remote_core_index_needs_no_local_models_and_rebuilds_changed_spaces() {
        let (remote, mock, task) = mock().await;
        let temp = tempfile::tempdir().unwrap();
        let history = temp.path().join("history");
        std::fs::create_dir(&history).unwrap();
        std::fs::write(history.join("session.jsonl"), concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"session\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"repeated searchable text\"}]}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"repeated searchable text\"}]}}\n"
        )).unwrap();
        let config = temp.path().join("config.toml");
        let db = temp.path().join("mash.sqlite3");
        let models = temp.path().join("no-models");
        let mut document = toml::toml! {
            version = 1
            providers = {codex = {roots = [(history.to_str().unwrap())]}}
            index = {all_history = true}
            embeddings = (toml::Value::try_from(&remote).unwrap())
            qdrant = (toml::Value::try_from(crate::views::local::test_support::qdrant_configuration()).unwrap())
        };
        std::fs::write(&config, toml::to_string(&document).unwrap()).unwrap();
        let request = crate::core::Request {
            paths: crate::core::Paths {
                config: Some(config.clone()),
                db: Some(db.clone()),
                models_dir: Some(models.clone()),
            },
            operation: crate::core::Operation::Index(crate::core::IndexRequest::default()),
        };
        crate::core::execute(request.clone()).await.unwrap();
        assert!(!models.exists());
        let resolved = crate::config::load(Some(&config)).unwrap();
        let local = crate::views::local::Local::open(&db, &resolved)
            .await
            .unwrap();
        assert_eq!(local.current().await.unwrap().unwrap().embeddings(), 2);
        assert_eq!(mock.requests.lock().unwrap().len(), 1);
        crate::core::execute(request.clone()).await.unwrap();
        assert_eq!(mock.requests.lock().unwrap().len(), 1);
        document["embeddings"]["model"] = toml::Value::String("voyage-4-large".into());
        std::fs::write(&config, toml::to_string(&document).unwrap()).unwrap();
        crate::core::execute(request.clone()).await.unwrap();
        assert_eq!(mock.requests.lock().unwrap().len(), 2);
        document["embeddings"]["model"] = toml::Value::String("voyage-4-lite".into());
        std::fs::write(&config, toml::to_string(&document).unwrap()).unwrap();
        mock.failure.store(429, Ordering::Relaxed);
        let error = crate::core::execute(request).await.unwrap_err();
        assert_eq!(error.error.kind, crate::ErrorKind::RemoteInference);
        let resolved = crate::config::load(Some(&config)).unwrap();
        let local = crate::views::local::Local::open(&db, &resolved)
            .await
            .unwrap();
        assert!(local.current().await.unwrap().is_none());
        crate::views::local::test_support::cleanup(&local, &resolved.qdrant.url).await;
        task.abort();
    }
}
