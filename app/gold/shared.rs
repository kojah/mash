//! Node-side gold materialization from interpreted silver conversations.
//! No provider discovery, source files, checkpoints or reconciliation.
use crate::AppError;
use crate::semantic::{self, EmbeddingPool, EmbeddingSummary, Models};
use crate::shared::protocol::{Batch, ConversationRecord, Event, MessageRecord};
use crate::shared::write_event;
use crate::silver::Conversation;
use std::path::Path;
use std::time::Instant;

pub(crate) const BATCH_ROWS: usize = 32;

#[derive(Default)]
pub(crate) struct Build {
    pub(crate) recipe: crate::gold::config::Build,
    pub(crate) messages: u64,
    pub(crate) embeddings: EmbeddingSummary,
    pub(crate) model_load_milliseconds: u64,
    pub(crate) embedding_milliseconds: u64,
}

fn elapsed(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests;

pub(crate) async fn embed_texts(
    models_dir: &Path,
    embeddings: Option<&semantic::remote::Configuration>,
    pool: &mut Option<EmbeddingPool>,
    texts: &[&str],
    result: &mut Build,
) -> Result<Vec<Vec<f64>>, AppError> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let existing = pool.take();
    let remote = embeddings.cloned();
    let root = models_dir.to_path_buf();
    let input = texts
        .iter()
        .map(|text| (*text).to_owned())
        .collect::<Vec<_>>();
    let (loaded, vectors, load_ms, embed_ms) = crate::blocking(move || {
        let loading = existing.is_none();
        let load_started = Instant::now();
        let mut loaded = match existing {
            Some(pool) => pool,
            None => crate::progress::during(crate::progress::Phase::ModelLoading, || {
                Models::new(root).with_remote(remote).load_indexer()
            })?
            .ok_or_else(|| {
                AppError::model("semantic models are not installed; run `mash models install`")
            })?,
        };
        let load_ms = if loading { elapsed(load_started) } else { 0 };
        let references = input.iter().map(String::as_str).collect::<Vec<_>>();
        let embed_started = Instant::now();
        let vectors = loaded.shared_vectors(&references)?;
        Ok((loaded, vectors, load_ms, elapsed(embed_started)))
    })
    .await?;
    *pool = Some(loaded);
    result.model_load_milliseconds += load_ms;
    result.embedding_milliseconds += embed_ms;
    Ok(vectors)
}

#[tracing::instrument(name = "gold_shared_materialize", skip_all)]
pub(crate) async fn materialize(
    file: &mut std::fs::File,
    conversation: &Conversation,
    models_dir: &Path,
    embeddings: Option<&semantic::remote::Configuration>,
    pool: &mut Option<EmbeddingPool>,
    result: &mut Build,
) -> Result<(), AppError> {
    let summary = conversation.messages.summary()?;
    let searchable = if result.recipe.users_only() {
        summary.searchable_users
    } else {
        summary.searchable
    };
    let total_vectors = result.recipe.expected_embeddings(searchable);
    let started = Instant::now();
    let initial_vectors = result.embeddings.stored_vectors;
    let mut estimate = crate::progress::Estimate::new();
    write_event(
        file,
        &Event::Batch(Batch::Conversations {
            records: vec![conversation_record(conversation)?],
        }),
    )?;
    let mut messages = conversation.messages.rows()?;
    let mut units = super::units::Stream::default();
    loop {
        let batch = messages
            .by_ref()
            .take(BATCH_ROWS)
            .collect::<Result<Vec<_>, AppError>>()?;
        if batch.is_empty() {
            break;
        }
        let (next, batch, prepared) =
            prepare_units(units, batch, conversation, &result.recipe).await?;
        units = next;
        let texts = prepared.iter().map(String::as_str).collect::<Vec<_>>();
        let mut vectors = embed_texts(models_dir, embeddings, pool, &texts, result)
            .await?
            .into_iter();
        result.embeddings.model_inferences += texts.len() as u64;
        drop(texts);
        let mut records = Vec::new();
        for message in batch {
            let projection = message.dialogue_text.as_deref().unwrap_or(&message.content);
            let vector = if result.recipe.includes_dense(&message.role, projection) {
                Some(
                    vectors
                        .next()
                        .ok_or_else(|| AppError::model("missing shared message embedding"))?,
                )
            } else {
                None
            };
            let generation = vector.as_ref().map(|_| {
                embeddings.map_or_else(
                    || semantic::SHARED_EMBEDDING_GENERATION.to_owned(),
                    |c| semantic::model_space(&c.model, c.dimensions),
                )
            });
            let has_vector = vector.is_some();
            let projection = message
                .dialogue_text
                .unwrap_or_else(|| message.content.clone());
            records.push(MessageRecord {
                id: message.id,
                conversation_id: conversation.id.clone(),
                ordinal: message.ordinal,
                context: message.context,
                role: message.role,
                content: message.content,
                projection,
                created_at: message.created_at,
                generation,
                vector,
            });
            result.messages += 1;
            result.embeddings.stored_vectors += u64::from(has_vector);
        }
        write_event(file, &Event::Batch(Batch::Messages { records }))?;
        let stored_vectors = result.embeddings.stored_vectors - initial_vectors;
        let elapsed_ms = elapsed(started);
        crate::progress::emit(&crate::progress::Event {
            stored_vectors: Some(stored_vectors),
            total_vectors: Some(total_vectors),
            eta_scope: crate::progress::EtaScope::Conversation,
            ..crate::progress::Event::new(
                crate::progress::Phase::SharedEmbeddings,
                elapsed_ms,
                estimate.observe(stored_vectors, total_vectors, elapsed_ms),
            )
        });
    }
    Ok(())
}

async fn prepare_units(
    mut units: super::units::Stream,
    batch: Vec<std::borrow::Cow<'_, crate::silver::NormalizedMessage>>,
    conversation: &Conversation,
    recipe: &crate::gold::config::Build,
) -> Result<
    (
        super::units::Stream,
        Vec<crate::silver::NormalizedMessage>,
        Vec<String>,
    ),
    AppError,
> {
    let batch = batch
        .into_iter()
        .map(std::borrow::Cow::into_owned)
        .collect::<Vec<_>>();
    let thread = conversation.id.clone();
    let title = conversation.title.clone();
    let recipe = recipe.clone();
    crate::blocking(move || {
        let mut prepared = Vec::new();
        for message in &batch {
            if let Some(text) = units.prepare(
                &recipe,
                super::units::Input {
                    thread: &thread,
                    ordinal: message.ordinal,
                    context: message.context,
                    role: &message.role,
                    projection: message.dialogue(),
                },
                title.as_deref(),
            )? {
                prepared.push(text);
            }
        }
        Ok((units, batch, prepared))
    })
    .await
}

fn conversation_record(conversation: &Conversation) -> Result<ConversationRecord, AppError> {
    Ok(ConversationRecord {
        enrichment: Some(crate::silver::enrichment::Evidence::capture(conversation)?),
        id: conversation.id.clone(),
        provider: conversation.provider,
        title: conversation.title.clone(),
        project: conversation.project.clone(),
        repository: conversation.repository.clone(),
        created_at: conversation.created_at,
        updated_at: conversation.updated_at,
    })
}
