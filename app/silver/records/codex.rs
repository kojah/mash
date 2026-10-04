//! Stream oversized, non-dialogue Codex lifecycle mirrors without retaining them.
use crate::AppError;
use serde::Deserialize;
use std::io::{BufRead, BufReader, Read, Seek};

const MAX_EVENT_BYTES: usize = 64 * 1024 * 1024;

pub(in crate::silver) struct Lines<R> {
    reader: R,
    finished: bool,
    physical_line: usize,
}

impl<R: BufRead + Seek> Lines<R> {
    pub(in crate::silver) fn new(reader: R) -> Self {
        Self {
            reader,
            finished: false,
            physical_line: 0,
        }
    }

    pub(in crate::silver) fn numbered(
        mut self,
    ) -> impl Iterator<Item = Result<(usize, String), AppError>> {
        std::iter::from_fn(move || {
            self.next()
                .map(|result| result.map(|text| (self.physical_line, text)))
        })
    }
}

impl<R: BufRead + Seek> Iterator for Lines<R> {
    type Item = Result<String, AppError>;

    fn next(&mut self) -> Option<Self::Item> {
        while !self.finished {
            // Track consumed bytes locally rather than issuing a seek syscall
            // for every ordinary history record. Only oversized input rewinds.
            let mut counted = Counted {
                reader: &mut self.reader,
                bytes: 0,
            };
            let result = super::read_line(&mut counted, super::super::MAX_RECORD_BYTES);
            let consumed = counted.bytes;
            if consumed != 0 {
                self.physical_line += 1;
            }
            match result {
                Ok(Some(line)) => return Some(Ok(line)),
                Ok(None) => self.finished = true,
                Err(error) => {
                    let result = i64::try_from(consumed)
                        .map_err(|_| AppError::internal("invalid Codex record read length"))
                        .and_then(|bytes| self.reader.seek_relative(-bytes).map_err(AppError::io))
                        .and_then(|()| skip_event(&mut self.reader));
                    match result {
                        Ok(true) => {}
                        Ok(false) => {
                            self.finished = true;
                            return Some(Err(error));
                        }
                        Err(error) => {
                            self.finished = true;
                            return Some(Err(error));
                        }
                    }
                }
            }
        }
        None
    }
}

struct Counted<'a, R> {
    reader: &'a mut R,
    bytes: usize,
}
impl<R: Read> Read for Counted<'_, R> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        let length = self.reader.read(output)?;
        self.bytes += length;
        Ok(length)
    }
}
impl<R: BufRead> BufRead for Counted<'_, R> {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        self.reader.fill_buf()
    }
    fn consume(&mut self, length: usize) {
        self.bytes += length;
        self.reader.consume(length);
    }
}

#[derive(Deserialize, PartialEq)]
enum EnvelopeKind {
    #[serde(rename = "event_msg")]
    Event,
    #[serde(rename = "compacted")]
    Compacted,
    #[serde(other)]
    Other,
}
#[derive(Default, Deserialize, PartialEq)]
enum EventKind {
    #[serde(rename = "item_completed")]
    Completed,
    #[serde(other)]
    #[default]
    Other,
}
#[derive(Deserialize, PartialEq)]
enum ItemKind {
    FileChange,
    CommandExecution,
    #[serde(other)]
    Other,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    #[serde(default, rename = "ordinal")]
    _ordinal: Option<u64>,
    #[serde(rename = "type")]
    kind: EnvelopeKind,
    #[serde(rename = "timestamp")]
    _timestamp: serde::de::IgnoredAny,
    payload: Event,
}
#[derive(Deserialize)]
struct Event {
    #[serde(default, rename = "type")]
    kind: EventKind,
    item: Option<Item>,
    // Compaction replays are not canonical dialogue in the ordinary parser.
    // IgnoredAny validates every replay value without retaining its text.
    replacement_history: Option<Vec<serde::de::IgnoredAny>>,
    message: Option<serde::de::IgnoredAny>,
}
#[derive(Deserialize)]
struct Item {
    #[serde(rename = "type")]
    kind: ItemKind,
}

fn skip_event(reader: &mut impl BufRead) -> Result<bool, AppError> {
    // IgnoredAny validates JSON without building the ignored diff/output trees.
    // A separate reader validates UTF-8, including ignored strings, and bounds
    // disk work. The line boundary prevents swallowing the following message.
    let line = Line {
        reader,
        bytes: 0,
        ended: false,
        pending: Vec::new(),
    };
    let event: Envelope = serde_json::from_reader(BufReader::new(line)).map_err(|_| {
        AppError::configuration("invalid or oversized Codex lifecycle envelope; canonical records retain the 8 MiB ingestion limit")
    })?;
    Ok(match event.kind {
        EnvelopeKind::Event => {
            event.payload.kind == EventKind::Completed
                && event.payload.item.is_some_and(|item| {
                    matches!(item.kind, ItemKind::FileChange | ItemKind::CommandExecution)
                })
        }
        EnvelopeKind::Compacted => {
            event.payload.replacement_history.is_some() && event.payload.message.is_some()
        }
        EnvelopeKind::Other => false,
    })
}

struct Line<'a, R> {
    reader: &'a mut R,
    bytes: usize,
    ended: bool,
    pending: Vec<u8>,
}

impl<R: BufRead> Read for Line<'_, R> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        if self.ended {
            return if self.pending.is_empty() {
                Ok(0)
            } else {
                Err(std::io::Error::other(
                    "invalid UTF-8 in Codex lifecycle envelope",
                ))
            };
        }
        let input = self.reader.fill_buf()?;
        let length = input
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(input.len(), |index| index + 1)
            .min(output.len());
        if self.bytes.saturating_add(length) > MAX_EVENT_BYTES {
            return Err(std::io::Error::other(
                "Codex lifecycle envelope exceeds 64 MiB",
            ));
        }
        self.ended = length == 0 || input.get(length.wrapping_sub(1)) == Some(&b'\n');
        self.pending.extend_from_slice(&input[..length]);
        match std::str::from_utf8(&self.pending) {
            Ok(_) => self.pending.clear(),
            Err(error) if error.error_len().is_none() => {
                self.pending.drain(..error.valid_up_to());
            }
            Err(_) => {
                return Err(std::io::Error::other(
                    "invalid UTF-8 in Codex lifecycle envelope",
                ));
            }
        }
        output[..length].copy_from_slice(&input[..length]);
        self.reader.consume(length);
        self.bytes += length;
        Ok(length)
    }
}

#[cfg(test)]
mod tests;
