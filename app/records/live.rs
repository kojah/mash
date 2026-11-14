//! Quotes from an agent's own running session, read from its transcript.
//!
//! Recording must not wait for indexing, and writing one chat into silver
//! would leave local views uncertified until the next index. A live quote is
//! verified against the provider's file and cited by the message ID indexing
//! will assign. Its evidence reads as stale until that message is retained.
use crate::{
    AppError,
    provider::{Provider, ProviderRoots},
    role::Role,
    silver::HistorySource,
};
use std::path::{Path, PathBuf};

/// A quote to find in one session's transcript.
pub struct Quote {
    pub session_id: String,
    pub text: String,
    /// Endorsements must be the person's own words.
    pub user_only: bool,
}

/// The latest transcript message containing a requested quote.
#[derive(Debug, Clone)]
pub struct Message {
    pub session_id: String,
    pub quote: String,
    pub chat_id: String,
    pub id: String,
    pub role: Role,
    pub content: String,
    pub project: Option<String>,
    pub repository: Option<String>,
}

/// Find each quote in its session's transcript under the configured roots.
///
/// A session with no transcript here, or a quote its transcript lacks, yields
/// nothing: the caller still checks retained history before rejecting.
pub fn read(roots: &ProviderRoots, quotes: &[Quote]) -> Result<Vec<Message>, AppError> {
    let mut sessions: Vec<&str> = quotes
        .iter()
        .map(|quote| quote.session_id.as_str())
        .collect();
    sessions.sort_unstable();
    sessions.dedup();
    let mut found = Vec::new();
    for session in sessions {
        let Some((provider, source)) = transcript(roots, session)? else {
            continue;
        };
        let Some(chat) = crate::silver::read_transcript(provider, &source)? else {
            // The provider is mid-write; a later attempt sees whole records.
            return Err(AppError::source_changed());
        };
        let wanted: Vec<&Quote> = quotes
            .iter()
            .filter(|quote| quote.session_id == session)
            .collect();
        let mut latest: Vec<Option<Message>> = vec![None; wanted.len()];
        for row in chat.messages.rows()? {
            let row = row?;
            for (slot, quote) in latest.iter_mut().zip(&wanted) {
                if (!quote.user_only || row.role == Role::User) && row.content.contains(&quote.text)
                {
                    *slot = Some(Message {
                        session_id: session.to_owned(),
                        quote: quote.text.clone(),
                        chat_id: chat.id.clone(),
                        id: row.id.clone(),
                        role: row.role.clone(),
                        content: row.content.clone(),
                        project: chat.project.clone(),
                        repository: chat.repository.clone(),
                    });
                }
            }
        }
        found.extend(latest.into_iter().flatten());
    }
    Ok(found)
}

/// The session's source under the configured roots. The ID only ever selects
/// a file name or a stored thread; it is never used as a path.
fn transcript(
    roots: &ProviderRoots,
    session: &str,
) -> Result<Option<(Provider, HistorySource)>, AppError> {
    let plain = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_';
    if session.is_empty() || session.len() > 128 || !session.bytes().all(plain) {
        return Ok(None);
    }
    let claude = format!("{session}.jsonl");
    let named = |name: &str| name == claude;
    if let Some(path) = newest(roots.get(&Provider::ClaudeCode), &named) {
        return Ok(Some((Provider::ClaudeCode, HistorySource::file(path))));
    }
    let codex = roots.get(&Provider::Codex);
    for store in codex.into_iter().flatten().filter(|root| {
        root.file_name()
            .is_some_and(|name| name == "thread_history_1.sqlite")
            && root.is_file()
    }) {
        if let Some(source) = crate::silver::codex_session_source(store, session)? {
            // The store names its own rollout files; accept only configured ones.
            let configured = source.identity.session.is_some()
                || codex
                    .into_iter()
                    .flatten()
                    .any(|root| source.identity.path.starts_with(root));
            return Ok(configured.then_some((Provider::Codex, source)));
        }
    }
    let suffix = format!("-{session}.jsonl");
    let rollout = |name: &str| name.starts_with("rollout-") && name.ends_with(&suffix);
    Ok(newest(codex, &rollout).map(|path| (Provider::Codex, HistorySource::file(path))))
}

/// The most recently modified matching transcript beneath the roots.
fn newest(roots: Option<&Vec<PathBuf>>, named: &dyn Fn(&str) -> bool) -> Option<PathBuf> {
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for root in roots.into_iter().flatten() {
        let mut paths = Vec::new();
        collect(root, named, 5, &mut paths);
        for path in paths {
            let Ok(modified) = path.metadata().and_then(|metadata| metadata.modified()) else {
                continue;
            };
            if newest.as_ref().is_none_or(|(seen, _)| modified > *seen) {
                newest = Some((modified, path));
            }
        }
    }
    newest.map(|(_, path)| path)
}

/// Claude Code writes `<project>/<session>.jsonl`; Codex writes dated
/// `rollout-<time>-<session>.jsonl`. Symbolic links are not followed.
fn collect(directory: &Path, named: &dyn Fn(&str) -> bool, depth: u8, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if depth > 0 {
                collect(&entry.path(), named, depth - 1, found);
            }
        } else if kind.is_file() && entry.file_name().to_str().is_some_and(named) {
            found.push(entry.path());
        }
    }
}
