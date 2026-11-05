//! Repeatable normalized-message reads with bounded inline payload retention.
use std::borrow::Cow;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::Arc;

use super::NormalizedMessage;
use crate::AppError;

const INLINE_BYTES: usize = 16 * 1024 * 1024;
mod reader;
mod summary;
use reader::{Records, read_at, records};

pub(crate) enum Messages {
    Inline(Vec<NormalizedMessage>),
    Staged(Spool),
}

pub(crate) struct Spool {
    path: tempfile::TempPath,
    offsets: Arc<[u64]>,
    length: u64,
    summary: summary::Summary,
}

pub(crate) struct Builder {
    state: State,
    limit: usize,
}

enum State {
    Inline {
        messages: Vec<NormalizedMessage>,
        bytes: usize,
    },
    Staged(Writer),
    Failed,
}

struct Writer {
    output: BufWriter<File>,
    path: tempfile::TempPath,
    offsets: Vec<u64>,
    position: u64,
    summary: summary::Summary,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            state: State::Inline {
                messages: Vec::new(),
                bytes: 0,
            },
            limit: INLINE_BYTES,
        }
    }
}

impl Builder {
    pub(super) fn len(&self) -> usize {
        match &self.state {
            State::Inline { messages, .. } => messages.len(),
            State::Staged(writer) => writer.offsets.len(),
            State::Failed => 0,
        }
    }

    pub(crate) fn push(&mut self, message: NormalizedMessage) -> Result<usize, AppError> {
        let result = self.push_inner(message);
        if result.is_err() {
            self.state = State::Failed;
        }
        result
    }

    fn push_inner(&mut self, message: NormalizedMessage) -> Result<usize, AppError> {
        let retained = std::mem::size_of::<NormalizedMessage>()
            .saturating_add(message.id.capacity())
            .saturating_add(message.content.capacity())
            .saturating_add(
                message
                    .attachments
                    .iter()
                    .map(crate::attachments::Source::retained_size)
                    .sum::<usize>(),
            )
            .saturating_add(match &message.role {
                crate::role::Role::Other(role) => role.capacity(),
                _ => 0,
            })
            .saturating_add(message.dialogue_text.as_ref().map_or(0, String::capacity))
            .saturating_add(message.tools.retained_size());
        if matches!(&self.state, State::Inline { bytes, .. } if bytes.saturating_add(retained) > self.limit)
        {
            let temporary = tempfile::Builder::new()
                .prefix("mash-messages-")
                .tempfile()
                .map_err(AppError::io)?;
            let (file, path) = temporary.into_parts();
            let mut writer = Writer {
                output: BufWriter::new(file),
                path,
                offsets: Vec::new(),
                position: 0,
                summary: summary::Summary::default(),
            };
            if let State::Inline { messages, .. } = &mut self.state {
                for message in messages.drain(..) {
                    writer.push(&message)?;
                }
            }
            self.state = State::Staged(writer);
            tracing::info!(
                messages = self.len(),
                inline_limit_bytes = self.limit,
                "spilled normalized messages to private scratch file"
            );
        }
        let position = self.len();
        match &mut self.state {
            State::Inline { messages, bytes } => {
                *bytes = bytes.saturating_add(retained);
                messages.push(message);
            }
            State::Staged(writer) => writer.push(&message)?,
            State::Failed => {
                return Err(AppError::internal(
                    "normalized-message staging previously failed",
                ));
            }
        }
        Ok(position)
    }

    pub(super) fn get(
        &mut self,
        index: usize,
    ) -> Result<Option<Cow<'_, NormalizedMessage>>, AppError> {
        match &mut self.state {
            State::Inline { messages, .. } => Ok(messages.get(index).map(Cow::Borrowed)),
            State::Staged(writer) => {
                writer.output.flush().map_err(AppError::io)?;
                tracing::info!(
                    messages = writer.offsets.len(),
                    staged_bytes = writer.position,
                    "finished normalized message staging"
                );
                read_at(&writer.path, &writer.offsets, index).map(|message| message.map(Cow::Owned))
            }
            State::Failed => Err(AppError::internal(
                "normalized-message staging previously failed",
            )),
        }
    }

    pub(crate) fn finish(self) -> Result<Messages, AppError> {
        match self.state {
            State::Inline { messages, .. } => Ok(Messages::Inline(messages)),
            State::Staged(mut writer) => {
                // A failed flush must abort parsing/reconciliation rather than
                // returning a successfully staged but incomplete chat.
                writer.output.flush().map_err(AppError::io)?;
                Ok(Messages::Staged(Spool {
                    path: writer.path,
                    offsets: writer.offsets.into(),
                    length: writer.position,
                    summary: writer.summary,
                }))
            }
            State::Failed => Err(AppError::internal(
                "normalized-message staging previously failed",
            )),
        }
    }
}

impl Writer {
    fn push(&mut self, message: &NormalizedMessage) -> Result<(), AppError> {
        self.offsets.push(self.position);
        let mut counter = CountingWriter {
            writer: &mut self.output,
            position: &mut self.position,
        };
        reader::write_record(&mut counter, message)?;
        self.summary.observe(message);
        Ok(())
    }
}

struct CountingWriter<'a> {
    writer: &'a mut BufWriter<File>,
    position: &'a mut u64,
}

impl Write for CountingWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let written = self.writer.write(bytes)?;
        *self.position = self
            .position
            .checked_add(written as u64)
            .ok_or_else(|| std::io::Error::other("normalized-message scratch file overflow"))?;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

fn scratch_error(error: serde_json::Error) -> AppError {
    AppError::io(std::io::Error::other(error))
}

pub(crate) enum Rows<'a> {
    Inline(std::slice::Iter<'a, NormalizedMessage>),
    Staged {
        records: Records,
        remaining: Option<usize>,
        _spool: &'a Spool,
    },
}

impl<'a> Iterator for Rows<'a> {
    type Item = Result<Cow<'a, NormalizedMessage>, AppError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Inline(rows) => rows.next().map(|row| Ok(Cow::Borrowed(row))),
            Self::Staged {
                records, remaining, ..
            } => next_record(records, remaining).map(|row| row.map(Cow::Owned)),
        }
    }
}

fn next_record(
    records: &mut Records,
    remaining: &mut Option<usize>,
) -> Option<Result<NormalizedMessage, AppError>> {
    let expected = (*remaining)?;
    match records.next() {
        Some(Ok(message)) if expected > 0 => {
            *remaining = Some(expected - 1);
            Some(Ok(message))
        }
        Some(result) => {
            *remaining = None;
            Some(result.and_then(|_| {
                Err(AppError::io(std::io::Error::other(
                    "extra normalized-message scratch record",
                )))
            }))
        }
        None if expected > 0 => {
            *remaining = None;
            Some(Err(AppError::io(std::io::Error::other(
                "truncated normalized-message scratch file",
            ))))
        }
        None => {
            *remaining = None;
            None
        }
    }
}

pub(crate) enum OwnedRows {
    Inline(std::vec::IntoIter<NormalizedMessage>),
    Staged {
        records: Records,
        remaining: Option<usize>,
        _spool: Box<Spool>,
    },
}

impl Iterator for OwnedRows {
    type Item = Result<NormalizedMessage, AppError>;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Inline(rows) => rows.next().map(Ok),
            Self::Staged {
                records, remaining, ..
            } => next_record(records, remaining),
        }
    }
}

impl Messages {
    pub(crate) fn stage(
        messages: impl IntoIterator<Item = NormalizedMessage>,
    ) -> Result<Self, AppError> {
        let mut builder = Builder::default();
        for message in messages {
            builder.push(message)?;
        }
        builder.finish()
    }
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Inline(messages) => messages.len(),
            Self::Staged(spool) => spool.offsets.len(),
        }
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub(crate) fn summary(&self) -> Result<summary::Summary, AppError> {
        if let Self::Staged(spool) = self {
            if std::fs::metadata(&spool.path).map_err(AppError::io)?.len() != spool.length {
                return Err(AppError::io(std::io::Error::other(
                    "normalized-message scratch file length changed",
                )));
            }
            return Ok(spool.summary.clone());
        }
        let mut summary = summary::Summary::default();
        for message in self.rows()? {
            let message = message?;
            summary.observe(&message);
        }
        Ok(summary)
    }
    pub(crate) fn rows(&self) -> Result<Rows<'_>, AppError> {
        match self {
            Self::Inline(messages) => Ok(Rows::Inline(messages.iter())),
            Self::Staged(spool) => Ok(Rows::Staged {
                records: records(&spool.path, spool.offsets.clone(), spool.length)?,
                remaining: Some(spool.offsets.len()),
                _spool: spool,
            }),
        }
    }
    pub(crate) fn into_rows(self) -> Result<OwnedRows, AppError> {
        match self {
            Self::Inline(messages) => Ok(OwnedRows::Inline(messages.into_iter())),
            Self::Staged(spool) => Ok(OwnedRows::Staged {
                records: records(&spool.path, spool.offsets.clone(), spool.length)?,
                remaining: Some(spool.offsets.len()),
                _spool: Box::new(spool),
            }),
        }
    }
    #[cfg(test)]
    pub(crate) fn stage_fixture(messages: Vec<NormalizedMessage>) -> Result<Self, AppError> {
        let mut builder = Builder {
            limit: 0,
            ..Builder::default()
        };
        for message in messages {
            builder.push(message)?;
        }
        builder.finish()
    }
    #[cfg(test)]
    pub(crate) fn staging_path(&self) -> Option<&std::path::Path> {
        match self {
            Self::Inline(_) => None,
            Self::Staged(spool) => Some(&spool.path),
        }
    }
    #[cfg(test)]
    pub(crate) fn inline(&self) -> &[NormalizedMessage] {
        match self {
            Self::Inline(messages) => messages,
            Self::Staged(_) => {
                panic!("fixture expects a small inline chat; use fallible rows for staged messages")
            }
        }
    }
    #[cfg(test)]
    pub(crate) fn inline_mut(&mut self) -> &mut Vec<NormalizedMessage> {
        match self {
            Self::Inline(messages) => messages,
            Self::Staged(_) => panic!("fixture expects a small inline chat"),
        }
    }
}

#[cfg(test)]
impl From<Vec<NormalizedMessage>> for Messages {
    fn from(messages: Vec<NormalizedMessage>) -> Self {
        Self::Inline(messages)
    }
}
#[cfg(test)]
impl FromIterator<NormalizedMessage> for Messages {
    fn from_iter<T: IntoIterator<Item = NormalizedMessage>>(messages: T) -> Self {
        Self::Inline(messages.into_iter().collect())
    }
}

#[cfg(test)]
mod tests;
