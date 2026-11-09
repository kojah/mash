//! Bounded, single-attempt inference; database ownership stays with Mash.
use crate::{AppError, config::Embeddings};
use serde::{Deserialize, Serialize};
use std::time::Duration;

// This identifies the inference recipe, independently of execution location.
pub(crate) const RECIPE: &str =
    "minilm-l6-v2:751bff37182d3f1213fa05d7196b954e230abad9:onnx-f32:mean-mask:512:l2:v1";
const DIMENSIONS: usize = 384;
const MAX_INPUT_BYTES: usize = 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_INPUTS: usize = 256;

#[derive(Serialize)]
struct Batch<'a> {
    recipe: &'static str,
    request_id: String,
    inputs: Vec<Input<'a>>,
}

#[derive(Serialize)]
struct Input<'a> {
    id: usize,
    text: &'a str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    recipe: String,
    request_id: String,
    vectors: Vec<Vector>,
    inference_milliseconds: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Vector {
    id: usize,
    values: Vec<f32>,
}

pub(crate) struct Result {
    pub(crate) vectors: Vec<Vec<f64>>,
    pub(crate) inference_milliseconds: u64,
}

fn request<'a>(texts: &'a [&'a str]) -> std::result::Result<Batch<'a>, AppError> {
    if texts.is_empty()
        || texts.len() > MAX_INPUTS
        || texts.iter().map(|text| text.len()).sum::<usize>() > MAX_INPUT_BYTES
    {
        return Err(AppError::model(
            "remote embedding batch exceeds input limits",
        ));
    }
    let mut hash = blake3::Hasher::new();
    hash.update(RECIPE.as_bytes());
    for text in texts {
        hash.update(&text.len().to_le_bytes());
        hash.update(text.as_bytes());
    }
    Ok(Batch {
        recipe: RECIPE,
        request_id: hash.finalize().to_hex().to_string(),
        inputs: texts
            .iter()
            .enumerate()
            .map(|(id, text)| Input { id, text })
            .collect(),
    })
}

fn validate(response: Response, request: &Batch<'_>) -> std::result::Result<Result, AppError> {
    if response.recipe != request.recipe
        || response.request_id != request.request_id
        || response.vectors.len() != request.inputs.len()
    {
        return Err(AppError::model(
            "embedding service returned mismatched recipe or batch",
        ));
    }
    let mut vectors = vec![None; request.inputs.len()];
    for vector in response.vectors {
        let Some(slot) = vectors.get_mut(vector.id) else {
            return Err(AppError::model(
                "embedding service returned an unknown input ID",
            ));
        };
        let norm = vector
            .values
            .iter()
            .map(|v| f64::from(*v).powi(2))
            .sum::<f64>();
        if slot.is_some()
            || vector.values.len() != DIMENSIONS
            || vector
                .values
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 1.001)
            || !(0.99..=1.01).contains(&norm)
        {
            return Err(AppError::model(
                "embedding service returned invalid vectors",
            ));
        }
        *slot = Some(
            super::retrieval::quantize_vector(&vector.values)
                .values
                .into_iter()
                .map(|value| f64::from(value.cast_signed()))
                .collect(),
        );
    }
    let vectors = vectors
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| AppError::model("embedding service omitted an input"))?;
    Ok(Result {
        vectors,
        inference_milliseconds: response.inference_milliseconds,
    })
}

pub(crate) struct Client {
    http: reqwest::Client,
    url: String,
    token: String,
}

impl Client {
    pub(crate) fn new(configuration: &Embeddings) -> std::result::Result<Self, AppError> {
        configuration.validate()?;
        let Embeddings::Remote {
            url,
            token_env,
            timeout_seconds,
        } = configuration
        else {
            return Err(AppError::internal(
                "remote inference requires remote configuration",
            ));
        };
        let token = std::env::var(token_env)
            .ok()
            .filter(|token| !token.trim().is_empty())
            .ok_or_else(|| {
                AppError::configuration("embedding service token environment variable is missing")
            })?;
        Ok(Self {
            http: http_client(*timeout_seconds)?,
            url: url.clone(),
            token,
        })
    }

    pub(crate) async fn embed(&self, texts: &[&str]) -> std::result::Result<Result, AppError> {
        let mut remaining = texts;
        let mut result = Result {
            vectors: Vec::new(),
            inference_milliseconds: 0,
        };
        while !remaining.is_empty() {
            let mut bytes = 0;
            let count = remaining
                .iter()
                .take(MAX_INPUTS)
                .take_while(|text| {
                    bytes += text.len();
                    bytes <= MAX_INPUT_BYTES
                })
                .count();
            if count == 0 {
                return Err(AppError::model("remote embedding input exceeds byte limit"));
            }
            let batch = send(&self.http, &self.url, &self.token, &remaining[..count]).await?;
            result.vectors.extend(batch.vectors);
            result.inference_milliseconds += batch.inference_milliseconds;
            remaining = &remaining[count..];
        }
        Ok(result)
    }
}

fn http_client(timeout_seconds: u32) -> std::result::Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(u64::from(timeout_seconds)))
        .build()
        .map_err(|_| AppError::model("cannot create embedding service client"))
}

async fn send(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    texts: &[&str],
) -> std::result::Result<Result, AppError> {
    let request = request(texts)?;
    // No response bodies or transport errors enter diagnostics: they may echo
    // credentials or private input. A timeout can still have incurred GPU cost.
    let failure = || {
        AppError::model(
            "remote embedding request failed; it was not retried and may have incurred inference cost",
        )
    };
    let mut response = client
        .post(url)
        .bearer_auth(token)
        .json(&request)
        .send()
        .await
        .map_err(|_| failure())?;
    if !response.status().is_success() {
        return Err(AppError::model(format!(
            "embedding service returned HTTP {}; request was not retried",
            response.status().as_u16()
        )));
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
    {
        return Err(AppError::model("embedding response exceeds size limit"));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| failure())? {
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(AppError::model("embedding response exceeds size limit"));
        }
        bytes.extend_from_slice(&chunk);
    }
    let response = serde_json::from_slice(&bytes)
        .map_err(|_| AppError::model("embedding service returned an invalid response"))?;
    validate(response, &request)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(request: &Batch<'_>) -> Response {
        Response {
            recipe: RECIPE.to_owned(),
            request_id: request.request_id.clone(),
            inference_milliseconds: 12,
            vectors: (0..request.inputs.len())
                .map(|id| {
                    let mut values = vec![0.0; DIMENSIONS];
                    values[id] = 1.0;
                    Vector { id, values }
                })
                .collect(),
        }
    }

    #[test]
    fn remote_embeddings_preserve_input_identity_when_results_arrive_reordered() {
        let request = request(&["first", "second"]).unwrap();
        let mut reply = response(&request);
        reply.vectors.reverse();
        let result = validate(reply, &request).unwrap();
        assert_eq!(result.vectors[0][0], 127.0);
        assert_eq!(result.vectors[0][1], 0.0);
        assert_eq!(result.vectors[1][0], 0.0);
        assert_eq!(result.vectors[1][1], 127.0);
    }

    #[test]
    fn remote_embeddings_reject_wrong_identity_and_malformed_vectors() {
        let request = request(&["first", "second"]).unwrap();
        for defect in 0..8 {
            let mut reply = response(&request);
            match defect {
                0 => reply.recipe.push('x'),
                1 => reply.request_id.push('x'),
                2 => {
                    reply.vectors.pop();
                }
                3 => reply.vectors[1].id = 0,
                4 => reply.vectors[1].id = 2,
                5 => {
                    reply.vectors[0].values.pop();
                }
                6 => reply.vectors[0].values[0] = f32::NAN,
                _ => reply.vectors[0].values.fill(0.0),
            }
            assert!(
                validate(reply, &request).is_err(),
                "accepted defect {defect}"
            );
        }
    }

    #[test]
    fn remote_embeddings_bound_batches_and_bind_exact_inputs() {
        assert!(request(&[]).is_err());
        assert!(request(&["x"; MAX_INPUTS + 1]).is_err());
        assert!(request(&[&"x".repeat(MAX_INPUT_BYTES + 1)]).is_err());
        assert_ne!(
            request(&["ab", "c"]).unwrap().request_id,
            request(&["a", "bc"]).unwrap().request_id
        );
        assert_ne!(
            request(&["a", "b"]).unwrap().request_id,
            request(&["b", "a"]).unwrap().request_id
        );
    }

    #[tokio::test]
    async fn remote_embeddings_split_by_byte_budget_without_losing_identity() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let app = axum::Router::new().route("/", axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
            let observed = Arc::clone(&observed);
            async move {
                observed.fetch_add(1, Ordering::SeqCst);
                let inputs = body["inputs"].as_array().unwrap();
                let vectors = inputs.iter().map(|input| {
                    let text = input["text"].as_str().unwrap();
                    let mut values = vec![0.0; DIMENSIONS];
                    values[usize::from(text.as_bytes()[0] - b'a')] = 1.0;
                    serde_json::json!({"id": input["id"], "values": values})
                }).collect::<Vec<_>>();
                axum::Json(serde_json::json!({"recipe": RECIPE, "request_id": body["request_id"], "vectors": vectors, "inference_milliseconds": 2}))
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = Client {
            http: http_client(5).unwrap(),
            url: format!("http://{}", listener.local_addr().unwrap()),
            token: "test".into(),
        };
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let texts = ["a", "b", "c"].map(|text| text.repeat(MAX_INPUT_BYTES / 2));
        let references = texts.each_ref().map(String::as_str);
        let result = client.embed(&references).await.unwrap();
        server.abort();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(result.inference_milliseconds, 4);
        assert_eq!(result.vectors.len(), 3);
        for (index, vector) in result.vectors.iter().enumerate() {
            assert_eq!(vector[index], 127.0);
        }
    }

    #[tokio::test]
    #[ignore = "requires an explicitly deployed embedding service and invocation token"]
    async fn remote_embeddings_live_service_contract() {
        let configuration = Embeddings::Remote {
            url: std::env::var("MASH_TEST_EMBEDDING_URL").expect("set MASH_TEST_EMBEDDING_URL"),
            token_env: "MASH_EMBEDDING_TOKEN".into(),
            timeout_seconds: 120,
        };
        let client = Client::new(&configuration).unwrap();
        let texts = [
            "A cat sleeps on the sofa.",
            "A kitten rests on a couch.",
            "A database transaction commits its changes to disk.",
        ];
        for iteration in 0..2 {
            let started = std::time::Instant::now();
            let result = client.embed(&texts).await.unwrap();
            assert_eq!(result.vectors.len(), texts.len());
            let cosine = |left: &[f64], right: &[f64]| {
                left.iter().zip(right).map(|(a, b)| a * b).sum::<f64>()
                    / (left.iter().map(|v| v * v).sum::<f64>()
                        * right.iter().map(|v| v * v).sum::<f64>())
                    .sqrt()
            };
            assert!(
                cosine(&result.vectors[0], &result.vectors[1])
                    > cosine(&result.vectors[0], &result.vectors[2])
            );
            eprintln!(
                "remote embedding call {iteration}: roundtrip_ms={} inference_ms={}",
                started.elapsed().as_millis(),
                result.inference_milliseconds
            );
        }
    }

    #[tokio::test]
    async fn remote_embeddings_validate_http_results_and_refuse_redirects() {
        use axum::{Json, http::StatusCode, routing::post};
        let app = axum::Router::new()
            .route("/valid", post(|headers: axum::http::HeaderMap, Json(body): Json<serde_json::Value>| async move {
                assert_eq!(headers["authorization"], "Bearer test-credential");
                assert_eq!(body["inputs"][0]["text"], "hello");
                let mut values = vec![0.0; DIMENSIONS];
                values[0] = 1.0;
                Json(serde_json::json!({
                    "recipe": RECIPE,
                    "request_id": body["request_id"],
                    "vectors": [{"id": 0, "values": values}],
                    "inference_milliseconds": 7,
                }))
            }))
            .route("/malformed", post(|| async { "private-input invalid JSON" }))
            .route("/oversized", post(|| async { "x".repeat(MAX_RESPONSE_BYTES + 1) }))
            .route("/redirect", post(|| async {
                (StatusCode::TEMPORARY_REDIRECT, [("location", "/valid")])
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = http_client(5).unwrap();
        let result = send(
            &client,
            &format!("{base}/valid"),
            "test-credential",
            &["hello"],
        )
        .await
        .unwrap();
        assert_eq!(result.vectors.len(), 1);
        assert_eq!(result.vectors[0][0], 127.0);
        assert_eq!(result.inference_milliseconds, 7);
        for (path, expected) in [
            ("malformed", "invalid response"),
            ("oversized", "size limit"),
            ("redirect", "307"),
        ] {
            let error = send(
                &client,
                &format!("{base}/{path}"),
                "test-credential",
                &["hello"],
            )
            .await
            .err()
            .unwrap();
            assert!(
                error.error.message.contains(expected),
                "{}",
                error.error.message
            );
            assert!(!error.error.message.contains("private-input"));
        }
        server.abort();
    }

    #[tokio::test]
    async fn remote_embeddings_do_not_retry_or_expose_service_errors() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let app = axum::Router::new().route(
            "/",
            axum::routing::post(move || {
                let observed = Arc::clone(&observed);
                async move {
                    observed.fetch_add(1, Ordering::SeqCst);
                    (
                        axum::http::StatusCode::SERVICE_UNAVAILABLE,
                        "private-input-and-secret",
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let error = send(
            &http_client(5).unwrap(),
            &url,
            "test-credential",
            &["private-input"],
        )
        .await
        .err()
        .unwrap();
        server.abort();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(error.error.message.contains("503"));
        assert!(!error.error.message.contains("private-input"));
        assert!(!error.error.message.contains("secret"));
        assert!(!error.error.message.contains("test-credential"));
    }

    #[tokio::test]
    #[ignore = "requires an explicitly deployed embedding service and invocation token"]
    async fn remote_embeddings_live_service_batch_256() {
        let configuration = Embeddings::Remote {
            url: std::env::var("MASH_TEST_EMBEDDING_URL").expect("set MASH_TEST_EMBEDDING_URL"),
            token_env: "MASH_EMBEDDING_TOKEN".into(),
            timeout_seconds: 120,
        };
        let client = Client::new(&configuration).unwrap();
        let texts: Vec<String> = (0..MAX_INPUTS)
            .map(|index| format!("short document number {index}"))
            .collect();
        let references: Vec<&str> = texts.iter().map(String::as_str).collect();
        for iteration in 0..2 {
            let started = std::time::Instant::now();
            let result = client.embed(&references).await.unwrap();
            assert_eq!(result.vectors.len(), MAX_INPUTS);
            assert!(result.inference_milliseconds > 0);
            eprintln!(
                "remote 256-input call {iteration}: roundtrip_ms={} inference_ms={}",
                started.elapsed().as_millis(),
                result.inference_milliseconds
            );
        }
    }
}
