use super::{ApiError, App, Identity, json};
use crate::{
    AppError,
    attachments::{self, Availability, Listing, Metadata, Retrieved},
};
use axum::{
    Extension, Json,
    extract::{State, rejection::JsonRejection},
};
use std::path::PathBuf;

fn directory(app: &App, id: &Identity, node: &str) -> Option<PathBuf> {
    app.attachments_dir.as_ref().map(|root| {
        root.join(blake3::hash(id.group_id.as_bytes()).to_hex().as_str())
            .join(blake3::hash(node.as_bytes()).to_hex().as_str())
    })
}

async fn known(
    app: &App,
    identity: &Identity,
    input: &attachments::Request,
) -> Result<Vec<Metadata>, ApiError> {
    let rows = app
        .store
        .view(
            identity,
            super::protocol::View {
                node_id: input.node_id.clone(),
                id: input.message_id.clone(),
                context: 0,
                content: crate::storage::MessageContent::Canonical,
            },
        )
        .await?;
    let mut records = rows
        .into_iter()
        .find(|m| m.id == input.message_id)
        .ok_or_else(|| {
            ApiError(
                axum::http::StatusCode::NOT_FOUND,
                AppError::usage("published message not found"),
            )
        })?
        .attachments;
    attachments::validate_metadata(&records)?;
    app.store.responses().attachments(&mut records);
    Ok(records)
}

pub(super) async fn list(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
    input: Result<Json<attachments::Request>, JsonRejection>,
) -> Result<Json<Listing>, ApiError> {
    let input = json(input)?;
    let records = match known(&app, &id, &input).await {
        Ok(records) => records,
        Err(ApiError(status, _)) if status == axum::http::StatusCode::NOT_FOUND => Vec::new(),
        Err(error) => return Err(error),
    };
    let root = directory(&app, &id, &input.node_id);
    let records = crate::blocking(move || {
        let mut records = records;
        for record in &mut records {
            if record.availability == Availability::Retained
                && root.as_ref().is_none_or(|root| {
                    record
                        .digest
                        .as_deref()
                        .is_none_or(|digest| !root.join(digest).is_file())
                })
            {
                record.availability = Availability::NotUploaded;
            }
        }
        Ok(records)
    })
    .await?;
    Ok(Json(Listing {
        message_id: input.message_id,
        attachments: records,
    }))
}

pub(super) async fn get(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
    input: Result<Json<attachments::Request>, JsonRejection>,
) -> Result<Json<Retrieved>, ApiError> {
    let input = json(input)?;
    let attachment = known(&app, &id, &input)
        .await?
        .into_iter()
        .find(|m| Some(&m.id) == input.attachment_id.as_ref())
        .ok_or_else(|| AppError::usage("attachment not found on published message"))?;
    let root = directory(&app, &id, &input.node_id)
        .ok_or_else(|| AppError::usage("shared attachment storage is disabled"))?;
    let result = crate::blocking(move || {
        let data = attachments::read(&root, &attachment)?;
        Ok(Retrieved {
            message_id: input.message_id,
            attachment,
            data,
        })
    })
    .await?;
    Ok(Json(result))
}

pub(super) async fn upload(
    State(app): State<App>,
    Extension(id): Extension<Identity>,
    request: axum::extract::Request,
) -> Result<Json<bool>, ApiError> {
    let progress = app.activity.start(
        &id.group_id,
        &id.node_id,
        crate::shared::activity::Kind::Attachment,
    );
    let permit = super::admission::reserve(&app.uploads, &app.waiters, &progress).await?;
    let file = super::upload::spool_bytes(request.into_body(), 8 * 1024 * 1024, &progress).await?;
    progress.phase(
        crate::shared::activity::Phase::Attachment,
        crate::shared::activity::Unit::Records,
    );
    let input: Retrieved = crate::blocking(move || {
        serde_json::from_reader(file.as_file())
            .map_err(|_| AppError::usage("invalid attachment upload JSON"))
    })
    .await?;
    let expected = known(
        &app,
        &id,
        &attachments::Request {
            node_id: id.node_id.clone(),
            message_id: input.message_id.clone(),
            attachment_id: Some(input.attachment.id.clone()),
        },
    )
    .await?;
    if !expected.iter().any(|m| m == &input.attachment) {
        return Err(
            AppError::usage("upload is not linked to this node's published message").into(),
        );
    }
    let root = directory(&app, &id, &id.node_id)
        .ok_or_else(|| AppError::usage("shared attachment storage is disabled"))?;
    crate::blocking(move || {
        let _permit = permit;
        let _progress = progress;
        attachments::write(&root, &input.attachment, &input.data)
    })
    .await?;
    Ok(Json(true))
}
