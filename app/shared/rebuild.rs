//! Rebuild shared embeddings from retained silver; no provider or local DB writes.
use super::{
    client::{self, Endpoint},
    gold_rebuild::{Embedding, Event},
    publication,
    silver_read::Retained,
    silver_revision,
};
use crate::{
    AppError,
    config::SharedConfig,
    gold::shared::{self as gold, Build},
    semantic,
};
use std::{io::Write, path::Path, time::Instant};

#[derive(Debug, serde::Serialize)]
pub(crate) struct Rebuilt {
    rebuild_shared_gold: bool,
    counts: crate::storage::Counts,
    silver: silver_revision::State,
    publication_revision: i64,
    generation: String,
    embeddings: u64,
    model_inferences: u64,
    model_load_milliseconds: u64,
    embedding_milliseconds: u64,
    download_milliseconds: u64,
    publication_milliseconds: u64,
    total_milliseconds: u64,
}

async fn append(
    mut file: tempfile::NamedTempFile,
    event: Event,
) -> Result<tempfile::NamedTempFile, AppError> {
    crate::blocking(move || {
        serde_json::to_writer(file.as_file_mut(), &event)
            .map_err(|_| AppError::internal("serialize shared gold upload"))?;
        file.as_file_mut().write_all(b"\n").map_err(AppError::io)?;
        Ok(file)
    })
    .await
}

async fn materialize(
    retained: Retained,
    models: &Path,
    embeddings: Option<&semantic::remote::Configuration>,
) -> Result<(tempfile::NamedTempFile, Build), AppError> {
    let file = crate::blocking(|| tempfile::NamedTempFile::new().map_err(AppError::io)).await?;
    let mut file = append(
        file,
        Event::Start {
            publication_revision: retained.publication_revision,
            silver: retained.silver,
            generation: retained.generation,
        },
    )
    .await?;
    let total = retained
        .history
        .messages
        .values()
        .filter(|m| !m.projection.is_empty())
        .count() as u64;
    let mut messages = retained
        .history
        .messages
        .into_iter()
        .filter(|(_, m)| !m.projection.is_empty());
    let mut result = Build::default();
    let mut pool = None;
    let started = Instant::now();
    let mut estimate = crate::progress::Estimate::new();
    loop {
        let batch = messages.by_ref().take(32).collect::<Vec<_>>();
        if batch.is_empty() {
            break;
        }
        if batch.iter().any(|(key, m)| {
            key != &m.id
                || !retained
                    .history
                    .conversations
                    .contains_key(&m.conversation_id)
        }) {
            return Err(AppError::shared("invalid retained silver message identity"));
        }
        let texts = batch
            .iter()
            .map(|(_, m)| m.projection.as_str())
            .collect::<Vec<_>>();
        let vectors = gold::embed_texts(models, embeddings, &mut pool, &texts, &mut result).await?;
        if vectors.len() != batch.len() {
            return Err(AppError::model("missing shared message embedding"));
        }
        drop(texts);
        result.embeddings.model_inferences += batch.len() as u64;
        result.embeddings.stored_vectors += batch.len() as u64;
        let records = batch
            .into_iter()
            .zip(vectors)
            .map(|((_, m), vector)| Embedding {
                message_id: m.id,
                vector,
            })
            .collect();
        file = append(file, Event::Embeddings { records }).await?;
        let elapsed = crate::progress::milliseconds(started);
        crate::progress::emit(&crate::progress::Event {
            stored_vectors: Some(result.embeddings.stored_vectors),
            total_vectors: Some(total),
            ..crate::progress::Event::new(
                crate::progress::Phase::SharedEmbeddings,
                elapsed,
                estimate.observe(result.embeddings.stored_vectors, total, elapsed),
            )
        });
    }
    Ok((append(file, Event::Finish).await?, result))
}

pub(crate) async fn rebuild_gold(
    config: &SharedConfig,
    models: &Path,
    embeddings: Option<&semantic::remote::Configuration>,
) -> Result<Rebuilt, AppError> {
    let started = Instant::now();
    crate::progress::phase(crate::progress::Phase::SharedSilverRead, 0);
    let download = client::get::<Retained>(config, Endpoint::Silver);
    tokio::pin!(download);
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let retained = loop {
        tokio::select! {
            result = &mut download => break result?,
            _ = interval.tick() => crate::progress::phase(crate::progress::Phase::SharedSilverRead, crate::progress::milliseconds(started)),
        }
    };
    retained.silver.validate()?;
    if retained.publication_revision < 1 {
        return Err(AppError::shared("invalid retained silver source revision"));
    }
    let download_milliseconds = crate::progress::milliseconds(started);
    let (generation, dimensions) = embeddings.map_or_else(
        || (semantic::SHARED_EMBEDDING_GENERATION.to_owned(), 384),
        |c| (semantic::model_space(&c.model, c.dimensions), c.dimensions),
    );
    if retained.generation != generation || retained.dimensions != dimensions {
        return Err(AppError::configuration(
            "node embeddings do not match the shared model space",
        ));
    }
    let silver = retained.silver.clone();
    let publication_revision = retained.publication_revision;
    let (file, built) = materialize(retained, models, embeddings).await?;
    let publish_started = Instant::now();
    let indexed = publication::upload_to(config, &file, Endpoint::Gold).await?;
    let result = Rebuilt {
        rebuild_shared_gold: true,
        counts: indexed.counts,
        silver,
        publication_revision,
        generation,
        embeddings: built.embeddings.stored_vectors,
        model_inferences: built.embeddings.model_inferences,
        model_load_milliseconds: built.model_load_milliseconds,
        embedding_milliseconds: built.embedding_milliseconds,
        download_milliseconds,
        publication_milliseconds: crate::progress::milliseconds(publish_started),
        total_milliseconds: crate::progress::milliseconds(started),
    };
    crate::progress::phase(crate::progress::Phase::Complete, result.total_milliseconds);
    Ok(result)
}
