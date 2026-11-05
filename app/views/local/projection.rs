//! Native and connector point writes for a captured local silver snapshot.
use super::{AppError, Arc, Input, NodeViews, Path, ResolvedConfig, Scope, Store, ViewChat};
use crate::views::{materialize::NativeWriter, qdrant::Documents};

pub(super) async fn native(
    store: &Store,
    scope: &Scope,
    models: &Path,
    configuration: &ResolvedConfig,
    input: &Arc<Input>,
    previous: Option<&NodeViews>,
    node: &mut NodeViews,
) -> Result<crate::views::shared::Build, AppError> {
    let mut writer = NativeWriter::new(store, scope, models, configuration.embeddings.as_ref());
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
    writer.finish().await
}

pub(super) async fn connectors(
    store: &Store,
    scope: &Scope,
    model: (&Path, &ResolvedConfig),
    input: &Input,
    previous: Option<&NodeViews>,
    node: &mut NodeViews,
    work: &mut crate::views::shared::Build,
) -> Result<(), AppError> {
    let (models, configuration) = model;
    let anchors = input
        .connectors
        .iter()
        .map(|(_, point)| point.unit.anchor.clone())
        .collect();
    store.retain_connector_points(scope, anchors).await?;
    let mut documents = Documents::new(scope);
    let mut pool = None;
    for batch in input.connectors.chunks(crate::views::shared::BATCH_ROWS) {
        let changed = batch
            .iter()
            .filter(|(digest, point)| {
                previous.and_then(|node| node.connectors.get(&point.unit.anchor)) != Some(digest)
            })
            .collect::<Vec<_>>();
        let vectors = if configuration.views.build.dense_enabled() {
            let texts = changed
                .iter()
                .map(|(_, point)| point.unit.text.as_str())
                .collect::<Vec<_>>();
            let vectors = crate::views::shared::embed_texts(
                models,
                configuration.embeddings.as_ref(),
                &mut pool,
                &texts,
                work,
            )
            .await?;
            if vectors.len() != changed.len() {
                return Err(AppError::model("missing local connector embeddings"));
            }
            work.embeddings.model_inferences += vectors.len() as u64;
            vectors.into_iter().map(Some).collect::<Vec<_>>()
        } else {
            vec![None; changed.len()]
        };
        for ((_, point), vector) in changed.into_iter().zip(vectors) {
            let mut point = point.clone();
            point.vector = vector;
            documents.push_connector(store, scope, point).await?;
        }
        for (digest, point) in batch {
            node.connectors
                .insert(point.unit.anchor.clone(), digest.clone());
        }
    }
    documents.finish(store).await?;

    Ok(())
}
