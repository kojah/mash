//! Native Cursor Agent and IDE histories, selected by their physical schemas.
use super::{Chat, HistorySource, NormalizedMessage, ParsedFile, SourceIdentity, records};
use crate::{AppError, message_context::MessageContext, provider::Provider, role::Role};
use std::{collections::HashMap, path::Path};

mod evidence;
mod ide;
mod models;
mod proto;
mod store;
#[cfg(test)]
mod tests;
mod text;
use store::Store;

fn invalid(message: &str) -> AppError {
    AppError::configuration(format!("Cursor Agent: {message}"))
}

pub(super) fn discover(path: &Path) -> Result<Vec<HistorySource>, AppError> {
    if format(path)? == Format::Ide {
        return ide::discover(path);
    }
    let store = Store::open(path)?;
    let metadata = store.metadata()?;
    Ok(vec![HistorySource {
        content: None,
        identity: SourceIdentity {
            path: path.into(),
            session: Some(metadata.agent_id),
        },
        activity_ns: None,
        estimated_bytes: None,
        dependencies: Vec::new(),
    }])
}

#[derive(Default)]
struct Messages {
    items: Vec<NormalizedMessage>,
    occurrences: HashMap<String, u64>,
    bytes: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    Archive,
    Current,
}

impl Messages {
    fn push(
        &mut self,
        store: &mut Store,
        id: &[u8],
        session: &str,
        origin: Origin,
        child: bool,
        recorded: &mut evidence::Recorded,
    ) -> Result<(), AppError> {
        let message: models::Message = records::decode(&store.blob(id)?)?;
        let mut text = text::normalized(&message);
        let evidence = if origin == Origin::Archive {
            None
        } else {
            recorded
                .get_mut(&evidence::key(&message.role, &text.projection))
                .and_then(std::collections::VecDeque::pop_front)
        };
        if child
            || message.summary()
            || evidence.is_some_and(|e| !e.visible)
            || (origin == Origin::Current && message.role == Role::User && evidence.is_none())
        {
            text.projection.clear();
        }
        if text.canonical.trim().is_empty() {
            return Ok(());
        }
        self.bytes = self
            .bytes
            .saturating_add(text.canonical.capacity())
            .saturating_add(text.projection.capacity())
            .saturating_add(512);
        if self.bytes > 32 * 1024 * 1024 || self.items.len() == 65_536 {
            return Err(invalid(
                "normalized history exceeds the retained replay limit",
            ));
        }
        let fingerprint =
            super::parsers::stable_id("turn", &[message.role.as_str(), &text.canonical]);
        let occurrence = self.occurrences.entry(fingerprint.clone()).or_default();
        let message_id = super::parsers::stable_id(
            "message",
            &[
                Provider::Cursor.as_str(),
                session,
                &fingerprint,
                &occurrence.to_string(),
            ],
        );
        *occurrence += 1;
        self.items.push(NormalizedMessage {
            attachments: Vec::new(),
            id: message_id,
            ordinal: i64::try_from(self.items.len()).map_err(|_| invalid("too many messages"))?,
            context: MessageContext::Linear,
            role: message.role,
            content: text.canonical,
            dialogue_text: Some(text.projection),
            created_at: evidence.and_then(|e| e.timestamp),
            tools: crate::tools::Activity::default(),
        });
        Ok(())
    }
}

pub(super) fn parse(source: &SourceIdentity) -> Result<ParsedFile, AppError> {
    if format(&source.path)? == Format::Ide {
        return ide::parse(source);
    }
    parse_agent(source)
}

#[derive(PartialEq, Eq)]
enum Format {
    Agent,
    Ide,
}

fn format(path: &Path) -> Result<Format, AppError> {
    let connection = super::sqlite_source::open(path)?;
    let mut statement = connection
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name IN ('meta','blobs','ItemTable','cursorDiskKV')")
        .map_err(AppError::database)?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(AppError::database)?
        .collect::<Result<std::collections::BTreeSet<_>, _>>()
        .map_err(AppError::database)?;
    match (
        names.contains("meta") && names.contains("blobs"),
        names.contains("ItemTable") && names.contains("cursorDiskKV"),
    ) {
        (true, false) => Ok(Format::Agent),
        (false, true) => Ok(Format::Ide),
        _ => Err(AppError::configuration(
            "unsupported or ambiguous Cursor storage schema",
        )),
    }
}

fn parse_agent(source: &SourceIdentity) -> Result<ParsedFile, AppError> {
    let mut store = Store::open(&source.path)?;
    let metadata = store.metadata()?;
    if source.session.as_deref() != Some(metadata.agent_id.as_str()) {
        return Err(invalid("source and native session identities disagree"));
    }
    let id =
        hex::decode(&metadata.latest_root_blob_id).map_err(|_| invalid("invalid root identity"))?;
    let root = if id.is_empty() {
        proto::Root::default()
    } else {
        store.protobuf::<proto::Root>(&id, &[21])?
    };
    if !root.turns.is_empty() && root.messages.is_empty() && root.archives.is_empty() {
        return Err(invalid("native turns have no reachable transcript history"));
    }
    let mut recorded = evidence::current(&mut store, &root)?;
    let mut messages = Messages::default();
    for id in &root.archives {
        let archive: proto::Archive = store.protobuf(id, &[])?;
        for id in archive.messages {
            messages.push(
                &mut store,
                &id,
                &metadata.agent_id,
                Origin::Archive,
                metadata.subagent_info.is_some(),
                &mut recorded,
            )?;
        }
    }
    for id in root.messages {
        messages.push(
            &mut store,
            &id,
            &metadata.agent_id,
            Origin::Current,
            metadata.subagent_info.is_some(),
            &mut recorded,
        )?;
    }
    let created_at = evidence::timestamp(root.started_at)?.or(Some(metadata.created_at));
    let updated_at = messages
        .items
        .iter()
        .filter_map(|m| m.created_at)
        .max()
        .or(created_at);
    let project = unique(
        root.workspaces
            .into_iter()
            .filter_map(|uri| crate::platform::file_uri_path(&uri)),
    );
    let repository = unique(
        root.repositories
            .into_iter()
            .map(|r| r.path)
            .filter(|p| !p.trim().is_empty()),
    );
    let title = crate::silver::dialogue::title(&metadata.name)
        .filter(|n| n != "New Agent")
        .or_else(|| {
            messages
                .items
                .iter()
                .filter(|m| m.role == Role::User)
                .find_map(|m| {
                    crate::silver::dialogue::title(m.dialogue_text.as_deref().unwrap_or_default())
                })
        });
    Ok(ParsedFile {
        chat: Some(Chat {
            id: super::parsers::stable_id(
                "session",
                &[Provider::Cursor.as_str(), &metadata.agent_id],
            ),
            provider: Provider::Cursor,
            title_origin: None,
            timestamp_origins: None,
            claude_lineage: None,
            source: source.clone(),
            project,
            repository,
            title,
            created_at,
            updated_at,
            messages: super::messages::Messages::stage(messages.items)?,
        }),
        malformed_records: 0,
        first_record_issue: None,
    })
}

fn unique(values: impl Iterator<Item = String>) -> Option<String> {
    let values = values.collect::<std::collections::BTreeSet<_>>();
    (values.len() == 1)
        .then(|| values.into_iter().next())
        .flatten()
}
