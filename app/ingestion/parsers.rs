//! Normalize Claude Code and Codex JSONL histories into canonical records.
use super::json::{Kind, Record};
use super::{Conversation, NormalizedMessage, ParsedFile};
use crate::AppError;
use crate::provider::Provider;
use chrono::DateTime;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

mod claude;
mod codex;

struct ExtractedMessage {
    source_id: Option<String>,
    role: crate::role::Role,
    content: String,
    search_projection: Option<String>,
    created_at: Option<i64>,
    session_id: Option<String>,
}

pub(super) fn parse_claude(path: &Path) -> Result<ParsedFile, AppError> {
    let file = File::open(path).map_err(AppError::io)?;
    let lines = super::records::Lines::new(BufReader::new(file));
    parse_jsonl(path, Provider::ClaudeCode, lines, |raw| {
        if !matches!(raw.kind, Kind::User | Kind::Assistant) {
            return None;
        }
        let message = raw.message.as_ref()?;
        let (content, search_projection) = message.content.as_ref()?.projected()?;
        Some(ExtractedMessage {
            source_id: raw.uuid.clone(),
            role: message
                .role
                .clone()
                .unwrap_or_else(|| raw.kind.as_str().into()),
            content,
            search_projection,
            created_at: parse_timestamp(raw.timestamp.as_deref()),
            session_id: raw.session_id.clone(),
        })
    })
}

pub(super) fn parse_codex(path: &Path) -> Result<ParsedFile, AppError> {
    let file = File::open(path).map_err(AppError::io)?;
    let lines = super::records::codex::Lines::new(BufReader::new(file));
    let mut extractor = codex::Extractor::default();
    parse_jsonl(path, Provider::Codex, lines, |raw| extractor.next(raw))
}

fn parse_jsonl(
    path: &Path,
    provider: Provider,
    lines: impl Iterator<Item = Result<String, AppError>>,
    mut extract: impl FnMut(&Record) -> Option<ExtractedMessage>,
) -> Result<ParsedFile, AppError> {
    let mut malformed_records = 0;
    let mut messages = super::messages::Builder::default();
    let mut session_id = None;
    let mut project = None;
    let mut repository = None;
    let mut claude_identity = claude::Identity::default();
    for line in lines {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let raw: Record = if let Ok(raw) = serde_json::from_str(&line) {
            raw
        } else {
            malformed_records += 1;
            continue;
        };
        capture_metadata(&raw, &mut project, &mut repository);
        if provider == Provider::ClaudeCode {
            claude_identity.observe(&raw)?;
        }
        if provider == Provider::Codex && session_id.is_none() && raw.kind == Kind::SessionMeta {
            session_id = raw
                .payload
                .as_ref()
                .and_then(|payload| payload.id.as_ref().or(payload.session_id.as_ref()))
                .cloned();
        }
        let Some(mut extracted) = extract(&raw) else {
            continue;
        };
        if extracted.content.trim().is_empty() {
            continue;
        }
        clean_projection(&mut extracted);
        if provider == Provider::ClaudeCode {
            claude_identity.capture_session(extracted.session_id.take())?;
        } else if session_id.is_none() {
            session_id = extracted.session_id.take();
        }
        let ordinal = i64::try_from(messages.len())
            .map_err(|_| AppError::internal("conversation contains too many messages"))?;
        if provider == Provider::ClaudeCode {
            claude_identity.keys.push(extracted.source_id.clone());
        }
        let message_key = extracted.source_id.unwrap_or_else(|| ordinal.to_string());
        messages.push(NormalizedMessage {
            id: stable_id(
                "message",
                &[provider.as_str(), &path.to_string_lossy(), &message_key],
            ),
            ordinal,
            context: crate::message_context::MessageContext::Linear,
            role: extracted.role,
            content: extracted.content,
            search_projection: extracted.search_projection,
            created_at: extracted.created_at,
        })?;
    }
    if messages.len() == 0 {
        return Ok(ParsedFile {
            conversation: None,
            malformed_records,
        });
    }
    let id = session_id
        .unwrap_or_else(|| stable_id("session", &[provider.as_str(), &path.to_string_lossy()]));
    let id = claude_identity.thread_id(&id);
    let mut messages = messages.finish()?;
    if messages.is_empty() {
        return Err(AppError::internal("parser lost normalized messages"));
    }
    let claude_lineage = if provider == Provider::ClaudeCode {
        let (normalized, lineage) = claude_identity.normalize(&id, messages)?;
        messages = normalized;
        Some(lineage)
    } else {
        None
    };
    let summary = messages.summary()?;
    Ok(ParsedFile {
        conversation: Some(Conversation {
            id,
            provider,
            claude_lineage,
            source: super::SourceIdentity::file(path),
            repository: repository.or_else(|| project.as_deref().and_then(repository_origin)),
            project,
            title: summary.title,
            created_at: summary.created_at,
            updated_at: summary.updated_at,
            messages,
        }),
        malformed_records,
    })
}

fn clean_projection(message: &mut ExtractedMessage) {
    let projection = message
        .search_projection
        .as_deref()
        .unwrap_or(&message.content);
    let clean = crate::search_text::project(&message.role, projection);
    if clean.as_ref() != projection {
        message.search_projection = Some(clean.into_owned());
    }
}

pub(super) fn capture_metadata(
    raw: &Record,
    project: &mut Option<String>,
    repository: &mut Option<String>,
) {
    let meta = if raw.kind == Kind::SessionMeta {
        raw.payload
            .as_ref()
            .map_or(&raw.metadata, |payload| &payload.metadata)
    } else {
        &raw.metadata
    };
    if project.is_none() {
        *project = meta.cwd.as_ref().filter(|value| !value.is_empty()).cloned();
    }
    if repository.is_none() {
        *repository = meta
            .git
            .as_ref()
            .and_then(|git| git.repository_url.as_ref())
            .filter(|value| !value.is_empty())
            .cloned();
    }
}

fn repository_origin(project: &str) -> Option<String> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, Option<String>>>,
    > = std::sync::OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().ok()?;
    cache
        .entry(project.to_owned())
        .or_insert_with(|| {
            if !Path::new(project).is_absolute() {
                return None;
            }
            let output = std::process::Command::new("git")
                .args(["-C", project, "config", "--get", "remote.origin.url"])
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            let url = String::from_utf8(output.stdout).ok()?.trim().to_owned();
            (!url.is_empty()).then_some(url)
        })
        .clone()
}

pub(super) fn stable_id(domain: &str, parts: &[&str]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(domain.as_bytes());
    for part in parts {
        hasher.update(&[0]);
        hasher.update(part.as_bytes());
    }
    hasher.finalize().to_hex()[..24].to_owned()
}

fn parse_timestamp(value: Option<&str>) -> Option<i64> {
    DateTime::parse_from_rfc3339(value?)
        .ok()
        .map(|timestamp| timestamp.timestamp_millis())
}

fn truncate_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}
