use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::AppError;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Configuration {
    /// The complete OpenAI-compatible embeddings endpoint, including its path.
    pub(crate) url: String,
    pub(crate) model: String,
    #[serde(default = "default_dimensions")]
    pub(crate) dimensions: usize,
    #[serde(default)]
    pub(crate) api: Api,
    pub(crate) token_env: Option<String>,
    /// Some fixed-dimension self-hosted endpoints do not accept this parameter.
    #[serde(default)]
    pub(crate) request_dimensions: bool,
}

fn default_dimensions() -> usize {
    384
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
            let mut body = serde_json::json!({
                "model": self.config.model, "input": batch, "encoding_format": "float"
            });
            if self.config.api == Api::Voyage {
                body.as_object_mut()
                    .ok_or_else(|| AppError::internal("invalid request body"))?
                    .remove("encoding_format");
                body["output_dimension"] = serde_json::json!(self.config.dimensions);
                body["input_type"] = serde_json::json!(if query { "query" } else { "document" });
                body["truncation"] = serde_json::json!(false);
            } else if self.config.request_dimensions {
                body["dimensions"] = serde_json::json!(self.config.dimensions);
            }
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
    AppError::new(4, "remote-inference", message, retryable)
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
    async fn remote_cli_index_needs_no_local_models_and_rebuilds_changed_spaces() {
        let (remote, mock, task) = mock().await;
        let temp = tempfile::tempdir().unwrap();
        let history = temp.path().join("history");
        std::fs::create_dir(&history).unwrap();
        std::fs::write(history.join("session.jsonl"), concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"session\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"repeated searchable text\"}]}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"repeated searchable text\"}]}}\n"
        )).unwrap();
        let config = temp.path().join("config.json");
        let db = temp.path().join("mash.sqlite3");
        let models = temp.path().join("no-models");
        let mut document = serde_json::json!({"version":1,"providers":{"codex":{"roots":[history]}},"index":{"since_days":null},"embeddings":remote});
        std::fs::write(&config, serde_json::to_vec(&document).unwrap()).unwrap();
        let args = vec![
            "mash".into(),
            "--config".into(),
            config.as_os_str().to_owned(),
            "--db".into(),
            db.as_os_str().to_owned(),
            "--models-dir".into(),
            models.as_os_str().to_owned(),
            "index".into(),
        ];
        crate::cli::run_async(args.clone()).await.unwrap();
        assert!(!models.exists());
        let generation = crate::semantic::model_space("voyage-4", 1024);
        let storage = crate::storage::Storage::open_existing(&db).unwrap();
        assert_eq!(storage.counts().unwrap().embeddings, 2);
        assert!(storage.semantic_index_is_ready(&generation).unwrap());
        drop(storage);
        assert_eq!(mock.requests.lock().unwrap().len(), 1);
        crate::cli::run_async(args.clone()).await.unwrap();
        assert_eq!(mock.requests.lock().unwrap().len(), 1);
        document["embeddings"]["model"] = serde_json::json!("voyage-4-large");
        std::fs::write(&config, serde_json::to_vec(&document).unwrap()).unwrap();
        crate::cli::run_async(args.clone()).await.unwrap();
        assert_eq!(mock.requests.lock().unwrap().len(), 2);
        document["embeddings"]["model"] = serde_json::json!("voyage-4-lite");
        std::fs::write(&config, serde_json::to_vec(&document).unwrap()).unwrap();
        mock.failure.store(429, Ordering::Relaxed);
        let error = crate::cli::run_async(args).await.unwrap_err();
        assert_eq!(error.error.kind, "remote-inference");
        assert!(
            !crate::storage::Storage::open_existing(&db)
                .unwrap()
                .semantic_index_is_ready(&crate::semantic::model_space("voyage-4-lite", 1024))
                .unwrap()
        );
        task.abort();
    }
}
