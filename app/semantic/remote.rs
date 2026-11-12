//! Bounded, single-attempt inference; database ownership stays with Mash.
use crate::{AppError, config::Embeddings};
use std::time::Duration;

// This identifies the inference recipe, independently of execution location.
pub(crate) const RECIPE: &str =
    "minilm-l6-v2:751bff37182d3f1213fa05d7196b954e230abad9:onnx-f32:mean-mask:512:l2:v1";
const DIMENSIONS: usize = 384;
const MAX_INPUT_BYTES: usize = 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_INPUTS: usize = 256;
/// Requests kept in flight together. The worker serializes GPU inference but
/// can receive and reply concurrently, hiding proxy round-trip latency.
pub(crate) const REQUEST_CONCURRENCY: usize = 4;
/// Small bodies gain nothing from framing overhead, so only compress requests
/// whose text is large enough to pay for the zstd frame.
const COMPRESSION_THRESHOLD: usize = 1024;
const COMPRESSION_LEVEL: i32 = 3;

// Versioned binary frames shared with deployment/embeddings/contract.py.
const REQUEST_MAGIC: &[u8; 4] = b"MSH1";
const RESPONSE_MAGIC: &[u8; 4] = b"MSR1";
const REQUEST_ID_BYTES: usize = 32;
const RESPONSE_HEADER_BYTES: usize = 4 + REQUEST_ID_BYTES + 4 + 4;
const VECTOR_BYTES: usize = 4 + DIMENSIONS * 2;

struct Batch<'a> {
    request_id: blake3::Hash,
    inputs: Vec<Input<'a>>,
}

struct Input<'a> {
    id: u32,
    text: &'a str,
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
        request_id: hash.finalize(),
        inputs: texts
            .iter()
            .enumerate()
            .map(|(id, text)| Input {
                id: u32::try_from(id).unwrap_or(u32::MAX),
                text,
            })
            .collect(),
    })
}

/// Encode the bounded request as a versioned binary frame. The recipe and
/// input lengths make the request self-describing without JSON escaping.
fn encode(request: &Batch<'_>) -> std::result::Result<Vec<u8>, AppError> {
    let recipe = RECIPE.as_bytes();
    let recipe_length = u16::try_from(recipe.len())
        .map_err(|_| AppError::internal("embedding recipe is too long"))?;
    let count = u32::try_from(request.inputs.len())
        .map_err(|_| AppError::internal("embedding batch is too large"))?;
    let mut bytes =
        Vec::with_capacity(4 + 2 + recipe.len() + REQUEST_ID_BYTES + 4 + request.inputs.len() * 8);
    bytes.extend_from_slice(REQUEST_MAGIC);
    bytes.extend_from_slice(&recipe_length.to_le_bytes());
    bytes.extend_from_slice(recipe);
    bytes.extend_from_slice(request.request_id.as_bytes());
    bytes.extend_from_slice(&count.to_le_bytes());
    for input in &request.inputs {
        let length = u32::try_from(input.text.len())
            .map_err(|_| AppError::internal("embedding input is too long"))?;
        bytes.extend_from_slice(&input.id.to_le_bytes());
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.extend_from_slice(input.text.as_bytes());
    }
    Ok(bytes)
}

fn invalid() -> AppError {
    AppError::model("embedding service returned an invalid response")
}

fn mismatched() -> AppError {
    AppError::model("embedding service returned mismatched batch")
}

/// Decode f16 without a dependency; the worker emits IEEE 754 half bits.
fn half_to_f32(bits: u16) -> f32 {
    let sign = u32::from(bits & 0x8000) << 16;
    let exponent = u32::from((bits >> 10) & 0x1f);
    let mantissa = u32::from(bits & 0x03ff);
    let value = match (exponent, mantissa) {
        (0, 0) => sign,
        (0, mantissa) => {
            let mut normalized = mantissa;
            let mut shift = 0;
            while normalized & 0x0400 == 0 {
                normalized <<= 1;
                shift += 1;
            }
            let exponent = 113 - shift;
            sign | (exponent << 23) | ((normalized & 0x03ff) << 13)
        }
        (0x1f, mantissa) => sign | 0x7f80_0000 | (mantissa << 13),
        (exponent, mantissa) => sign | ((exponent + 127 - 15) << 23) | (mantissa << 13),
    };
    f32::from_bits(value)
}

fn validate(bytes: &[u8], request: &Batch<'_>) -> std::result::Result<Result, AppError> {
    if bytes.len() < RESPONSE_HEADER_BYTES || &bytes[..4] != RESPONSE_MAGIC {
        return Err(invalid());
    }
    if &bytes[4..4 + REQUEST_ID_BYTES] != request.request_id.as_bytes() {
        return Err(mismatched());
    }
    let inference_milliseconds = u64::from(u32::from_le_bytes(
        bytes[36..40].try_into().map_err(|_| invalid())?,
    ));
    let count = usize::try_from(u32::from_le_bytes(
        bytes[40..44].try_into().map_err(|_| invalid())?,
    ))
    .map_err(|_| invalid())?;
    if count != request.inputs.len() {
        return Err(mismatched());
    }
    if bytes.len() != RESPONSE_HEADER_BYTES + count * VECTOR_BYTES {
        return Err(invalid());
    }
    let mut vectors = vec![None; count];
    let mut cursor = RESPONSE_HEADER_BYTES;
    for _ in 0..count {
        let id = usize::try_from(u32::from_le_bytes(
            bytes[cursor..cursor + 4]
                .try_into()
                .map_err(|_| invalid())?,
        ))
        .map_err(|_| invalid())?;
        cursor += 4;
        let Some(slot) = vectors.get_mut(id) else {
            return Err(AppError::model(
                "embedding service returned an unknown input ID",
            ));
        };
        if slot.is_some() {
            return Err(AppError::model(
                "embedding service returned duplicate input IDs",
            ));
        }
        let mut values = Vec::with_capacity(DIMENSIONS);
        for _ in 0..DIMENSIONS {
            let bits = u16::from_le_bytes(
                bytes[cursor..cursor + 2]
                    .try_into()
                    .map_err(|_| invalid())?,
            );
            cursor += 2;
            values.push(half_to_f32(bits));
        }
        let norm = values.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>();
        if values.iter().any(|v| !v.is_finite() || v.abs() > 1.001)
            || !(0.99..=1.01).contains(&norm)
        {
            return Err(AppError::model(
                "embedding service returned invalid vectors",
            ));
        }
        *slot = Some(
            super::retrieval::quantize_vector(&values)
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
        inference_milliseconds,
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
            ..
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
        let mut batches = Vec::new();
        let mut remaining = texts;
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
            batches.push(&remaining[..count]);
            remaining = &remaining[count..];
        }
        let mut result = Result {
            vectors: Vec::new(),
            inference_milliseconds: 0,
        };
        for group in batches.chunks(REQUEST_CONCURRENCY) {
            let completed = futures_util::future::join_all(
                group
                    .iter()
                    .map(|texts| send(&self.http, &self.url, &self.token, texts)),
            )
            .await;
            for batch in completed {
                let batch = batch?;
                result.vectors.extend(batch.vectors);
                result.inference_milliseconds += batch.inference_milliseconds;
            }
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

#[tracing::instrument(
    name = "remote_embedding_request",
    skip_all,
    fields(
        inputs = texts.len(),
        request_bytes = tracing::field::Empty,
        request_frame_bytes = tracing::field::Empty,
        roundtrip_ms = tracing::field::Empty,
        inference_ms = tracing::field::Empty,
        response_bytes = tracing::field::Empty
    )
)]
async fn send(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    texts: &[&str],
) -> std::result::Result<Result, AppError> {
    let started = std::time::Instant::now();
    let request = request(texts)?;
    let frame = encode(&request)?;
    let request_frame_bytes = frame.len();
    let (body, compressed) = if frame.len() >= COMPRESSION_THRESHOLD {
        (
            zstd::stream::encode_all(frame.as_slice(), COMPRESSION_LEVEL)
                .map_err(|_| AppError::internal("cannot compress embedding request"))?,
            true,
        )
    } else {
        (frame, false)
    };
    let request_bytes = body.len();
    // No response bodies or transport errors enter diagnostics: they may echo
    // credentials or private input. A timeout can still have incurred GPU cost.
    let failure = || {
        AppError::model(
            "remote embedding request failed; it was not retried and may have incurred inference cost",
        )
    };
    let mut builder = client
        .post(url)
        .bearer_auth(token)
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream");
    if compressed {
        builder = builder.header(reqwest::header::CONTENT_ENCODING, "zstd");
    }
    let mut response = builder.body(body).send().await.map_err(|_| failure())?;
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
    let response_bytes = bytes.len();
    let result = validate(&bytes, &request)?;
    let span = tracing::Span::current();
    span.record(
        "roundtrip_ms",
        u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    );
    span.record("request_bytes", request_bytes);
    span.record("request_frame_bytes", request_frame_bytes);
    span.record("inference_ms", result.inference_milliseconds);
    span.record("response_bytes", response_bytes);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn half(value: f32) -> u16 {
        match value {
            0.0 => 0x0000,
            1.0 => 0x3c00,
            -1.0 => 0xbc00,
            value if value.is_nan() => 0x7e00,
            _ => unimplemented!("test vectors only use 0, 1, -1 and NaN"),
        }
    }

    fn basis(index: usize) -> Vec<f32> {
        let mut values = vec![0.0; DIMENSIONS];
        values[index] = 1.0;
        values
    }

    fn frame(
        request_id: &[u8],
        inference_milliseconds: u32,
        vectors: &[(u32, Vec<f32>)],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(RESPONSE_MAGIC);
        bytes.extend_from_slice(request_id);
        bytes.extend_from_slice(&inference_milliseconds.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(vectors.len()).unwrap().to_le_bytes());
        for (id, values) in vectors {
            bytes.extend_from_slice(&id.to_le_bytes());
            for value in values {
                bytes.extend_from_slice(&half(*value).to_le_bytes());
            }
        }
        bytes
    }

    fn response(request: &Batch<'_>) -> Vec<u8> {
        frame(
            request.request_id.as_bytes(),
            12,
            &(0..request.inputs.len())
                .map(|id| (u32::try_from(id).unwrap(), basis(id)))
                .collect::<Vec<_>>(),
        )
    }

    fn decode_request(bytes: &[u8]) -> (Vec<u8>, Vec<(u32, String)>) {
        assert_eq!(&bytes[..4], REQUEST_MAGIC);
        let recipe_length = usize::from(u16::from_le_bytes(bytes[4..6].try_into().unwrap()));
        assert_eq!(&bytes[6..6 + recipe_length], RECIPE.as_bytes());
        let mut cursor = 6 + recipe_length;
        let request_id = bytes[cursor..cursor + REQUEST_ID_BYTES].to_vec();
        cursor += REQUEST_ID_BYTES;
        let count = usize::try_from(u32::from_le_bytes(
            bytes[cursor..cursor + 4].try_into().unwrap(),
        ))
        .unwrap();
        cursor += 4;
        let mut inputs = Vec::with_capacity(count);
        for _ in 0..count {
            let id = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
            cursor += 4;
            let length = usize::try_from(u32::from_le_bytes(
                bytes[cursor..cursor + 4].try_into().unwrap(),
            ))
            .unwrap();
            cursor += 4;
            let text = std::str::from_utf8(&bytes[cursor..cursor + length])
                .unwrap()
                .to_owned();
            cursor += length;
            inputs.push((id, text));
        }
        assert_eq!(cursor, bytes.len());
        (request_id, inputs)
    }

    #[test]
    fn remote_embeddings_preserve_input_identity_when_results_arrive_reordered() {
        let request = request(&["first", "second"]).unwrap();
        let reply = frame(
            request.request_id.as_bytes(),
            12,
            &[(1, basis(1)), (0, basis(0))],
        );
        let result = validate(&reply, &request).unwrap();
        assert_eq!(result.vectors[0][0], 127.0);
        assert_eq!(result.vectors[0][1], 0.0);
        assert_eq!(result.vectors[1][0], 0.0);
        assert_eq!(result.vectors[1][1], 127.0);
    }

    #[test]
    fn remote_embeddings_reject_wrong_identity_and_malformed_vectors() {
        let request = request(&["first", "second"]).unwrap();
        let identity = request.request_id.as_bytes();
        for defect in 0..8 {
            let reply = match defect {
                0 => {
                    let mut bytes = response(&request);
                    bytes[5] ^= 0xff;
                    bytes
                }
                1 => {
                    let mut bytes = response(&request);
                    bytes.push(0);
                    bytes
                }
                2 => {
                    let mut bytes = response(&request);
                    let length = bytes.len();
                    bytes.truncate(length - VECTOR_BYTES);
                    bytes
                }
                3 => frame(identity, 12, &[(0, basis(0)), (0, basis(1))]),
                4 => frame(identity, 12, &[(0, basis(0)), (2, basis(1))]),
                5 => {
                    let mut bytes = response(&request);
                    let length = bytes.len();
                    bytes.truncate(length - 2);
                    bytes
                }
                6 => {
                    let mut values = basis(0);
                    values[0] = f32::NAN;
                    frame(identity, 12, &[(0, values), (1, basis(1))])
                }
                _ => frame(
                    identity,
                    12,
                    &[(0, vec![0.0; DIMENSIONS]), (1, vec![0.0; DIMENSIONS])],
                ),
            };
            assert!(
                validate(&reply, &request).is_err(),
                "accepted defect {defect}"
            );
        }
    }

    #[test]
    fn remote_embeddings_round_trip_binary_frame_and_bound_inputs() {
        let batch = request(&["ab", "c"]).unwrap();
        let bytes = encode(&batch).unwrap();
        let (id, inputs) = decode_request(&bytes);
        assert_eq!(id, batch.request_id.as_bytes());
        assert_eq!(inputs, vec![(0, "ab".to_owned()), (1, "c".to_owned())]);
        assert_eq!(half_to_f32(0x3c00), 1.0);
        assert_eq!(half_to_f32(0xbc00), -1.0);
        assert_eq!(half_to_f32(0x0000), 0.0);
        assert_eq!(half_to_f32(0x0001), 2f32.powi(-24));
        assert_eq!(half_to_f32(0x03ff), 1023.0 * 2f32.powi(-24));
        assert!(half_to_f32(0x7e00).is_nan());
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
        let app = axum::Router::new().route(
            "/",
            axum::routing::post(
                move |headers: axum::http::HeaderMap, body: axum::body::Bytes| {
                    let observed = Arc::clone(&observed);
                    async move {
                        observed.fetch_add(1, Ordering::SeqCst);
                        let body = if headers.contains_key(axum::http::header::CONTENT_ENCODING) {
                            zstd::stream::decode_all(body.as_ref()).unwrap()
                        } else {
                            body.to_vec()
                        };
                        let (request_id, inputs) = decode_request(&body);
                        let vectors = inputs
                            .iter()
                            .map(|(id, text)| {
                                let mut values = vec![0.0; DIMENSIONS];
                                values[usize::from(text.as_bytes()[0] - b'a')] = 1.0;
                                (*id, values)
                            })
                            .collect::<Vec<_>>();
                        (
                            [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
                            frame(&request_id, 2, &vectors),
                        )
                    }
                },
            ),
        );
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
            local_below: 0,
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
        use axum::{http::StatusCode, routing::post};
        let app = axum::Router::new()
            .route(
                "/valid",
                post(
                    |headers: axum::http::HeaderMap, body: axum::body::Bytes| async move {
                        assert_eq!(headers["authorization"], "Bearer test-credential");
                        let (request_id, inputs) = decode_request(&body);
                        assert_eq!(inputs, vec![(0, "hello".to_owned())]);
                        (
                            [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
                            frame(&request_id, 7, &[(0, basis(0))]),
                        )
                    },
                ),
            )
            .route(
                "/malformed",
                post(|| async { "private-input invalid response" }),
            )
            .route(
                "/oversized",
                post(|| async { vec![0u8; MAX_RESPONSE_BYTES + 1] }),
            )
            .route(
                "/redirect",
                post(|| async { (StatusCode::TEMPORARY_REDIRECT, [("location", "/valid")]) }),
            );
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
            local_below: 0,
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
