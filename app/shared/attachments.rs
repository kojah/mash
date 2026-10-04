//! Optional byte publication and retrieval through the authenticated shared API.
use super::client::{self, Endpoint};
use crate::{
    AppError,
    attachments::{Listing, Request, Retrieved},
    config::SharedConfig,
};
use std::path::Path;

pub(crate) async fn list(
    config: &SharedConfig,
    node: &str,
    message: &str,
) -> Result<Listing, AppError> {
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
pub(crate) async fn get(
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

pub(crate) async fn publish(database: &Path, config: &SharedConfig) -> Result<(), AppError> {
    let db = database.to_path_buf();
    // Only IDs are materialized; each file is loaded and uploaded individually.
    let candidates = crate::blocking(move || {
        crate::storage::Storage::open_catalog(&db)?.attachment_candidates()
    })
    .await?;
    for (message_id, attachment_id) in candidates {
        let remote = list(config, &config.node_id, &message_id).await?;
        if !remote.attachments.iter().any(|m| {
            m.id == attachment_id && m.availability == crate::attachments::Availability::NotUploaded
        }) {
            continue;
        }
        let db = database.to_path_buf();
        let file = crate::blocking(move || {
            crate::storage::Storage::open_catalog(&db)?.get_attachment(&message_id, &attachment_id)
        })
        .await?;
        let _: bool = client::post(config, Endpoint::AttachmentUpload, &file).await?;
    }
    Ok(())
}
