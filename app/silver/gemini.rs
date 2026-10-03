//! Replay native Gemini recording updates and inclusive rewind records.
use super::{Conversation, NormalizedMessage, ParsedFile, SourceIdentity, records};
use crate::{AppError, message_context::MessageContext, provider::Provider};
use models::{Header, Message, Record, SessionKind, Update};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufReader, Read},
    path::{Path, PathBuf},
};

mod checkpoint;
mod models;
#[cfg(test)]
mod tests;

const MAX_REPLAY_BYTES: usize = 32 * 1024 * 1024;
const MAX_MESSAGES: usize = 65_536;

#[derive(Clone, Copy)]
enum Origin {
    Recorded,
    ModelContext,
}

struct Position {
    index: usize,
    origin: Origin,
}

#[derive(Default)]
struct Replay {
    session: String,
    project: String,
    created_at: Option<i64>,
    updated_at: Option<i64>,
    summary: Option<String>,
    kind: SessionKind,
    messages: Vec<NormalizedMessage>,
    archived: Vec<NormalizedMessage>,
    archived_ids: HashSet<String>,
    compression_root: Option<String>,
    positions: HashMap<String, Position>,
    retained_bytes: usize,
}

fn timestamp(value: &str) -> Result<i64, AppError> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|t| t.timestamp_millis())
        .map_err(|_| AppError::configuration("invalid Gemini session timestamp"))
}

impl Replay {
    fn start(mut header: Header) -> Result<Self, AppError> {
        let messages = header.update.messages.take();
        let mut replay = Self {
            session: header.session_id,
            project: header.project_hash,
            ..Self::default()
        };
        replay.update(header.update)?;
        if let Some(messages) = messages {
            for message in messages {
                replay.message(message, Origin::Recorded)?;
            }
        }
        replay.validate()?;
        Ok(replay)
    }

    fn validate(&self) -> Result<(), AppError> {
        if self.session.trim().is_empty() || self.project.trim().is_empty() {
            return Err(AppError::configuration(
                "empty Gemini session or project identity",
            ));
        }
        Ok(())
    }

    fn message(&mut self, message: Message, origin: Origin) -> Result<(), AppError> {
        if message.id.trim().is_empty() {
            return Err(AppError::configuration("empty Gemini message identity"));
        }
        if self.archived_ids.contains(&message.id) {
            return Err(AppError::configuration(
                "Gemini message reuses a compacted history identity",
            ));
        }
        let created_at = timestamp(&message.timestamp)?;
        // Retain prose instead of tool/SDK containers. Expensive search filtering
        // runs only on final saved versions, after all updates and rewinds.
        let (role, text) = message.normalized();
        let message = NormalizedMessage {
            id: message.id,
            ordinal: 0,
            context: MessageContext::Linear,
            role,
            content: text.canonical,
            dialogue_text: Some(text.projection),
            created_at: Some(created_at),
        };
        let previous_bytes = self
            .positions
            .get(&message.id)
            .map_or(0, |p| retained_bytes(&self.messages[p.index]));
        let retained = self
            .retained_bytes
            .saturating_sub(previous_bytes)
            .saturating_add(retained_bytes(&message));
        if retained > MAX_REPLAY_BYTES
            || (self.messages.len() + self.archived.len() == MAX_MESSAGES
                && !self.positions.contains_key(&message.id))
        {
            return Err(AppError::configuration(
                "Gemini history exceeds the retained replay limit",
            ));
        }
        self.retained_bytes = retained;
        if let Some(position) = self.positions.get_mut(&message.id) {
            position.origin = origin;
            self.messages[position.index] = message;
        } else {
            self.positions.insert(
                message.id.clone(),
                Position {
                    index: self.messages.len(),
                    origin,
                },
            );
            self.messages.push(message);
        }
        Ok(())
    }

    fn update(&mut self, update: Update) -> Result<(), AppError> {
        if let Some(value) = update.session_id {
            self.session = value;
        }
        if let Some(value) = update.project_hash {
            self.project = value;
        }
        if let Some(value) = update.start_time {
            self.created_at = Some(timestamp(&value)?);
        }
        if let Some(value) = update.last_updated {
            self.updated_at = Some(timestamp(&value)?);
        }
        if let Some(value) = update.summary {
            self.summary = Some(value);
        }
        if let Some(value) = update.kind {
            self.kind = value;
        }
        if let Some(messages) = update.messages {
            self.checkpoint(messages)?;
        }
        Ok(())
    }

    fn record(&mut self, record: Record) -> Result<(), AppError> {
        match record {
            Record::Message(message) => self.message(message, Origin::Recorded)?,
            Record::Set { update } => self.update(update)?,
            Record::Rewind { target } => {
                // The native writer emits rewinds only for existing messages.
                // Its tolerant loader clears everything on a broken reference;
                // ingestion must preserve the previous snapshot and report it.
                let position = self
                    .positions
                    .get(&target)
                    .map(|p| p.index)
                    .ok_or_else(|| {
                        AppError::configuration("Gemini rewind references a missing message")
                    })?;
                if self.compression_root.as_ref().is_some_and(|root| {
                    self.positions
                        .get(root)
                        .is_some_and(|p| p.index >= position)
                }) {
                    self.retained_bytes = self
                        .retained_bytes
                        .saturating_sub(self.archived.iter().map(retained_bytes).sum::<usize>());
                    self.archived.clear();
                    self.archived_ids.clear();
                    self.compression_root = None;
                }
                for removed in self.messages.drain(position..) {
                    self.positions.remove(&removed.id);
                    self.retained_bytes =
                        self.retained_bytes.saturating_sub(retained_bytes(&removed));
                }
            }
            Record::Header(header) => {
                self.session = header.session_id;
                self.project = header.project_hash;
                self.update(header.update)?;
            }
        }
        self.validate()
    }
}

fn retained_bytes(message: &NormalizedMessage) -> usize {
    // Include both owned ID copies, normalized strings and conservative slack
    // for geometric Vec/HashMap capacity and replay bookkeeping. This is an
    // accounting limit rather than an allocator/RSS measurement.
    message
        .id
        .capacity()
        .saturating_mul(2)
        .saturating_add(message.content.capacity())
        .saturating_add(message.dialogue_text.as_ref().map_or(0, String::capacity))
        .saturating_add(2 * std::mem::size_of::<NormalizedMessage>() + 128)
}

fn json(path: &Path) -> Result<Header, AppError> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(AppError::io)?
        .take((super::MAX_RECORD_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(AppError::io)?;
    if bytes.len() > super::MAX_RECORD_BYTES {
        return Err(AppError::configuration(
            "Gemini JSON session exceeds the 8 MiB document limit",
        ));
    }
    records::decode(&bytes)
}

fn replay(path: &Path) -> Result<Replay, AppError> {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"))
    {
        return Replay::start(json(path)?);
    }
    let mut lines = records::Lines::new(BufReader::new(File::open(path).map_err(AppError::io)?))
        .filter(|line| match line {
            Ok(line) => !line.trim().is_empty(),
            Err(_) => true,
        });
    let header: Header = records::decode(
        lines
            .next()
            .ok_or_else(|| AppError::configuration("Gemini session header is missing"))??
            .as_bytes(),
    )?;
    let mut state = Replay::start(header)?;
    for line in lines {
        let record = records::decode(line?.as_bytes())?;
        state.record(record)?;
    }
    Ok(state)
}

/// The upstream registry writes this ownership marker beside the chats folder.
pub(super) fn project_marker(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|p| p.file_name().is_some_and(|n| n == "chats"))?
        .parent()
        .map(|p| p.join(".project_root"))
}

fn project(path: &Path, hash: &str) -> Result<String, AppError> {
    use sha2::{Digest, Sha256};
    let Some(marker) = project_marker(path) else {
        return Ok(hash.into());
    };
    let mut file = match File::open(marker) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(hash.into()),
        Err(error) => return Err(AppError::io(error)),
    };
    let mut bytes = Vec::new();
    file.by_ref()
        .take(8193)
        .read_to_end(&mut bytes)
        .map_err(AppError::io)?;
    if bytes.len() > 8192 {
        return Err(AppError::configuration(
            "Gemini project marker exceeds 8 KiB",
        ));
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| AppError::configuration("invalid Gemini project marker"))?
        .trim();
    if hex::encode(Sha256::digest(text.as_bytes())) == hash {
        Ok(text.into())
    } else {
        Ok(hash.into())
    }
}

pub(super) fn parse(path: &Path) -> Result<ParsedFile, AppError> {
    let mut state = replay(path)?;
    state.archived.append(&mut state.messages);
    state.messages = state.archived;
    state
        .messages
        .retain(|message| !message.content.trim().is_empty());
    for (ordinal, message) in state.messages.iter_mut().enumerate() {
        if !matches!(state.kind, SessionKind::Main)
            || state
                .positions
                .get(&message.id)
                .is_some_and(|p| matches!(p.origin, Origin::ModelContext))
        {
            message.dialogue_text = Some(String::new());
        } else if let Some(projection) = &mut message.dialogue_text {
            *projection = models::projection(&message.role, projection);
        }
        message.ordinal =
            i64::try_from(ordinal).map_err(|_| AppError::internal("too many Gemini messages"))?;
        message.id = super::parsers::stable_id(
            "message",
            &[Provider::GeminiCli.as_str(), &state.session, &message.id],
        );
    }
    let messages = state.messages;
    let created_at = state
        .created_at
        .or_else(|| messages.iter().filter_map(|m| m.created_at).min());
    let updated_at = state
        .updated_at
        .or_else(|| messages.iter().filter_map(|m| m.created_at).max())
        .or(created_at);
    let title = state
        .summary
        .as_deref()
        .and_then(crate::silver::dialogue::title)
        .or_else(|| {
            messages
                .iter()
                .filter(|m| m.role == crate::role::Role::User)
                .find_map(|m| {
                    crate::silver::dialogue::title(m.dialogue_text.as_deref().unwrap_or_default())
                })
        });
    Ok(ParsedFile {
        conversation: Some(Conversation {
            id: super::parsers::stable_id(
                "session",
                &[Provider::GeminiCli.as_str(), &state.session],
            ),
            provider: Provider::GeminiCli,
            title_origin: None,
            claude_lineage: None,
            source: SourceIdentity::file(path),
            project: Some(project(path, &state.project)?),
            repository: None,
            title,
            created_at,
            updated_at,
            messages: super::messages::Messages::stage(messages)?,
        }),
        malformed_records: 0,
        first_record_issue: None,
    })
}
