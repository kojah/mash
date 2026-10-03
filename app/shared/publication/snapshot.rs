//! Recipe-aware snapshot preparation from a consistent, read-only SQLite silver view.
use super::{Inference, SnapshotWork, write_event};
use crate::shared::{
    protocol::{Batch, BatchKind, ConversationRecord, Event, MessageRecord, Start},
    reuse::{Fingerprint, Target},
};
use crate::{AppError, gold::shared as gold, storage::Storage};
use std::{
    collections::{BTreeMap, VecDeque},
    io::{BufRead, Write},
    path::Path,
    time::Instant,
};

#[cfg(test)]
mod tests;

pub(super) fn validate_owner(
    snapshot: &crate::storage::publication::Snapshot<'_>,
    target: &Target,
) -> Result<(), AppError> {
    if target
        .instance
        .as_deref()
        .is_some_and(|instance| instance != snapshot.instance)
        || target.publication_revision > snapshot.revision
    {
        return Err(AppError::shared(
            "shared node ownership or revision changed; retry indexing or choose a new node_id",
        ));
    }
    Ok(())
}

struct Source {
    file: tempfile::NamedTempFile,
    revision: i64,
    conversations: BTreeMap<String, ConversationRecord>,
    hashes: BTreeMap<String, String>,
    visible: u64,
}

fn capture(path: &Path, target: &Target, reuse_gold: bool) -> Result<Source, AppError> {
    crate::progress::during(crate::progress::Phase::SharedSnapshot, || {
        let mut file = tempfile::NamedTempFile::new().map_err(AppError::io)?;
        let mut writer = std::io::BufWriter::with_capacity(256 * 1024, file.as_file_mut());
        let mut conversations = BTreeMap::new();
        let mut hashes = BTreeMap::new();
        let mut active: Option<(String, Fingerprint)> = None;
        let mut visible = 0;
        let revision = Storage::open_catalog(path)?.with_publication_snapshot(|snapshot| {
            validate_owner(snapshot, target)?;
            write_event(
                &mut writer,
                &Event::Start(Start {
                    reuse_gold,
                    expected_version: Some(target.publication_version.clone()),
                    gold_build: Some(target.build.clone()),
                    source_silver: Some(snapshot.source_silver.clone()),
                    instance: snapshot.instance.clone(),
                    revision: Some(snapshot.revision),
                    base_revision: None,
                    snapshot: true,
                    forgets: Vec::new(),
                }),
            )?;
            snapshot.visit(BatchKind::Tombstones, |batch| {
                write_event(&mut writer, &Event::Batch(batch))
            })?;
            snapshot.visit(BatchKind::Conversations, |batch| {
                if let Batch::Conversations { records } = &batch {
                    for conversation in records {
                        hashes.insert(
                            conversation.id.clone(),
                            Fingerprint::new(conversation)?.finish(),
                        );
                        conversations.insert(conversation.id.clone(), conversation.clone());
                    }
                }
                write_event(&mut writer, &Event::Batch(batch))
            })?;
            snapshot.visit_messages_by_thread(|mut batch| {
                if let Batch::Messages { records } = &mut batch {
                    for message in records {
                        if active
                            .as_ref()
                            .is_none_or(|(id, _)| id != &message.conversation_id)
                        {
                            if let Some((id, fingerprint)) = active.take() {
                                hashes.insert(id, fingerprint.finish());
                            }
                            let conversation =
                                conversations.get(&message.conversation_id).ok_or_else(|| {
                                    AppError::database_data("snapshot message has no thread")
                                })?;
                            active =
                                Some((conversation.id.clone(), Fingerprint::new(conversation)?));
                        }
                        if let Some((_, fingerprint)) = &mut active {
                            fingerprint.push(message)?;
                        }
                        visible += u64::from(!message.projection.is_empty());
                        // Local gold has a different recipe. Only canonical rows enter
                        // this temporary stream; shared reuse/preparation follows below.
                        message.vector = None;
                        message.generation = None;
                    }
                }
                write_event(&mut writer, &Event::Batch(batch))
            })?;
            write_event(&mut writer, &Event::Finish)
        })?;
        if let Some((id, fingerprint)) = active {
            hashes.insert(id, fingerprint.finish());
        }
        writer.flush().map_err(AppError::io)?;
        drop(writer);
        Ok(Source {
            file,
            revision,
            conversations,
            hashes,
            visible,
        })
    })
}

struct Reader {
    input: std::io::BufReader<std::fs::File>,
    pending: VecDeque<MessageRecord>,
    done: bool,
    output: tempfile::NamedTempFile,
}

impl Reader {
    fn new(input: std::fs::File) -> Result<Self, AppError> {
        Ok(Self {
            input: std::io::BufReader::with_capacity(64 * 1024, input),
            pending: VecDeque::new(),
            done: false,
            output: tempfile::NamedTempFile::new().map_err(AppError::io)?,
        })
    }

    fn next(mut self) -> Result<(Self, Vec<MessageRecord>), AppError> {
        let mut batch: Vec<MessageRecord> = Vec::new();
        loop {
            if batch.len() == gold::BATCH_ROWS
                || self.pending.front().is_some_and(|message| {
                    batch
                        .first()
                        .is_some_and(|first| first.conversation_id != message.conversation_id)
                })
            {
                break;
            }
            if let Some(message) = self.pending.pop_front() {
                batch.push(message);
                continue;
            }
            if self.done {
                break;
            }
            let mut line = String::new();
            if self.input.read_line(&mut line).map_err(AppError::io)? == 0 {
                return Err(AppError::internal("incomplete prepared silver snapshot"));
            }
            let event: Event = serde_json::from_str(&line)
                .map_err(|_| AppError::internal("decode prepared silver snapshot"))?;
            match event {
                Event::Batch(Batch::Messages { records }) => self.pending.extend(records),
                Event::Finish => self.done = true,
                event => write_event(self.output.as_file_mut(), &event)?,
            }
        }
        Ok((self, batch))
    }
}

fn validate_model(target: &Target, inference: &Inference<'_>) -> Result<(), AppError> {
    let (generation, dimensions) = inference.embeddings.map_or_else(
        || (crate::semantic::SHARED_EMBEDDING_GENERATION.to_owned(), 384),
        |config| {
            (
                crate::semantic::model_space(&config.model, config.dimensions),
                config.dimensions,
            )
        },
    );
    if generation != target.generation || dimensions != target.dimensions {
        return Err(AppError::configuration(
            "node embedding model/dimensions do not match the shared index",
        ));
    }
    Ok(())
}

async fn embed_batch(
    batch: &mut [MessageRecord],
    target: &Target,
    title: Option<&str>,
    inference: Option<&Inference<'_>>,
    pool: &mut Option<crate::semantic::EmbeddingPool>,
    build: &mut gold::Build,
) -> Result<(), AppError> {
    let inference = inference
        .ok_or_else(|| AppError::shared("shared gold needs preparation; run mash index"))?;
    validate_model(target, inference)?;
    let prepared = batch
        .iter()
        .filter(|m| !m.projection.is_empty())
        .map(|m| target.build.prepare(m.projection.clone(), title))
        .collect::<Vec<_>>();
    let texts = prepared.iter().map(String::as_str).collect::<Vec<_>>();
    let vectors = gold::embed_texts(
        inference.models_dir,
        inference.embeddings,
        pool,
        &texts,
        build,
    )
    .await?;
    if vectors.len() != prepared.len()
        || vectors
            .iter()
            .any(|v| !crate::shared::protocol::valid_vector(v, target.dimensions))
    {
        return Err(AppError::model("invalid prepared shared embeddings"));
    }
    build.embeddings.model_inferences += vectors.len() as u64;
    for (message, vector) in batch
        .iter_mut()
        .filter(|m| !m.projection.is_empty())
        .zip(vectors)
    {
        message.vector = Some(vector);
        message.generation = Some(target.generation.clone());
    }
    Ok(())
}

pub(super) async fn prepare(
    path: &Path,
    target: &Target,
    inference: Option<Inference<'_>>,
    started: Instant,
) -> Result<(tempfile::NamedTempFile, i64, SnapshotWork), AppError> {
    let path = path.to_path_buf();
    // Serialize only the lightweight target across the blocking capture boundary.
    let metadata = Target {
        schema_version: target.schema_version,
        build: target.build.clone(),
        generation: target.generation.clone(),
        dimensions: target.dimensions,
        publication_version: target.publication_version.clone(),
        instance: target.instance.clone(),
        publication_revision: target.publication_revision,
        threads: BTreeMap::new(),
    };
    let reuse_gold = !target.threads.is_empty();
    let source = crate::blocking(move || capture(&path, &metadata, reuse_gold)).await?;
    let reusable: BTreeMap<_, _> = source
        .hashes
        .into_iter()
        .filter(|(id, hash)| target.threads.get(id) == Some(hash))
        .collect();
    let input = source.file.reopen().map_err(AppError::io)?;
    let mut reader = crate::blocking(move || Reader::new(input)).await?;
    let mut build = gold::Build::default();
    let mut pool = None;
    let mut reused_embeddings = 0;
    let mut estimate = crate::progress::Estimate::new();
    let total_vectors = target.build.expected_embeddings(source.visible);
    loop {
        let (next, mut batch) = crate::blocking(move || reader.next()).await?;
        reader = next;
        if batch.is_empty() {
            break;
        }
        let reusable_thread = reusable.contains_key(&batch[0].conversation_id);
        let visible = batch.iter().filter(|m| !m.projection.is_empty()).count();
        if reusable_thread {
            reused_embeddings += visible as u64;
        } else if visible > 0 && target.build.dense_enabled() {
            let title = source
                .conversations
                .get(&batch[0].conversation_id)
                .and_then(|c| c.title.as_deref());
            embed_batch(
                &mut batch,
                target,
                title,
                inference.as_ref(),
                &mut pool,
                &mut build,
            )
            .await?;
        }
        reader = crate::blocking(move || {
            write_event(
                reader.output.as_file_mut(),
                &Event::Batch(Batch::Messages { records: batch }),
            )?;
            Ok(reader)
        })
        .await?;
        let processed = reused_embeddings + build.embeddings.model_inferences;
        let elapsed = crate::progress::milliseconds(started);
        crate::progress::emit(&crate::progress::Event {
            stored_vectors: Some(processed),
            total_vectors: Some(total_vectors),
            ..crate::progress::Event::new(
                crate::progress::Phase::SharedEmbeddings,
                elapsed,
                estimate.observe(processed, total_vectors, elapsed),
            )
        });
    }
    let reused_threads = reusable.len() as u64;
    let file = crate::blocking(move || {
        for (conversation_id, fingerprint) in reusable {
            write_event(
                reader.output.as_file_mut(),
                &Event::ReuseGold {
                    conversation_id,
                    fingerprint,
                },
            )?;
        }
        write_event(reader.output.as_file_mut(), &Event::Finish)?;
        reader.output.as_file_mut().flush().map_err(AppError::io)?;
        Ok(reader.output)
    })
    .await?;
    let preparation = SnapshotWork {
        reused_threads,
        reused_embeddings,
        forwarded_embeddings: 0,
        model_inferences: build.embeddings.model_inferences,
        model_load_milliseconds: build.model_load_milliseconds,
        embedding_milliseconds: build.embedding_milliseconds,
        preparation_milliseconds: crate::progress::milliseconds(started),
        upload_bytes: file.as_file().metadata().map_err(AppError::io)?.len(),
    };
    Ok((file, source.revision, preparation))
}
