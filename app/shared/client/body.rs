//! Bounded wire preview and cooperative streamed decoding of large responses.
use super::Endpoint;
use crate::AppError;
use futures_util::{StreamExt, TryStreamExt, stream};
use serde::de::DeserializeOwned;
use std::io::{self, BufReader, Read};
use tokio::sync::SemaphorePermit;
use tokio_util::{
    bytes::Bytes,
    io::{StreamReader, SyncIoBridge},
    sync::CancellationToken,
};

const PREVIEW_BYTES: usize = 1024 * 1024;
const READER_BYTES: usize = 64 * 1024;

fn invalid() -> AppError {
    AppError::shared("invalid shared index HTTP response")
}

pub(super) async fn read<T: DeserializeOwned + Send + 'static>(
    mut response: reqwest::Response,
    endpoint: Endpoint,
    permit: SemaphorePermit<'static>,
) -> Result<T, AppError> {
    if matches!(
        endpoint,
        Endpoint::State | Endpoint::Silver | Endpoint::PublicationPlan
    ) || response
        .content_length()
        .is_some_and(|bytes| bytes > PREVIEW_BYTES as u64)
    {
        return streamed(response, Vec::new(), permit).await;
    }
    let mut preview = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| invalid())? {
        if chunk.len() > PREVIEW_BYTES - preview.len() {
            let mut prefix = Vec::with_capacity(2);
            if !preview.is_empty() {
                prefix.push(Bytes::from(preview));
            }
            prefix.push(chunk);
            return streamed(response, prefix, permit).await;
        }
        let needed = preview.len() + chunk.len();
        if needed > preview.capacity() {
            let capacity = preview
                .capacity()
                .saturating_mul(2)
                .max(needed)
                .min(PREVIEW_BYTES);
            preview.reserve_exact(capacity - preview.len());
        }
        preview.extend_from_slice(&chunk);
    }
    buffered(preview, permit).await
}

async fn buffered<T: DeserializeOwned + Send + 'static>(
    bytes: Vec<u8>,
    permit: SemaphorePermit<'static>,
) -> Result<T, AppError> {
    crate::blocking(move || {
        let _permit = permit;
        let span = tracing::info_span!(
            "shared_http_decode",
            bytes = bytes.len(),
            mode = "buffered",
            outcome = tracing::field::Empty,
        );
        let _guard = span.enter();
        let result = serde_json::from_slice(&bytes).map_err(|_| invalid());
        span.record("outcome", if result.is_ok() { "success" } else { "error" });
        result
    })
    .await
}

async fn streamed<T: DeserializeOwned + Send + 'static>(
    response: reqwest::Response,
    prefix: Vec<Bytes>,
    permit: SemaphorePermit<'static>,
) -> Result<T, AppError> {
    let cancel = CancellationToken::new();
    // Dropping the caller wakes a worker blocked on the next HTTP frame. A
    // running deserializer still owns its admission permit until completion.
    let _cancel_on_drop = cancel.clone().drop_guard();
    let frames = stream::iter(prefix.into_iter().map(Ok))
        .chain(response.bytes_stream())
        .map_err(|_| io::Error::other("shared response body read failed"))
        .take_until(cancel.cancelled_owned())
        .boxed();
    let bridge = SyncIoBridge::new(StreamReader::new(frames));
    crate::blocking(move || {
        let _permit = permit;
        let span = tracing::info_span!(
            "shared_http_decode",
            bytes = tracing::field::Empty,
            mode = "streamed",
            outcome = tracing::field::Empty,
        );
        let _guard = span.enter();
        let mut reader = BufReader::with_capacity(
            READER_BYTES,
            Counted {
                inner: bridge,
                bytes: 0,
            },
        );
        let result = serde_json::from_reader(&mut reader).map_err(|_| invalid());
        span.record("bytes", reader.get_ref().bytes);
        span.record("outcome", if result.is_ok() { "success" } else { "error" });
        result
    })
    .await
}

struct Counted<R> {
    inner: R,
    bytes: usize,
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let bytes = self.inner.read(buffer)?;
        self.bytes = self.bytes.saturating_add(bytes);
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests;
