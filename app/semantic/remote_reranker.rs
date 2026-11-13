//! Opt-in remote contextual reranking: one bounded attempt per search pass,
//! never retried. A search that cannot use the worker reranks on the local
//! CPU and reports why, so one pass never mixes engines.
use crate::AppError;
use crate::config::Reranking;
use crate::observability::retrieval::RerankerFallback;
use std::sync::{Arc, Mutex};
use std::time::Duration;

// Shared with deployment/reranking/contract.py. The recipe pins the model
// revision Mash installs, so remote and CPU logits are interchangeable.
pub(crate) const REVISION: &str = "b8c14f4e723d9e0aab4732a7b7b93741eeeb77c2";
pub(crate) const RECIPE: &str = "jina-reranker-v1-turbo-en:b8c14f4e723d9e0aab4732a7b7b93741eeeb77c2:onnx-f32:pair-512-longest-first:logit0:v1";
const _: () = assert!(RECIPE.len() > REVISION.len());
const MAX_QUERY_BYTES: usize = 16 * 1024;
const MAX_DOCUMENTS: usize = 64;
const MAX_DOCUMENT_BYTES: usize = 1024 * 1024;
const MAX_SCORE_MAGNITUDE: f32 = 1000.0;
const REQUEST_MAGIC: &[u8; 4] = b"MRK1";
const RESPONSE_MAGIC: &[u8; 4] = b"MRR1";
const REQUEST_ID_BYTES: usize = 32;
const RESPONSE_HEADER_BYTES: usize = 4 + REQUEST_ID_BYTES + 4 + 4;
const SCORE_BYTES: usize = 4 + 4;
const MAX_RESPONSE_BYTES: usize = RESPONSE_HEADER_BYTES + MAX_DOCUMENTS * SCORE_BYTES;
const COMPRESSION_THRESHOLD: usize = 1024;
const COMPRESSION_LEVEL: i32 = 3;
/// A warm-up waits out a cold start (container, CUDA and model load) so the
/// worker is ready when a search follows; searches never wait this long.
pub(crate) const WARM_UP_TIMEOUT: Duration = Duration::from_secs(120);
/// Idle connections outlive the gap between a warm-up and the first search,
/// so a long-running process does not repeat its TLS handshake.
const POOL_IDLE: Duration = Duration::from_secs(600);

pub(crate) struct Client {
    http: reqwest::Client,
    url: String,
    token: String,
    timeout: Duration,
}

pub(crate) struct Scores {
    /// One raw logit per document, in input order.
    pub(crate) scores: Vec<f32>,
    pub(crate) inference_milliseconds: u64,
}

// One client per process keeps its connection pool across searches; MCP and
// the dashboard reuse the connection their warm-up opened.
static CLIENT: Mutex<Option<Arc<Client>>> = Mutex::new(None);

/// The configured remote reranker for a search with these installed assets,
/// or why it cannot be used. `None` means local CPU reranking by configuration.
pub(crate) fn connect(
    configuration: &Reranking,
    asset_root: &std::path::Path,
) -> Option<Result<Arc<Client>, RerankerFallback>> {
    if matches!(configuration, Reranking::Local {}) {
        return None;
    }
    let installed = super::models::snapshot(asset_root, super::RERANKER_REPOSITORY)
        .ok()
        .and_then(|path| path.file_name().map(std::ffi::OsStr::to_owned));
    if installed.as_deref() != Some(std::ffi::OsStr::new(REVISION)) {
        return Some(Err(RerankerFallback::RecipeMismatch));
    }
    Some(shared_client(configuration))
}

fn shared_client(configuration: &Reranking) -> Result<Arc<Client>, RerankerFallback> {
    let Reranking::Remote {
        url,
        token_env,
        timeout_milliseconds,
    } = configuration
    else {
        return Err(RerankerFallback::Unavailable);
    };
    let token = std::env::var(token_env)
        .ok()
        .filter(|token| !token.trim().is_empty())
        .ok_or(RerankerFallback::CredentialMissing)?;
    let timeout = Duration::from_millis(u64::from(*timeout_milliseconds));
    let mut cached = CLIENT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(client) = cached.as_ref()
        && client.url == *url
        && client.token == token
        && client.timeout == timeout
    {
        return Ok(Arc::clone(client));
    }
    let client = Arc::new(Client {
        http: http_client().map_err(|_| RerankerFallback::Unavailable)?,
        url: url.clone(),
        token,
        timeout,
    });
    *cached = Some(Arc::clone(&client));
    Ok(client)
}

fn http_client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .pool_idle_timeout(POOL_IDLE)
        .build()
        .map_err(|_| AppError::model("cannot create reranking service client"))
}

/// Start the worker if it is cold, without inference. Fire and forget: the
/// caller ignores the result, which is only for tests and tracing.
pub(crate) async fn warm_up(configuration: &Reranking) -> Result<(), AppError> {
    configuration.validate()?;
    let client = shared_client(configuration)
        .map_err(|_| AppError::configuration("reranking service token is unavailable"))?;
    let request = Request::new("", &[]);
    let started = std::time::Instant::now();
    let result = client.send(&request, WARM_UP_TIMEOUT).await;
    tracing::info!(
        name: "remote_reranking_warm_up",
        roundtrip_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        ok = result.is_ok(),
    );
    result
        .map(|_| ())
        .map_err(|_| AppError::model("reranking warm-up failed; it was not retried"))
}

/// What makes a search likely enough to warm the worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Occasion {
    /// MCP or dashboard start: warm unless `MASH_RERANK_WARMUP=off`.
    LongRunning,
    /// An agent session's catch-up `mash index`: warm only when the plugin
    /// asked with `MASH_RERANK_WARMUP=on`, so routine or fleet indexing never
    /// starts a GPU.
    CatchUp,
}

/// A bounded background warm-up for a process started on this occasion, if
/// remote reranking is configured and not opted out. It never fails startup.
pub(crate) fn warm_up_task(
    occasion: Occasion,
    config_path: Option<std::path::PathBuf>,
) -> Option<impl std::future::Future<Output = ()> + Send + 'static> {
    let setting = std::env::var("MASH_RERANK_WARMUP")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let wanted = match occasion {
        Occasion::LongRunning => !matches!(setting.as_str(), "0" | "off" | "false"),
        Occasion::CatchUp => setting == "on",
    };
    wanted.then_some(async move {
        let Ok(configuration) =
            crate::blocking(move || crate::config::load(config_path.as_deref())).await
        else {
            return;
        };
        if warms(&configuration) {
            let _ = warm_up(&configuration.reranking).await;
        }
    })
}

/// Whether a search here is likely to use the remote worker: one is
/// configured, and the default search reranks. With word search as the
/// default the worker is not woken (or billed) for searches that never use
/// it; an explicit search by meaning then starts it, or falls back to CPU.
fn warms(configuration: &crate::config::ResolvedConfig) -> bool {
    !matches!(configuration.reranking, Reranking::Local {})
        && configuration.views.query.intent.semantic()
        && configuration.views.query.ranking == crate::views::config::Ranking::ContextualRerank
}

impl Client {
    /// One attempt within the configured bound; the caller reranks on CPU
    /// on any error. A timed-out request may still have incurred GPU cost.
    pub(crate) async fn rerank(
        &self,
        query: &str,
        documents: &[&str],
    ) -> Result<Scores, RerankerFallback> {
        if query.is_empty()
            || query.len() > MAX_QUERY_BYTES
            || documents.is_empty()
            || documents.len() > MAX_DOCUMENTS
            || documents.iter().map(|text| text.len()).sum::<usize>() > MAX_DOCUMENT_BYTES
        {
            return Err(RerankerFallback::InputLimit);
        }
        self.send(&Request::new(query, documents), self.timeout)
            .await
    }

    #[tracing::instrument(
        name = "remote_reranking_request",
        skip_all,
        fields(
            documents = request.documents.len(),
            request_bytes = tracing::field::Empty,
            roundtrip_ms = tracing::field::Empty,
            inference_ms = tracing::field::Empty,
            fallback = tracing::field::Empty,
        )
    )]
    async fn send(
        &self,
        request: &Request<'_>,
        bound: Duration,
    ) -> Result<Scores, RerankerFallback> {
        let started = std::time::Instant::now();
        let result = tokio::time::timeout(bound, self.exchange(request, bound))
            .await
            .unwrap_or(Err(RerankerFallback::Timeout));
        let span = tracing::Span::current();
        span.record(
            "roundtrip_ms",
            u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        );
        match &result {
            Ok(scores) => {
                span.record("inference_ms", scores.inference_milliseconds);
            }
            Err(reason) => {
                span.record("fallback", tracing::field::debug(reason));
            }
        }
        result
    }

    async fn exchange(
        &self,
        request: &Request<'_>,
        bound: Duration,
    ) -> Result<Scores, RerankerFallback> {
        let frame = request.encode();
        let compressed = frame.len() >= COMPRESSION_THRESHOLD;
        let body = if compressed {
            zstd::stream::encode_all(frame.as_slice(), COMPRESSION_LEVEL)
                .map_err(|_| RerankerFallback::InputLimit)?
        } else {
            frame
        };
        tracing::Span::current().record("request_bytes", body.len());
        let mut builder = self
            .http
            .post(&self.url)
            .bearer_auth(&self.token)
            .timeout(bound)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream");
        if compressed {
            builder = builder.header(reqwest::header::CONTENT_ENCODING, "zstd");
        }
        // Transport errors and response bodies never enter diagnostics: they
        // may echo credentials or private input.
        let timed_out = |error: reqwest::Error| {
            if error.is_timeout() {
                RerankerFallback::Timeout
            } else {
                RerankerFallback::Unavailable
            }
        };
        let mut response = builder.body(body).send().await.map_err(timed_out)?;
        if !response.status().is_success() {
            return Err(RerankerFallback::Unavailable);
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
        {
            return Err(RerankerFallback::InvalidResponse);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(timed_out)? {
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(RerankerFallback::InvalidResponse);
            }
            bytes.extend_from_slice(&chunk);
        }
        request
            .validate(&bytes)
            .ok_or(RerankerFallback::InvalidResponse)
    }
}

struct Request<'a> {
    id: blake3::Hash,
    query: &'a str,
    documents: &'a [&'a str],
}

impl<'a> Request<'a> {
    fn new(query: &'a str, documents: &'a [&'a str]) -> Self {
        let mut hash = blake3::Hasher::new();
        hash.update(RECIPE.as_bytes());
        hash.update(&query.len().to_le_bytes());
        hash.update(query.as_bytes());
        for text in documents {
            hash.update(&text.len().to_le_bytes());
            hash.update(text.as_bytes());
        }
        Self {
            id: hash.finalize(),
            query,
            documents,
        }
    }

    /// Lengths fit in u32: the caller bounded every input far below that.
    fn encode(&self) -> Vec<u8> {
        let length = |text: &str| u32::try_from(text.len()).unwrap_or(u32::MAX);
        let mut bytes = Vec::with_capacity(
            64 + RECIPE.len()
                + self.query.len()
                + self
                    .documents
                    .iter()
                    .map(|text| 8 + text.len())
                    .sum::<usize>(),
        );
        bytes.extend_from_slice(REQUEST_MAGIC);
        bytes.extend_from_slice(
            &u16::try_from(RECIPE.len())
                .unwrap_or(u16::MAX)
                .to_le_bytes(),
        );
        bytes.extend_from_slice(RECIPE.as_bytes());
        bytes.extend_from_slice(self.id.as_bytes());
        bytes.extend_from_slice(&length(self.query).to_le_bytes());
        bytes.extend_from_slice(self.query.as_bytes());
        bytes.extend_from_slice(
            &u32::try_from(self.documents.len())
                .unwrap_or(u32::MAX)
                .to_le_bytes(),
        );
        for (id, text) in self.documents.iter().enumerate() {
            bytes.extend_from_slice(&u32::try_from(id).unwrap_or(u32::MAX).to_le_bytes());
            bytes.extend_from_slice(&length(text).to_le_bytes());
            bytes.extend_from_slice(text.as_bytes());
        }
        bytes
    }

    /// Accept only a reply to this exact request with one bounded, finite
    /// score per document.
    fn validate(&self, bytes: &[u8]) -> Option<Scores> {
        let word = |offset: usize| {
            bytes
                .get(offset..offset + 4)
                .and_then(|slice| slice.try_into().ok())
                .map(u32::from_le_bytes)
        };
        if bytes.get(..4)? != RESPONSE_MAGIC
            || bytes.get(4..4 + REQUEST_ID_BYTES)? != self.id.as_bytes()
        {
            return None;
        }
        let inference_milliseconds = u64::from(word(36)?);
        let count = usize::try_from(word(40)?).ok()?;
        if count != self.documents.len()
            || bytes.len() != RESPONSE_HEADER_BYTES + count * SCORE_BYTES
        {
            return None;
        }
        let mut scores = vec![None; count];
        for entry in 0..count {
            let offset = RESPONSE_HEADER_BYTES + entry * SCORE_BYTES;
            let slot = scores.get_mut(usize::try_from(word(offset)?).ok()?)?;
            let score = f32::from_bits(word(offset + 4)?);
            if slot.is_some() || !score.is_finite() || score.abs() > MAX_SCORE_MAGNITUDE {
                return None;
            }
            *slot = Some(score);
        }
        Some(Scores {
            scores: scores.into_iter().collect::<Option<_>>()?,
            inference_milliseconds,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests;
