//! Bounded streaming upload transport; canonical event validation stays in storage.
use super::ApiError;
use crate::AppError;
use axum::body::Body;
use axum::http::StatusCode;
use futures_util::TryStreamExt;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

pub(super) async fn spool(body: Body, max_bytes: u64) -> Result<tempfile::NamedTempFile, ApiError> {
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
            if line_bytes > 8 * 1024 * 1024 || total > max_bytes {
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
        input.consume(consumed);
    }
    if line_bytes != 0 {
        return Err(AppError::usage("unterminated index event").into());
    }
    output
        .flush()
        .await
        .map_err(|_| AppError::internal("flush incoming index"))?;
    Ok(file)
}
