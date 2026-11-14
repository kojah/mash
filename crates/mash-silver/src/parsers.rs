//! Normalize Claude Code and Codex JSONL histories into canonical records.
use super::json::{Kind, Record};
use super::{Chat, NormalizedMessage, ParsedFile};
use crate::AppError;
use crate::provider::Provider;
use chrono::DateTime;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

mod claude;
mod codex;

struct ExtractedMessage {
    attachments: Vec<crate::attachments::Source>,
    tools: crate::tools::Activity,
    source_id: Option<String>,
    role: crate::role::Role,
    content: String,
    dialogue_text: Option<String>,
    created_at: Option<i64>,
    session_id: Option<String>,
}

impl ExtractedMessage {
    fn normalized(self, id: String, ordinal: i64) -> NormalizedMessage {
        NormalizedMessage {
            attachments: self.attachments,
            id,
            ordinal,
            context: crate::message_context::MessageContext::Linear,
            role: self.role,
            content: self.content,
            dialogue_text: self.dialogue_text,
            created_at: self.created_at,
            tools: self.tools,
        }
    }
}

pub fn parse_claude(path: &Path) -> Result<ParsedFile, AppError> {
    let file = File::open(path).map_err(AppError::io)?;
    parse_claude_file(path, file)
}

fn parse_claude_file(path: &Path, file: File) -> Result<ParsedFile, AppError> {
    let lines = super::records::Lines::new(BufReader::new(file))
        .enumerate()
        .map(|(index, line)| line.map(|text| (index + 1, text)));
    parse_jsonl(path, Provider::ClaudeCode, lines, |raw| {
        if !matches!(raw.kind, Kind::User | Kind::Assistant) {
            return None;
        }
        let message = raw.message.as_ref()?;
        let (content, dialogue_text) = message.content.as_ref()?.projected()?;
        Some(ExtractedMessage {
            attachments: message.content.as_ref()?.attachments(),
            tools: message.content.as_ref()?.tools(),
            source_id: raw.uuid.clone(),
            role: message
                .role
                .clone()
                .unwrap_or_else(|| raw.kind.as_str().into()),
            content,
            dialogue_text,
            created_at: parse_timestamp(raw.timestamp.as_deref()),
            session_id: raw.session_id.clone(),
        })
    })
}

pub fn parse_codex(path: &Path) -> Result<ParsedFile, AppError> {
    let file = File::open(path).map_err(AppError::io)?;
    parse_codex_file(path, file)
}

fn parse_codex_file(path: &Path, file: File) -> Result<ParsedFile, AppError> {
    let lines = super::records::codex::Lines::new(BufReader::new(file)).numbered();
    let mut extractor = codex::Extractor::default();
    parse_jsonl(path, Provider::Codex, lines, |raw| extractor.next(raw))
}

pub fn parse_captured(path: &Path, provider: Provider, file: File) -> Result<ParsedFile, AppError> {
    match provider {
        Provider::ClaudeCode => parse_claude_file(path, file),
        Provider::Codex => parse_codex_file(path, file),
        _ => Err(AppError::internal("invalid captured JSONL provider")),
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one pass decodes, interrupts replays at damage, and normalizes records"
)]
fn parse_jsonl(
    path: &Path,
    provider: Provider,
    lines: impl Iterator<Item = Result<(usize, String), AppError>>,
    mut extract: impl FnMut(&Record) -> Option<ExtractedMessage>,
) -> Result<ParsedFile, AppError> {
    let mut rejected = RejectedRecords::default();
    let mut messages = super::messages::Builder::default();
    let mut session_id = None;
    let mut project = None;
    let mut repository = None;
    let mut claude_identity = claude::Identity::default();
    let mut named = None;
    for line in lines {
        let (number, line) = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Some(raw) = rejected.decode(path, provider, number, &line) else {
            // An unreadable record interrupts a Claude replay: a replay that
            // spans it cannot be proven and later raises conflicting ancestry.
            claude_identity.interrupt();
            rejected.readable_after = false;
            continue;
        };
        // A readable record after a corrupt one shows the file moved past it:
        // the damage is permanent, not a write still in progress.
        rejected.readable_after = rejected.count != 0;
        capture_metadata(&raw, &mut project, &mut repository);
        // The agent renames a session as it goes; the latest name stands.
        if provider == Provider::ClaudeCode
            && let Some(title) = raw.custom_title.as_deref().and_then(crate::dialogue::title)
        {
            named = Some(title);
        }
        if provider == Provider::ClaudeCode
            && let Err(error) = claude_identity.observe(&raw)
        {
            return rejected.unprovable(error);
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
            if let Err(error) = claude_identity.capture_session(extracted.session_id.take()) {
                return rejected.unprovable(error);
            }
        } else if session_id.is_none() {
            session_id = extracted.session_id.take();
        }
        let ordinal = i64::try_from(messages.len())
            .map_err(|_| AppError::internal("chat contains too many messages"))?;
        if provider == Provider::ClaudeCode
            && let Err(error) = claude_identity.push_key(extracted.source_id.clone())
        {
            return rejected.unprovable(error);
        }
        let message_key = extracted
            .source_id
            .take()
            .unwrap_or_else(|| ordinal.to_string());
        let id = stable_id(
            "message",
            &[provider.as_str(), &path.to_string_lossy(), &message_key],
        );
        messages.push(extracted.normalized(id, ordinal))?;
    }
    // A corrupt final record may be a write in progress: keep the previous
    // contribution until a clean read. Interior damage never heals, so the
    // readable records are applied and the skipped ones still reported.
    if (rejected.count != 0 && !rejected.readable_after) || messages.len() == 0 {
        return Ok(ParsedFile {
            chat: None,
            malformed_records: rejected.count,
            first_record_issue: rejected.first,
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
        let (normalized, lineage) = match claude_identity.normalize(&id, messages) {
            Ok(normalized) => normalized,
            Err(error) => return rejected.unprovable(error),
        };
        messages = normalized;
        Some(lineage)
    } else {
        None
    };
    let summary = messages.summary()?;
    Ok(ParsedFile {
        chat: Some(Chat {
            id,
            provider,
            claude_lineage,
            title_origin: None,
            timestamp_origins: None,
            source: super::SourceIdentity::file(path),
            repository: repository.or_else(|| project.as_deref().and_then(repository_origin)),
            project,
            // A session's own name says what it was about; its first message
            // is the fallback for sessions that were never named.
            title: named.or(summary.title),
            created_at: summary.created_at,
            updated_at: summary.updated_at,
            messages,
        }),
        malformed_records: rejected.count,
        first_record_issue: rejected.first,
    })
}

#[derive(Default)]
struct RejectedRecords {
    count: u64,
    first: Option<super::diagnostics::RecordIssue>,
    /// A record decoded after the last corrupt one.
    readable_after: bool,
}

impl RejectedRecords {
    /// Ancestry that skipped records leave unprovable rejects the snapshot,
    /// keeping the previous contribution; without skips the error stands.
    fn unprovable(self, error: AppError) -> Result<ParsedFile, AppError> {
        if self.count == 0 {
            return Err(error);
        }
        Ok(ParsedFile {
            chat: None,
            malformed_records: self.count,
            first_record_issue: self.first,
        })
    }

    fn decode(
        &mut self,
        path: &Path,
        provider: Provider,
        line: usize,
        text: &str,
    ) -> Option<Record> {
        match serde_json::from_str(text) {
            Ok(record) => Some(record),
            Err(error) => {
                self.count += 1;
                self.first.get_or_insert_with(|| {
                    super::diagnostics::RecordIssue::json(provider, path, line, &error)
                });
                None
            }
        }
    }
}

fn clean_projection(message: &mut ExtractedMessage) {
    let projection = message.dialogue_text.as_deref().unwrap_or(&message.content);
    let clean = crate::dialogue::visible(&message.role, projection);
    if clean.as_ref() != projection {
        message.dialogue_text = Some(clean.into_owned());
    }
}

/// A working folder as people write it. Windows programs may record the
/// extended-length form (`\\?\C:\work`), which names the same folder as
/// `C:\work`; kept as written, one project would be listed twice.
pub fn folder(path: &str) -> String {
    match path.strip_prefix(r"\\?\UNC\") {
        Some(share) => format!(r"\\{share}"),
        None => path.strip_prefix(r"\\?\").unwrap_or(path).to_owned(),
    }
}

#[test]
fn folder_drops_the_windows_extended_length_prefix_only() {
    assert_eq!(folder(r"\\?\C:\work\disk-report"), r"C:\work\disk-report");
    assert_eq!(folder(r"\\?\UNC\host\share\repo"), r"\\host\share\repo");
    for same in [r"C:\work", r"\\host\share", "/home/me/work", ""] {
        assert_eq!(folder(same), same);
    }
}

pub fn capture_metadata(
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
        *project = meta
            .cwd
            .as_ref()
            .filter(|value| !value.is_empty())
            .map(|value| folder(value));
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

pub fn stable_id(domain: &str, parts: &[&str]) -> String {
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
