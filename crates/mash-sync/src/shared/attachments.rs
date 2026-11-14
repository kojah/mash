//! Optional byte publication and retrieval through the authenticated shared API.
use super::client::{self, Endpoint};
use crate::{
    AppError,
    attachments::{Listing, Request, Retrieved},
    config::SharedConfig,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::path::Path;

pub async fn list(config: &SharedConfig, node: &str, message: &str) -> Result<Listing, AppError> {
    client::post(
        config,
        Endpoint::Attachments,
        &Request {
            node_id: node.into(),
            message_id: message.into(),
            attachment_id: None,
        },
    )
    .await
}
pub async fn get(
    config: &SharedConfig,
    node: &str,
    message: &str,
    attachment: &str,
) -> Result<Retrieved, AppError> {
    client::post(
        config,
        Endpoint::Attachment,
        &Request {
            node_id: node.into(),
            message_id: message.into(),
            attachment_id: Some(attachment.into()),
        },
    )
    .await
}

pub async fn publish(database: &Path, config: &SharedConfig) -> Result<(), AppError> {
    let db = database.to_path_buf();
    // Only IDs are materialized; each file is loaded and uploaded individually.
    let candidates =
        crate::blocking(move || crate::storage::Storage::open_silver(&db)?.attachment_candidates())
            .await?;
    for (message_id, attachment_id) in candidates {
        let remote = list(config, &config.node_id, &message_id).await?;
        if !remote.attachments.iter().any(|m| {
            m.id == attachment_id && m.availability == crate::attachments::Availability::NotUploaded
        }) {
            continue;
        }
        let db = database.to_path_buf();
        let lookup = message_id.clone();
        let file = crate::blocking(move || {
            crate::storage::Storage::open_silver(&db)?.get_attachment(&lookup, &attachment_id)
        })
        .await?;
        let bytes = STANDARD
            .decode(&file.data)
            .map_err(|_| AppError::database_data("invalid retained attachment bytes"))?;
        crate::shared::sync::client::upload_attachment(
            config,
            &message_id,
            &file.attachment,
            &bytes,
        )
        .await?;
    }
    Ok(())
}
