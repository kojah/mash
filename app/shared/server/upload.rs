//! Bounded streaming upload transport; canonical event validation stays in storage.
use super::ApiError;
use crate::AppError;
use axum::body::Body;
use axum::http::StatusCode;
use futures_util::TryStreamExt;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

pub(super) async fn spool_tracked(
    body: Body,
    max_bytes: u64,
    progress: &crate::shared::activity::Progress,
) -> Result<tempfile::NamedTempFile, ApiError> {
    spool_body(body, max_bytes, true, progress).await
}

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "legacy HTTP mutation path retained for test coverage only; /v1/sync replaces it"
    )
)]
pub(super) async fn spool_bytes(
    body: Body,
    max_bytes: u64,
    progress: &crate::shared::activity::Progress,
) -> Result<tempfile::NamedTempFile, ApiError> {
    spool_body(body, max_bytes, false, progress).await
}

async fn spool_body(
    body: Body,
    max_bytes: u64,
    line_framed: bool,
    progress: &crate::shared::activity::Progress,
) -> Result<tempfile::NamedTempFile, ApiError> {
    let file = crate::blocking(|| {
        tempfile::NamedTempFile::new().map_err(|_| AppError::internal("create incoming index"))
    })
    .await?;
    let output = file
        .reopen()
        .map_err(|_| AppError::internal("open incoming index"))?;
    let mut output = tokio::fs::File::from_std(output);
    let stream = body.into_data_stream().map_err(std::io::Error::other);
    let mut input = tokio::io::BufReader::new(tokio_util::io::StreamReader::new(stream));
    let mut line_bytes = 0usize;
    let mut total = 0u64;
    // Native history may contain a single large canonical tool result (for
    // example a Codex file-change item). Request and mutation admission budgets
    // still bound the upload; attachment bodies retain their existing cap.
    let line_limit = if line_framed { 64 } else { 8 } * 1024 * 1024;
    loop {
        let chunk = input
            .fill_buf()
            .await
            .map_err(|_| AppError::usage("incomplete index upload"))?;
        if chunk.is_empty() {
            break;
        }
        for byte in chunk {
            line_bytes += 1;
            total += 1;
            if line_bytes > line_limit || total > max_bytes {
                return Err(ApiError(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    AppError::usage("index upload exceeds limits"),
                ));
            }
            if *byte == b'\n' {
                line_bytes = 0;
            }
        }
        output
            .write_all(chunk)
            .await
            .map_err(|_| AppError::internal("write incoming index"))?;
        let consumed = chunk.len();
        progress.advance(consumed as u64);
        input.consume(consumed);
    }
    if line_framed && line_bytes != 0 {
        return Err(AppError::usage("unterminated index event").into());
    }
    output
        .flush()
        .await
        .map_err(|_| AppError::internal("flush incoming index"))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn native_upload_spools_large_records_but_preserves_request_and_attachment_limits() {
        let record = format!("{}\n", "x".repeat(9 * 1024 * 1024));
        let progress = crate::shared::activity::Progress::default();
        let accepted = spool_tracked(Body::from(record.clone()), 16 * 1024 * 1024, &progress)
            .await
            .unwrap_or_else(|error| panic!("{}", error.1.error.message));
        assert_eq!(
            accepted.as_file().metadata().unwrap().len(),
            record.len() as u64
        );
        assert!(
            spool_tracked(Body::from(record.clone()), 8 * 1024 * 1024, &progress)
                .await
                .is_err()
        );
        assert!(
            spool_bytes(Body::from(record), 16 * 1024 * 1024, &progress)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn native_upload_rejects_oversized_event_across_body_frames() {
        let frames = futures_util::stream::iter(
            (0..8193).map(|_| Ok::<_, std::io::Error>(vec![b'x'; 8192])),
        );
        let result = spool_tracked(
            Body::from_stream(frames),
            128 * 1024 * 1024,
            &crate::shared::activity::Progress::default(),
        )
        .await;
        assert!(result.is_err());
    }
}
