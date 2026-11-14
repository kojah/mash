//! Agent message writes for a captured local silver snapshot.
use super::{AppError, Arc, Input, NodeViews, ProjectionEvidence, Scope, Store};
use crate::views::documents::{ChatRecord, SearchMessage};
use crate::views::materialize::Writer;

/// Chats materialized together so remote inference fills whole batches across
/// chat boundaries. Kept below the replay window because local chats can be
/// much larger and the snapshot is held in memory while preparing.
const LOCAL_CHAT_WINDOW: usize = 64;

pub(super) async fn native(
    store: &Store,
    scope: &Scope,
    writer: &mut Writer<'_>,
    input: &Arc<Input>,
    previous: Option<&NodeViews>,
    node: &mut NodeViews,
) -> Result<(), AppError> {
    let mut pending = Vec::new();
    let mut identities = Vec::new();
    for (id, chat) in &input.chats {
        // An unchanged chat is left as it is when its documents are complete,
        // or when this pass can no longer embed: its text is already stored,
        // and rewriting it without vectors on every pass would cost a whole
        // index for nothing. It stays incomplete for a later pass.
        if let Some(old) = previous
            .and_then(|n| n.projections.get(&id.clone()))
            .filter(|old| old.fingerprint == chat.fingerprint)
            .filter(|old| old.complete || writer.inference_failed())
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
        identities.push((id.clone(), chat.fingerprint.clone()));
        pending.push((chat.record.clone(), messages));
        if pending.len() >= LOCAL_CHAT_WINDOW {
            flush(writer, &mut pending, &mut identities, node).await?;
        }
    }
    flush(writer, &mut pending, &mut identities, node).await?;
    writer.flush().await
}

async fn flush(
    writer: &mut Writer<'_>,
    pending: &mut Vec<(ChatRecord, Vec<SearchMessage>)>,
    identities: &mut Vec<(String, String)>,
    node: &mut NodeViews,
) -> Result<(), AppError> {
    if pending.is_empty() {
        return Ok(());
    }
    let built = writer.write_chats(pending.as_slice()).await?;
    for ((id, fingerprint), (_, build)) in identities.drain(..).zip(built) {
        node.projections.insert(
            id,
            ProjectionEvidence {
                fingerprint,
                embeddings: build.embeddings,
                complete: build.complete,
            },
        );
    }
    pending.clear();
    Ok(())
}
