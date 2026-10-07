//! Recipe-aware snapshot preparation from a consistent, read-only SQLite silver view.
use super::{Inference, SnapshotWork, write_event};
use crate::shared::{
    protocol::{Batch, BatchKind, ConversationRecord, Event, MessageRecord, Start},
    reuse::Target,
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
    visible: u64,
}

fn capture(path: &Path, target: &Target, allow_delta: bool) -> Result<Source, AppError> {
    crate::progress::during(crate::progress::Phase::SharedSnapshot, || {
        let mut file = tempfile::NamedTempFile::new().map_err(AppError::io)?;
        let mut writer = std::io::BufWriter::with_capacity(256 * 1024, file.as_file_mut());
        let mut conversations = BTreeMap::new();
        let mut visible = 0;
        let revision = Storage::open_catalog(path)?.with_publication_snapshot(|snapshot| {
            validate_owner(snapshot, target)?;
            let base = snapshot
                .delta_base(target.instance.as_deref(), target.publication_revision)?
                .filter(|_| allow_delta);
            let incremental = base.is_some();
            write_event(
                &mut writer,
                &Event::Start(Start {
                    gold_build: Some(target.build.clone()),
                    source_silver: Some(snapshot.source_silver.clone()),
                    instance: snapshot.instance.clone(),
                    revision: Some(snapshot.revision),
                    base_revision: base,
                    snapshot: !incremental,
                    forgets: Vec::new(),
                }),
            )?;
            if let Some(base) = base {
                for id in snapshot.removed_threads(base)? {
                    write_event(&mut writer, &Event::Remove { id })?;
                }
            }
            snapshot.visit_selected(BatchKind::Tombstones, false, base, |batch| {
                write_event(&mut writer, &Event::Batch(batch))
            })?;
            snapshot.visit_selected(BatchKind::Conversations, false, base, |mut batch| {
                crate::redaction::batch(&mut batch);
                if let Batch::Conversations { records } = &batch {
                    for conversation in records {
                        conversations.insert(conversation.id.clone(), conversation.clone());
                    }
                }
                write_event(&mut writer, &Event::Batch(batch))
            })?;
            snapshot.visit_selected(BatchKind::Messages, true, base, |mut batch| {
                crate::redaction::batch(&mut batch);
                if let Batch::Messages { records } = &mut batch {
                    for message in records {
                        visible += u64::from(
                            target
                                .build
                                .includes_dense(&message.role, &message.projection),
                        );
                        // Local gold has a different recipe. Only canonical rows enter
                        // this temporary stream; shared embedding follows below.
                        message.vector = None;
                        message.generation = None;
                    }
                }
                write_event(&mut writer, &Event::Batch(batch))
            })?;
            write_event(&mut writer, &Event::Finish)
        })?;
        writer.flush().map_err(AppError::io)?;
        drop(writer);
        Ok(Source {
            file,
            revision,
            conversations,
            visible,
        })
    })
}

struct Reader {
    units: crate::gold::units::Stream,
    input: std::io::BufReader<std::fs::File>,
    pending: VecDeque<MessageRecord>,
    done: bool,
    output: tempfile::NamedTempFile,
}

impl Reader {
    fn new(input: std::fs::File) -> Result<Self, AppError> {
        Ok(Self {
            units: crate::gold::units::Stream::default(),
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

async fn prepare_units(
    mut reader: Reader,
    batch: Vec<MessageRecord>,
    recipe: &crate::gold::config::Build,
    title: Option<String>,
) -> Result<(Reader, Vec<MessageRecord>, Vec<String>), AppError> {
    let recipe = recipe.clone();
    crate::blocking(move || {
        let mut prepared = Vec::new();
        for message in &batch {
            if let Some(text) = reader.units.prepare(
                &recipe,
                crate::gold::units::Input {
                    thread: &message.conversation_id,
                    ordinal: message.ordinal,
                    context: message.context,
                    role: &message.role,
                    projection: &message.projection,
                },
                title.as_deref(),
            )? {
                prepared.push(text);
            }
        }
        Ok((reader, batch, prepared))
    })
    .await
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
    prepared: &[String],
    inference: Option<&Inference<'_>>,
    pool: &mut Option<crate::semantic::LoadedIndexer>,
    build: &mut gold::Build,
) -> Result<(), AppError> {
    let inference = inference
        .ok_or_else(|| AppError::shared("shared gold needs preparation; run mash index"))?;
    validate_model(target, inference)?;
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
        .filter(|m| target.build.includes_dense(&m.role, &m.projection))
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
    allow_delta: bool,
) -> Result<(tempfile::NamedTempFile, i64, SnapshotWork), AppError> {
    let path = path.to_path_buf();
    let metadata = target.clone();
    let source = crate::blocking(move || capture(&path, &metadata, allow_delta)).await?;
    let input = source.file.reopen().map_err(AppError::io)?;
    let mut reader = crate::blocking(move || Reader::new(input)).await?;
    let mut build = gold::Build::default();
    let mut pool = None;
    let mut estimate = crate::progress::Estimate::new();
    let total_vectors = target.build.expected_embeddings(source.visible);
    loop {
        let (next, mut batch) = crate::blocking(move || reader.next()).await?;
        reader = next;
        if batch.is_empty() {
            break;
        }
        let visible = batch
            .iter()
            .any(|m| target.build.includes_dense(&m.role, &m.projection));
        if visible && target.build.dense_enabled() {
            let title = source
                .conversations
                .get(&batch[0].conversation_id)
                .and_then(|c| c.title.clone());
            let (next, next_batch, prepared) =
                prepare_units(reader, batch, &target.build, title).await?;
            reader = next;
            batch = next_batch;
            embed_batch(
                &mut batch,
                target,
                &prepared,
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
        let processed = build.embeddings.model_inferences;
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
    let file = crate::blocking(move || {
        write_event(reader.output.as_file_mut(), &Event::Finish)?;
        reader.output.as_file_mut().flush().map_err(AppError::io)?;
        Ok(reader.output)
    })
    .await?;
    let preparation = SnapshotWork {
        forwarded_embeddings: 0,
        model_inferences: build.embeddings.model_inferences,
        model_load_milliseconds: build.model_load_milliseconds,
        embedding_milliseconds: build.embedding_milliseconds,
        preparation_milliseconds: crate::progress::milliseconds(started),
        upload_bytes: file.as_file().metadata().map_err(AppError::io)?.len(),
    };
    Ok((file, source.revision, preparation))
}
