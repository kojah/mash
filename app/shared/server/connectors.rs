//! Bounded authenticated connector uploads, using the canonical batch contract.
use super::{ApiError, App, Identity, admission};
use crate::AppError;
use crate::connectors::{Batch, MAX_BATCH_BYTES, Receipt, silver};
use axum::Json;
use axum::extract::{Extension, Request, State};
use axum::http::StatusCode;
use futures_util::TryStreamExt;

pub(super) async fn ingest(
    State(app): State<App>,
    Extension(identity): Extension<Identity>,
    request: Request,
) -> Result<Json<Receipt>, ApiError> {
    // Reserve before collecting the body. The detached admission task retains
    // this capacity through decoding and commit even if the requester vanishes.
    let permit = admission::reserve(&app.uploads)?;
    let progress = app.activity.start(
        &identity.group_id,
        &identity.node_id,
        crate::shared::activity::Kind::Connector,
    );
    let media = request
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok());
    if media
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        != Some("application/json")
    {
        return Err(ApiError(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            AppError::usage("connector batches require application/json"),
        ));
    }
    let limit = app.store.max_node_bytes().min(MAX_BATCH_BYTES as u64);
    let mut stream = request.into_body().into_data_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream
        .try_next()
        .await
        .map_err(|_| AppError::usage("incomplete connector upload"))?
    {
        if chunk.len() as u64 > limit.saturating_sub(bytes.len() as u64) {
            return Err(ApiError(
                StatusCode::PAYLOAD_TOO_LARGE,
                AppError::usage("connector upload exceeds limits"),
            ));
        }
        bytes.extend_from_slice(&chunk);
        progress.advance(chunk.len() as u64);
    }
    let receipt = admission::run(permit, async move {
        progress.phase(
            crate::shared::activity::Phase::Connector,
            crate::shared::activity::Unit::Records,
        );
        let _progress = progress;
        let batch = crate::blocking(move || {
            Batch::decode(&bytes)
                .map_err(|problem| silver::app_error(silver::Error::InvalidBatch(problem)))
        })
        .await?;
        app.store.ingest_connector(&identity, batch).await
    })
    .await?;
    Ok(Json(receipt))
}
