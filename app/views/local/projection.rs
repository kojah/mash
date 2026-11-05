//! Native and connector point writes for a captured local silver snapshot.
use super::{AppError, Arc, Input, NodeViews, Scope, Store, ViewChat};
use crate::views::materialize::Writer;

pub(super) async fn native(
    store: &Store,
    scope: &Scope,
    writer: &mut Writer<'_>,
    input: &Arc<Input>,
    previous: Option<&NodeViews>,
    node: &mut NodeViews,
) -> Result<(), AppError> {
    for (id, chat) in &input.chats {
        if let Some(old) = previous
            .and_then(|n| n.chats.get(id))
            .filter(|old| old.complete && old.fingerprint == chat.fingerprint)
        {
            node.chats.insert(id.clone(), old.clone());
            continue;
        }
        let source = Arc::clone(input);
        let chat_id = id.clone();
        let messages = crate::blocking(move || source.read_chat(&chat_id)).await?;
        store
            .retain_documents(
                scope,
                id,
                messages
                    .iter()
                    .filter(|message| !message.projection.is_empty())
                    .map(|message| message.id.clone())
                    .collect(),
            )
            .await?;
        let built = writer.write_chat(&chat.record, &messages).await?;
        node.chats.insert(
            id.clone(),
            ViewChat {
                fingerprint: chat.fingerprint.clone(),
                embeddings: built.embeddings,
                complete: built.complete,
            },
        );
    }
    writer.flush().await
}

pub(super) async fn connectors(
    store: &Store,
    scope: &Scope,
    writer: &mut Writer<'_>,
    input: &Input,
    previous: Option<&NodeViews>,
    node: &mut NodeViews,
) -> Result<(), AppError> {
    let anchors = input
        .connectors
        .iter()
        .map(|(_, point)| point.unit.anchor.clone())
        .collect();
    store.retain_connector_points(scope, anchors).await?;
    for batch in input.connectors.chunks(crate::views::shared::BATCH_ROWS) {
        let changed = batch
            .iter()
            .filter(|(digest, point)| {
                previous.and_then(|node| node.connectors.get(&point.unit.anchor)) != Some(digest)
            })
            .collect::<Vec<_>>();
        let points = changed
            .into_iter()
            .map(|(_, point)| point.clone())
            .collect::<Vec<_>>();
        writer.write_connectors(&points).await?;
        for (digest, point) in batch {
            node.connectors
                .insert(point.unit.anchor.clone(), digest.clone());
        }
    }
    writer.flush().await?;

    Ok(())
}
