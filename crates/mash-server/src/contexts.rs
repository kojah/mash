//! Stream completed search contexts without waiting for the slowest anchor.
use super::{ApiError, App, Identity, json, protocol};
use crate::AppError;
use axum::{
    Json,
    body::Body,
    extract::{Extension, State, rejection::JsonRejection},
    http::header,
    response::{IntoResponse, Response},
};
use futures_util::{StreamExt, stream};
use std::collections::BTreeSet;

// Multiple concurrent searches must not multiply the eight-read budget.
static READS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(8);

pub(super) async fn read(
    State(app): State<App>,
    Extension(identity): Extension<Identity>,
    input: Result<Json<protocol::Contexts>, JsonRejection>,
) -> Result<Response, ApiError> {
    let input = json(input)?;
    if input.anchors.is_empty() || input.anchors.len() > 64 {
        return Err(AppError::usage("invalid context batch size").into());
    }
    let mut seen = BTreeSet::new();
    for anchor in &input.anchors {
        crate::shared::validation::valid_id(&anchor.id)?;
        crate::shared::validation::valid_id(&anchor.node_id)?;
        if anchor.context > 2
            || !matches!(
                anchor.content,
                crate::storage::MessageContent::SearchProjection
            )
            || !seen.insert((&anchor.node_id, &anchor.id))
        {
            return Err(AppError::usage("invalid search context batch").into());
        }
    }
    app.store.available(&identity).await?;
    let pending = stream::iter(input.anchors.into_iter().enumerate())
        .map(move |(index, input)| {
            let store = app.store.clone();
            let identity = identity.clone();
            async move {
                let result = async {
                    let _permit = READS
                        .acquire()
                        .await
                        .map_err(|_| AppError::shared("shared context admission is unavailable"))?;
                    store.view(&identity, input).await
                }
                .await;
                let result = match result {
                    Ok(mut messages) => {
                        for message in &mut messages {
                            message.content = store.responses().text(&message.content);
                            store.responses().attachments(&mut message.attachments);
                        }
                        protocol::ContextResult::Complete { index, messages }
                    }
                    Err(error) => protocol::ContextResult::Failed { index, error },
                };
                let mut bytes = serde_json::to_vec(&result)
                    .map_err(|_| std::io::Error::other("context serialization failed"))?;
                bytes.push(b'\n');
                Ok::<_, std::io::Error>(bytes)
            }
        })
        .buffer_unordered(8);
    // Dropping the response stream cancels its pending silver reads; there
    // are no detached tasks and at most eight active context reads.
    Ok((
        [(header::CONTENT_TYPE, "application/x-ndjson")],
        Body::from_stream(pending),
    )
        .into_response())
}
