//! Recipe-aware snapshot preparation from a consistent, read-only SQLite silver view.
use super::{Inference, SnapshotWork, write_event};
use crate::shared::{
    protocol::{Batch, BatchKind, ChatRecord, Event, MessageRecord, Start},
    reuse::Target,
};
use crate::{
    AppError,
    storage::{Storage, publication::Delta},
    views::shared as views,
};
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
            "shared node ownership or revision changed; retry indexing. If this machine's index was rebuilt, retire the node on the server (mash-server --retire-node) or choose a new node_id",
        ));
    }
    Ok(())
}

struct Source {
    file: tempfile::NamedTempFile,
    revision: i64,
    chats: BTreeMap<String, ChatRecord>,
    visible: u64,
}

#[derive(Clone, Copy)]
pub(super) enum Selection {
    Replace,
    Delta { journal_base: i64 },
    Backfill,
}

fn capture(path: &Path, target: &Target, selection: Selection) -> Result<Source, AppError> {
    crate::progress::during(crate::progress::Phase::SharedSnapshot, || {
        let mut file = tempfile::NamedTempFile::new().map_err(AppError::io)?;
        let mut writer = std::io::BufWriter::with_capacity(256 * 1024, file.as_file_mut());
        let mut chats = BTreeMap::new();
        let mut visible = 0;
        let revision = Storage::open_silver(path)?.with_publication_snapshot(|snapshot| {
            validate_owner(snapshot, target)?;
            let base = match selection {
                Selection::Delta { journal_base } => snapshot
                    .delta_base(target.instance.as_deref(), journal_base)?
                    .filter(|_| snapshot.revision > target.publication_revision),
                Selection::Replace | Selection::Backfill => None,
            };
            let incremental = base.is_some();
            // Amending keeps a chat's other documents, so only a recipe whose
            // dense input is the message alone may amend.
            let delta = base.map(|after| Delta {
                after,
                amend: target.build.message_local(),
            });
            write_event(
                &mut writer,
                &Event::Start(Start {
                    views_build: Some(target.build.clone()),
                    source_silver: Some(snapshot.source_silver.clone()),
                    instance: snapshot.instance.clone(),
                    revision: Some(snapshot.revision),
                    base_revision: if matches!(selection, Selection::Backfill) {
                        Some(snapshot.revision.saturating_sub(1))
                    } else {
                        base.map(|_| target.publication_revision)
                    },
                    snapshot: !incremental && !matches!(selection, Selection::Backfill),
                    forgets: Vec::new(),
                }),
                target.redaction,
            )?;
            if let Some(base) = base {
                for id in snapshot.removed_chats(base)? {
                    write_event(&mut writer, &Event::Remove { id }, target.redaction)?;
                }
            }
            snapshot.visit_selected(BatchKind::Tombstones, false, delta, |batch| {
                write_event(&mut writer, &Event::Batch(batch), target.redaction)
            })?;
            snapshot.visit_selected(BatchKind::Chats, false, delta, |mut batch| {
                batch.redact(target.redaction);
                if let Batch::Chats { records } = &batch {
                    for chat in records {
                        chats.insert(chat.id.clone(), chat.clone());
                    }
                }
                write_event(&mut writer, &Event::Batch(batch), target.redaction)
            })?;
            let amended = delta.filter(|delta| delta.amend).map(|delta| delta.after);
            if let Some(after) = amended {
                snapshot.visit_amendments(after, |mut amendment| {
                    amendment.chat.redact(target.redaction);
                    chats.insert(amendment.chat.id.clone(), amendment.chat.clone());
                    write_event(
                        &mut writer,
                        &Event::Amend(Box::new(amendment)),
                        target.redaction,
                    )
                })?;
            }
            let mut messages = |mut batch: Batch| {
                batch.redact(target.redaction);
                if let Batch::Messages { records } = &mut batch {
                    for message in records {
                        visible += u64::from(
                            target
                                .build
                                .includes_dense(&message.role, &message.projection),
                        );
                        // Only canonical rows enter this stream. Reuse is checked
                        // against the target's prepared, redacted input below.
                        message.vector = None;
                        message.generation = None;
                    }
                }
                write_event(&mut writer, &Event::Batch(batch), target.redaction)
            };
            snapshot.visit_selected(BatchKind::Messages, true, delta, &mut messages)?;
            if let Some(after) = amended {
                snapshot.visit_amended_messages(after, &mut messages)?;
            }
            snapshot.visit_records(base, target.redaction, |batch| {
                write_event(&mut writer, &Event::Batch(batch), target.redaction)
            })?;
            write_event(&mut writer, &Event::Finish, target.redaction)
        })?;
        writer.flush().map_err(AppError::io)?;
        drop(writer);
        Ok(Source {
            file,
            revision,
            chats,
            visible,
        })
    })
}

struct Reader {
    batch_rows: usize,
    units: crate::views::units::Stream,
    input: std::io::BufReader<std::fs::File>,
    pending: VecDeque<MessageRecord>,
    done: bool,
    output: tempfile::NamedTempFile,
    redaction: crate::redaction::Policy,
}

impl Reader {
    fn new(input: std::fs::File, redaction: crate::redaction::Policy) -> Result<Self, AppError> {
        Ok(Self {
            batch_rows: views::BATCH_ROWS,
            units: crate::views::units::Stream::default(),
            input: std::io::BufReader::with_capacity(64 * 1024, input),
            pending: VecDeque::new(),
            done: false,
            output: tempfile::NamedTempFile::new().map_err(AppError::io)?,
            redaction,
        })
    }

    fn next(mut self) -> Result<(Self, Vec<MessageRecord>), AppError> {
        let mut batch: Vec<MessageRecord> = Vec::new();
        loop {
            if batch.len() == self.batch_rows
                || self.pending.front().is_some_and(|message| {
                    batch
                        .first()
                        .is_some_and(|first| first.chat_id != message.chat_id)
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
                event => write_event(self.output.as_file_mut(), &event, self.redaction)?,
            }
        }
        Ok((self, batch))
    }
}

async fn prepare_units(
    mut reader: Reader,
    batch: Vec<MessageRecord>,
    recipe: &crate::views::config::Build,
    title: Option<String>,
) -> Result<(Reader, Vec<MessageRecord>, Vec<String>), AppError> {
    let recipe = recipe.clone();
    crate::blocking(move || {
        let mut prepared = Vec::new();
        for message in &batch {
            if let Some(text) = reader.units.prepare(
                &recipe,
                crate::views::units::Input {
                    chat: &message.chat_id,
                    id: &message.id,
                    ordinal: message.ordinal,
                    context: &message.context,
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

fn validate_model(target: &Target) -> Result<(), AppError> {
    let (generation, dimensions) = (crate::semantic::SHARED_EMBEDDING_GENERATION, 384);
    if generation != target.generation || dimensions != target.dimensions {
        return Err(AppError::configuration(
            "node embedding model/dimensions do not match the shared index",
        ));
    }
    Ok(())
}

struct Prepared<'a> {
    chat: &'a ChatRecord,
    texts: &'a [String],
    cache: Option<&'a crate::views::local::Local>,
}

async fn embed_batch(
    batch: &mut [MessageRecord],
    target: &Target,
    prepared: Prepared<'_>,
    inference: Option<&Inference<'_>>,
    pool: &mut Option<crate::semantic::LoadedIndexer>,
    build: &mut views::Build,
) -> Result<(), AppError> {
    let inference = inference
        .ok_or_else(|| AppError::shared("shared views needs preparation; run mash index"))?;
    validate_model(target)?;
    let eligible = batch
        .iter()
        .filter(|m| target.build.includes_dense(&m.role, &m.projection))
        .collect::<Vec<_>>();
    if eligible.len() != prepared.texts.len() {
        return Err(AppError::internal(
            "prepared publication input cardinality differs from messages",
        ));
    }
    let inputs = eligible
        .iter()
        .zip(prepared.texts)
        .map(|(message, text)| crate::views::documents::EmbeddingInput {
            provider: prepared.chat.provider,
            anchor: &message.id,
            text,
        })
        .collect::<Vec<_>>();
    let cached = if let Some(local) = prepared.cache {
        let mut scope = local.scope.clone();
        scope.generation.clone_from(&target.generation);
        scope.dimensions = target.dimensions;
        match local.store.reusable_vectors(&scope, &inputs).await {
            Ok(cached) => cached,
            Err(error) => {
                tracing::debug!(kind = ?error.error.kind, "local embedding cache unavailable during publication");
                vec![None; inputs.len()]
            }
        }
    } else {
        vec![None; inputs.len()]
    };
    let vectors = views::embed_units(
        &inputs,
        (&target.generation, target.dimensions),
        Some(inference.models_dir),
        pool,
        build,
        cached,
        inference
            .local
            .map_or(&crate::config::Embeddings::Local {}, |config| {
                &config.embeddings
            }),
    )
    .await?;
    for (message, (vector, _)) in batch
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
    selection: Selection,
) -> Result<(tempfile::NamedTempFile, i64, SnapshotWork), AppError> {
    let database = path.to_path_buf();
    let path = database.clone();
    let redaction = target.redaction;
    let metadata = target.clone();
    let source = crate::blocking(move || capture(&path, &metadata, selection)).await?;
    let cache = if let Some(configuration) =
        inference.as_ref().and_then(|inference| inference.local)
    {
        match crate::views::local::Local::open(&database, configuration).await {
            Ok(local) => Some(local),
            Err(error) => {
                tracing::debug!(kind = ?error.error.kind, "local embedding cache unavailable during publication");
                None
            }
        }
    } else {
        None
    };
    let input = source.file.reopen().map_err(AppError::io)?;
    let mut reader = crate::blocking(move || Reader::new(input, redaction)).await?;
    if let Some(configuration) = inference.as_ref().and_then(|inference| inference.local) {
        reader.batch_rows = views::batch_rows(&configuration.embeddings);
    }
    let mut build = views::Build::default();
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
            let chat = source
                .chats
                .get(&batch[0].chat_id)
                .ok_or_else(|| AppError::internal("publication message has no chat"))?;
            let title = chat.title.clone();
            let (next, next_batch, prepared) =
                prepare_units(reader, batch, &target.build, title).await?;
            reader = next;
            batch = next_batch;
            embed_batch(
                &mut batch,
                target,
                Prepared {
                    chat,
                    texts: &prepared,
                    cache: cache.as_ref(),
                },
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
                redaction,
            )?;
            Ok(reader)
        })
        .await?;
        let processed = build.embeddings.model_inferences + build.embeddings.reused_vectors;
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
        write_event(reader.output.as_file_mut(), &Event::Finish, redaction)?;
        reader.output.as_file_mut().flush().map_err(AppError::io)?;
        Ok(reader.output)
    })
    .await?;
    let preparation = SnapshotWork {
        forwarded_embeddings: build.embeddings.reused_vectors,
        model_inferences: build.embeddings.model_inferences,
        model_load_milliseconds: build.model_load_milliseconds,
        embedding_milliseconds: build.embedding_milliseconds,
        preparation_milliseconds: crate::progress::milliseconds(started),
        upload_bytes: file.as_file().metadata().map_err(AppError::io)?.len(),
    };
    Ok((file, source.revision, preparation))
}
