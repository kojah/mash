//! Agent message writes for a captured local silver snapshot.
use super::{AppError, Arc, Input, NodeViews, ProjectionEvidence, Scope, Store};
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
            .and_then(|n| n.projections.get(&id.clone()))
            .filter(|old| old.complete && old.fingerprint == chat.fingerprint)
        {
            node.projections.insert(id.clone(), old.clone());
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
        node.projections.insert(
            id.clone(),
            ProjectionEvidence {
                fingerprint: chat.fingerprint.clone(),
                embeddings: built.embeddings,
                complete: built.complete,
            },
        );
    }
    writer.flush().await
}
